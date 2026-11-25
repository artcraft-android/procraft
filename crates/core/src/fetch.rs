//! Reading a web address: one GET, a cap on what comes back, and a limit on how long it may take.
//! Nothing here touches a socket unless a catalogue is read from the web, so a store of files on
//! this machine never reaches the network.

use std::io::{Read, Write};
use std::net::IpAddr;
use std::time::Duration;
use ureq::http::Uri;
use ureq::unversioned::{
    resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver},
    transport::{DefaultConnector, NextTimeout},
};

/// How long one read may take, from connecting through to the last byte.
pub(crate) const FETCH_TIME: Duration = Duration::from_secs(30);

/// How a fetch is made, so tests can answer without a network.
pub(crate) trait Transport: Send + Sync {
    /// The bytes `address` answers with, up to `cap` of them, or why it could not be read.
    fn get(&self, address: &str, cap: usize, timeout: Duration) -> Result<Vec<u8>, String>;

    fn get_scoped(
        &self,
        address: &str,
        cap: usize,
        timeout: Duration,
        local_origin: Option<&str>,
    ) -> Result<Vec<u8>, String> {
        let mut bytes = Vec::new();
        self.get_into_scoped(address, cap, timeout, local_origin, &mut bytes)?;
        Ok(bytes)
    }

    fn get_into_scoped(
        &self,
        address: &str,
        cap: usize,
        timeout: Duration,
        _local_origin: Option<&str>,
        out: &mut dyn Write,
    ) -> Result<usize, String> {
        let bytes = self.get(address, cap, timeout)?;
        copy_capped(bytes.as_slice(), cap, out)
    }
}

/// The web, read through ureq over rustls with its own root certificates.
pub(crate) struct Web;

impl Transport for Web {
    fn get(&self, address: &str, cap: usize, timeout: Duration) -> Result<Vec<u8>, String> {
        let mut bytes = Vec::new();
        self.get_into_scoped(address, cap, timeout, None, &mut bytes)?;
        Ok(bytes)
    }

    fn get_into_scoped(
        &self,
        address: &str,
        cap: usize,
        timeout: Duration,
        local_origin: Option<&str>,
        out: &mut dyn Write,
    ) -> Result<usize, String> {
        web_address(address)?;
        let allow_local = local_origin.is_some_and(|origin| same_origin(origin, address));
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .max_redirects(0)
            .proxy(None)
            .build();
        let agent = ureq::Agent::with_parts(config, DefaultConnector::default(), StoreResolver { allow_local });
        let mut response = agent.get(address).call().map_err(|e| match e {
            ureq::Error::Timeout(_) => "it took too long to answer".to_owned(),
            other => format!("it could not be reached: {other}"),
        })?;
        let status = response.status();
        if status.is_redirection() {
            return Err(format!(
                "it answers {} and points somewhere else; add the address it points at",
                status.as_u16()
            ));
        }
        if !status.is_success() {
            return Err(format!("it answers {}", status.as_u16()));
        }
        copy_capped(response.body_mut().as_reader(), cap, out)
    }
}

// Filter the exact DNS results the connector will use. No second lookup can change them.
#[derive(Debug)]
struct StoreResolver {
    allow_local: bool,
}

impl Resolver for StoreResolver {
    fn resolve(
        &self,
        uri: &Uri,
        config: &ureq::config::Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let addresses = DefaultResolver::default().resolve(uri, config, timeout)?;
        let mut allowed = self.empty();
        for address in addresses.iter() {
            if allowed_ip(address.ip(), self.allow_local) {
                allowed.push(*address);
            }
        }
        if allowed.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "the store blocks this address because it is outside the allowed network",
            )
            .into());
        }
        Ok(allowed)
    }
}

fn allowed_ip(ip: IpAddr, allow_local: bool) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            if ip.is_private() || ip.is_loopback() || ip.is_link_local() {
                return allow_local;
            }
            !matches!(
                ip.octets(),
                [0 | 127 | 224..=255, ..]
                    | [100, 64..=127, ..]
                    | [192, 0, 0 | 2, _]
                    | [192, 88, 99, _]
                    | [198, 18 | 19, ..]
                    | [198, 51, 100, _]
                    | [203, 0, 113, _]
            )
        }
        IpAddr::V6(ip) => {
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return allowed_ip(IpAddr::V4(mapped), allow_local);
            }
            if ip.is_unique_local() || ip.is_loopback() || ip.is_unicast_link_local() {
                return allow_local;
            }
            matches!(ip.segments(), [0x2000..=0x3fff, ..])
                && !matches!(ip.segments(), [0x2001, 0..=0x1ff | 0xdb8, ..] | [0x2002 | 0x3fff, ..])
        }
    }
}

