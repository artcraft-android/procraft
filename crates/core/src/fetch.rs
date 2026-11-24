//! Reading a web address: one GET, a cap on what comes back, and a limit on how long it may take.
//! Nothing here touches a socket unless a catalogue is read from the web, so a store of files on
//! this machine never reaches the network.

use std::io::Read;
use std::time::Duration;

/// How long one read may take, from connecting through to the last byte.
pub(crate) const FETCH_TIME: Duration = Duration::from_secs(30);

/// How a fetch is made, so tests can answer without a network.
pub(crate) trait Transport: Send + Sync {
    /// The bytes `address` answers with, up to `cap` of them, or why it could not be read.
    fn get(&self, address: &str, cap: usize, timeout: Duration) -> Result<Vec<u8>, String>;
}

/// The web, read through ureq over rustls with its own root certificates.
pub(crate) struct Web;

impl Transport for Web {
    fn get(&self, address: &str, cap: usize, timeout: Duration) -> Result<Vec<u8>, String> {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .http_status_as_error(false)
            .max_redirects(0)
            .build();
        let agent: ureq::Agent = config.into();
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
        read_capped(response.body_mut().as_reader(), cap)
    }
}

/// Reads all of `reader`, refusing as soon as it holds more than `cap` bytes.
pub(crate) fn read_capped(mut reader: impl Read, cap: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let read = reader.read(&mut buf).map_err(|e| format!("it stopped in the middle: {e}"))?;
        if read == 0 {
            break;
        }
        if bytes.len() + read > cap {
            return Err(format!("it is larger than the {cap} bytes it may be"));
        }
        bytes.extend_from_slice(buf.get(..read).unwrap_or_default());
    }
    Ok(bytes)
}

/// Whether `address` is one this build reads, and why not when it is not.
pub(crate) fn web_address(address: &str) -> Result<(), String> {
    if address.starts_with("https://") {
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

    #[test]
    fn a_read_stops_at_the_cap_and_not_a_byte_after() {
        let fitting = read_capped(&b"12345"[..], 5);
        assert_eq!(fitting, Ok(b"12345".to_vec()));
        let over = read_capped(&b"123456"[..], 5);
        assert!(over.is_err(), "one byte past the cap is refused");
        assert!(read_capped(&b""[..], 0).is_ok(), "an empty read of nothing is fine");
    }

    #[test]
    fn only_https_is_an_address_the_store_reads() {
        assert!(web_address("https://example.org/x.json").is_ok());
        assert!(web_address("http://example.org/x.json").is_err(), "plain http is refused");
        assert!(web_address("/home/me/x.json").is_err(), "a path is not a web address");
    }
}
