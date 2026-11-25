//! A bounded file protocol between the Store and its platform decoder process.
use std::io::{self, BufReader, BufWriter, Read, Seek, Write};
use std::sync::{Arc, Mutex, OnceLock};

#[derive(Clone, Copy, uniffi::Enum)]
pub enum StoreDecodeKind {
    Cover,
    Gallery,
    BrushSet,
    Plugin,
    Webm,
    Mp4,
}

#[uniffi::export(with_foreign)]
pub trait StoreDecoder: Send + Sync {
    fn decode(&self, kind: StoreDecodeKind, input: String, output: String) -> Option<String>;
}

static DECODER: OnceLock<Arc<dyn StoreDecoder>> = OnceLock::new();
static DECODING: Mutex<()> = Mutex::new(());

#[uniffi::export]
pub fn set_store_decoder(decoder: Arc<dyn StoreDecoder>) {
    let _ = DECODER.set(decoder);
}

impl StoreDecodeKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Cover => "cover",
            Self::Gallery => "gallery",
            Self::BrushSet => "brushset",
            Self::Plugin => "plugin",
            Self::Webm => "webm",
            Self::Mp4 => "mp4",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "cover" => Some(Self::Cover),
            "gallery" => Some(Self::Gallery),
            "brushset" => Some(Self::BrushSet),
            "plugin" => Some(Self::Plugin),
            "webm" => Some(Self::Webm),
            "mp4" => Some(Self::Mp4),
            _ => None,
        }
    }

    fn output_cap(self) -> u64 {
        match self {
            Self::Cover => 3 << 20,
            Self::Gallery => 50 << 20,
            Self::BrushSet => 512 << 20,
            Self::Plugin => 65 << 20,
            Self::Webm | Self::Mp4 => 1024,
        }
    }
}

const MAGIC: &[u8] = b"PCSD\x01";
const BAD_RESPONSE: &str = "The Store decoder returned invalid data.";

pub(crate) fn decode(kind: StoreDecodeKind, input: &std::path::Path) -> Result<BufReader<std::fs::File>, String> {
    let _one = DECODING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut output = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    if let Some(decoder) = DECODER.get() {
        if let Some(error) =
            decoder.decode(kind, input.to_string_lossy().into_owned(), output.path().to_string_lossy().into_owned())
        {
            return Err(error);
        }
    } else {
        #[cfg(not(test))]
        return Err("The Store decoder is unavailable on this platform.".to_owned());
        #[cfg(test)]
        if !run(
            kind,
            std::fs::File::open(input).map_err(|e| e.to_string())?,
            output.reopen().map_err(|e| e.to_string())?,
        ) {
            return Err("The Store decoder failed.".to_owned());
        }
    }
    if output.as_file().metadata().map_err(|e| e.to_string())?.len() > kind.output_cap() {
        return Err(BAD_RESPONSE.to_owned());
    }
    output.as_file_mut().rewind().map_err(|e| e.to_string())?;
    let mut read = BufReader::new(output.into_file());
    let mut header = [0; MAGIC.len() + 1];
    read.read_exact(&mut header).map_err(|_| BAD_RESPONSE)?;
    if header.get(..MAGIC.len()) != Some(MAGIC) {
        return Err(BAD_RESPONSE.to_owned());
    }
    match header.last() {
        Some(0) => Ok(read),
        Some(1) => {
            Err(String::from_utf8(read_blob(&mut read, 1024).map_err(|_| BAD_RESPONSE)?).map_err(|_| BAD_RESPONSE)?)
        }
        _ => Err(BAD_RESPONSE.to_owned()),
    }
}

pub(crate) fn decode_bytes(kind: StoreDecodeKind, input: &[u8]) -> Result<BufReader<std::fs::File>, String> {
    let mut file = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
    file.write_all(input).map_err(|e| e.to_string())?;
    decode(kind, file.path())
}

/// Entry point for a decoder process. Its input is read-only and its output is a fresh file.
pub fn run_store_decoder(kind: StoreDecodeKind, input: std::fs::File, output: std::fs::File) -> bool {
    run(kind, input, output)
}

fn run(kind: StoreDecodeKind, input: std::fs::File, mut file: std::fs::File) -> bool {
    let result = {
        let mut bounded = Bounded { remaining: kind.output_cap(), file: &mut file };
        let mut out = BufWriter::new(&mut bounded);
        (|| {
            out.write_all(MAGIC).and_then(|()| out.write_all(&[0])).map_err(|e| e.to_string())?;
            match kind {
                StoreDecodeKind::Cover | StoreDecodeKind::Gallery => {
                    crate::store_media::decode_images(kind, input, &mut out)?
                }
                StoreDecodeKind::BrushSet => crate::brush_library::decode_store_set(input, &mut out)?,
                StoreDecodeKind::Plugin => crate::plugin_file::decode_store_plugin(input, &mut out)?,
                StoreDecodeKind::Webm | StoreDecodeKind::Mp4 => crate::store_video::check_video(kind, input)?,
            }
            out.flush().map_err(|e| e.to_string())
        })()
    };
    match result {
        Ok(()) => true,
        Err(reason) => {
            let message: String = reason.chars().filter(|c| !c.is_control()).take(250).collect();
            file.rewind()
                .and_then(|()| file.set_len(0))
                .and_then(|()| file.write_all(MAGIC))
                .and_then(|()| file.write_all(&[1]))
                .and_then(|()| write_blob(&mut file, message.as_bytes()))
                .is_ok()
        }
    }
}

#[cfg(target_os = "android")]
#[allow(unsafe_code)]
#[uniffi::export]
pub fn decode_store_descriptors(kind: StoreDecodeKind, input: i32, output: i32) -> bool {
    use std::os::fd::FromRawFd as _;
    if input < 0 || output < 0 || input == output {
        return false;
    }
    // The service detaches two distinct ParcelFileDescriptors and transfers ownership here.
    let (input, output) = unsafe { (std::fs::File::from_raw_fd(input), std::fs::File::from_raw_fd(output)) };
    run(kind, input, output)
}

struct Bounded<'a> {
    remaining: u64,
    file: &'a mut std::fs::File,
}

impl Write for Bounded<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(io::Error::other("The decoded Store file exceeds its size limit."));
        }
        let written = self.file.write(bytes)?;
        self.remaining -= written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

pub(crate) fn read_u32(input: &mut impl Read) -> io::Result<u32> {
    let mut bytes = [0; 4];
    input.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

pub(crate) fn write_u32(output: &mut impl Write, number: u32) -> io::Result<()> {
    output.write_all(&number.to_le_bytes())
}

pub(crate) fn read_blob(input: &mut impl Read, max: usize) -> io::Result<Vec<u8>> {
    let length = read_u32(input)? as usize;
    if length > max {
        return Err(io::Error::other(BAD_RESPONSE));
    }
    let mut data = Vec::with_capacity(length);
    input.take(length as u64).read_to_end(&mut data)?;
    if data.len() != length {
        return Err(io::Error::other(BAD_RESPONSE));
    }
    Ok(data)
}

pub(crate) fn write_blob(output: &mut impl Write, data: &[u8]) -> io::Result<()> {
    let len = u32::try_from(data.len()).map_err(io::Error::other)?;
    write_u32(output, len)?;
    output.write_all(data)
}

pub(crate) fn end(input: &mut impl Read) -> io::Result<()> {
    if input.read(&mut [0])? == 0 { Ok(()) } else { Err(io::Error::other(BAD_RESPONSE)) }
}