fn same_origin(left: &str, right: &str) -> bool {
    let (Ok(left), Ok(right)) = (left.parse::<Uri>(), right.parse::<Uri>()) else { return false };
    web_address(&left.to_string()).is_ok()
        && web_address(&right.to_string()).is_ok()
        && left.host().zip(right.host()).is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
        && left.port_u16().unwrap_or(443) == right.port_u16().unwrap_or(443)
}

/// Copies a response without holding it in memory, enforcing the cap as it goes.
fn copy_capped(mut reader: impl Read, cap: usize, out: &mut dyn Write) -> Result<usize, String> {
    let mut count = 0;
    let mut buf = [0u8; 8192];
    loop {
        let read = reader.read(&mut buf).map_err(|e| format!("it stopped in the middle: {e}"))?;
        if read == 0 {
            break;
        }
        if read > cap - count {
            return Err(format!("it is larger than the {cap} bytes it may be"));
        }
        let part = buf.get(..read).ok_or("the download reader returned an invalid byte count")?;
        out.write_all(part).map_err(|e| format!("the download could not be saved: {e}"))?;
        count += read;
    }
    Ok(count)
}

/// Whether `address` is one this build reads, and why not when it is not.
pub(crate) fn web_address(address: &str) -> Result<(), String> {
    if address.starts_with("https://") {
        let uri = address.parse::<Uri>().map_err(|_| "it is not a valid HTTPS address".to_owned())?;
        if uri.authority().is_none_or(|a| a.host().is_empty() || a.as_str().contains('@')) {
            return Err("it needs an HTTPS host without a username or password".to_owned());
        }
        Ok(())
    } else if address.starts_with("http://") {
        Err("it is not https, and the store only reads https".to_owned())
    } else {
        Err("it is not a web address".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_capped(reader: impl Read, cap: usize) -> Result<Vec<u8>, String> {
        let mut bytes = Vec::new();
        copy_capped(reader, cap, &mut bytes)?;
        Ok(bytes)
    }

    #[test]
    fn a_read_stops_at_the_cap_and_not_a_byte_after() {
        let fitting = read_capped(&b"12345"[..], 5);
        assert_eq!(fitting, Ok(b"12345".to_vec()));
        let over = read_capped(&b"123456"[..], 5);
        assert!(over.is_err(), "one byte past the cap is refused");
        assert!(read_capped(&b""[..], 0).is_ok(), "an empty read of nothing is fine");
    }

    #[test]
    fn local_network_grants_are_limited_to_the_catalogue_origin() {
        assert!(same_origin("https://Local.test/catalogue", "https://local.test:443/image"));
        assert!(!same_origin("https://local.test/catalogue", "https://other.test/image"));
        assert!(!same_origin("https://local.test:8443/catalogue", "https://local.test/image"));
        assert!(!same_origin("https://local.test/catalogue", "https://name@local.test/image"));
        for address in ["10.1.2.3", "127.0.0.1", "169.254.169.254", "::1", "fd00::1", "::ffff:192.168.1.3"] {
            let ip = address.parse().unwrap();
            assert!(!allowed_ip(ip, false), "{address}");
            assert!(allowed_ip(ip, true), "{address}");
        }
        for address in [
            "0.0.0.0",
            "100.64.1.2",
            "192.0.2.1",
            "198.19.1.2",
            "224.0.0.1",
            "::",
            "64:ff9b::a00:1",
            "2001:db8::1",
            "2002:a00:1::1",
            "ff02::1",
        ] {
            assert!(!allowed_ip(address.parse().unwrap(), true), "{address}");
        }
        assert!(allowed_ip("8.8.8.8".parse().unwrap(), false));
        assert!(allowed_ip("2606:4700::1111".parse().unwrap(), false));
    }

    #[test]
    fn literal_loopback_is_stopped_before_connecting() {
        let refused = Web.get("https://127.0.0.1/catalogue.json", 100, Duration::from_secs(2)).unwrap_err();
        assert!(refused.contains("outside the allowed network"), "{refused}");
    }

    #[test]
    fn only_https_is_an_address_the_store_reads() {
        assert!(web_address("https://example.org/x.json").is_ok());
        assert!(web_address("http://example.org/x.json").is_err(), "plain http is refused");
        assert!(web_address("/home/me/x.json").is_err(), "a path is not a web address");
    }
}
