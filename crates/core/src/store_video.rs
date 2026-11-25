//! Validate the boundaries and essential records of a self-contained Store demo in the worker.
//! The browser or Android media service is responsible for decoding its audio and video.
use std::io::Read as _;

use crate::{StoreDecodeKind, store_media::VIDEO_BYTES};

pub(crate) fn check_video(kind: StoreDecodeKind, file: std::fs::File) -> Result<(), String> {
    let mut bytes = Vec::new();
    file.take(VIDEO_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    let valid = bytes.len() <= VIDEO_BYTES
        && match kind {
            StoreDecodeKind::Mp4 => valid_mp4(&bytes),
            StoreDecodeKind::Webm => valid_webm(&bytes),
            _ => false,
        };
    valid.then_some(()).ok_or_else(|| "The file isn't a readable, self-contained WebM or MP4 demo.".to_owned())
}

// Read one ISO BMFF box, accepting 32-bit, extended or end-of-file size fields.
fn mp4_box<'a>(bytes: &mut &'a [u8]) -> Option<([u8; 4], &'a [u8])> {
    let size = u32::from_be_bytes(bytes.get(..4)?.try_into().ok()?);
    let name = bytes.get(4..8)?.try_into().ok()?;
    let (header, size) = match size {
        0 => (8, bytes.len()),
        1 => (16, usize::try_from(u64::from_be_bytes(bytes.get(8..16)?.try_into().ok()?)).ok()?),
        n => (8, n as usize),
    };
    let contents = bytes.get(header..size)?;
    *bytes = bytes.get(size..)?;
    Some((name, contents))
}

fn valid_mp4(mut bytes: &[u8]) -> bool {
    let Some((kind, brand)) = mp4_box(&mut bytes) else { return false };
    if &kind != b"ftyp" || brand.len() < 8 || !brand.len().is_multiple_of(4) || brand.starts_with(b"qt  ") {
        return false;
    }
    let (mut movie, mut media) = (false, false);
    let mut budget = 32_768;
    while !bytes.is_empty() {
        let Some((kind, data)) = mp4_box(&mut bytes) else { return false };
        if budget == 0 {
            return false;
        }
        budget -= 1;
        match &kind {
            b"moov" if !movie => movie = local_mp4_movie(data, &mut budget, 0),
            b"mdat" => media |= !data.is_empty(),
            b"moov" | b"ftyp" | b"uuid" => return false,
            _ => {}
        }
        if &kind == b"moov" && !movie {
            return false;
        }
    }
    movie && media
}

fn local_mp4_movie(mut bytes: &[u8], budget: &mut usize, depth: usize) -> bool {
    if depth > 4 {
        return false;
    }
    let mut tracks = 0usize;
    let mut header = false;
    while !bytes.is_empty() {
        let Some((kind, data)) = mp4_box(&mut bytes) else { return false };
        if *budget == 0 {
            return false;
        }
        *budget -= 1;
        match &kind {
            b"mvhd" if depth == 0 => header = !data.is_empty(),
            b"trak" if depth == 0 => {
                if data.is_empty() || !local_mp4_movie(data, budget, depth + 1) {
                    return false;
                }
                tracks += 1;
            }
            b"mdia" | b"minf" | b"dinf" if !local_mp4_movie(data, budget, depth + 1) => return false,
            b"dref" if !local_mp4_references(data, budget) => return false,
            b"rmra" | b"rmda" | b"uuid" => return false,
            _ => {}
        }
    }
    depth > 0 || (header && tracks > 0)
}

fn local_mp4_references(bytes: &[u8], budget: &mut usize) -> bool {
    let Some((header, mut references)) = bytes.split_at_checked(8) else { return false };
    let [0, 0, 0, 0, a, b, c, d] = header else { return false };
    let count = u32::from_be_bytes([*a, *b, *c, *d]) as usize;
    if count == 0 || count > *budget {
        return false;
    }
    *budget -= count;
    (0..count).all(|_| {
        // A version-zero url entry with its self-contained flag set and no external location.
        mp4_box(&mut references).is_some_and(|(kind, body)| &kind == b"url " && body == b"\0\0\0\x01")
    }) && references.is_empty()
}

fn ebml_int(bytes: &mut &[u8], max_len: usize, with_marker: bool) -> Option<(u64, bool)> {
    let first = *bytes.first()?;
    let length = first.leading_zeros() as usize + 1;
    if length > max_len {
        return None;
    }
    let (field, tail) = bytes.split_at_checked(length)?;
    *bytes = tail;
    let mut number = u64::from(first);
    if !with_marker {
        number &= (1 << (8 - length)) - 1;
    }
    for byte in field.iter().skip(1) {
        number = (number << 8) | u64::from(*byte);
    }
    Some((number, !with_marker && number == (1 << (7 * length)) - 1))
}

fn ebml_element<'a>(bytes: &mut &'a [u8], allow_unknown: bool) -> Option<(u64, &'a [u8])> {
    let (id, _) = ebml_int(bytes, 4, true)?;
    let (size, unknown) = ebml_int(bytes, 8, false)?;
    let size = if unknown && allow_unknown && matches!(id, 0x1853_8067 | 0x1F43_B675) {
        bytes.len()
    } else if unknown {
        return None;
    } else {
        usize::try_from(size).ok()?
    };
    let (data, remaining) = bytes.split_at_checked(size)?;
    *bytes = remaining;
    Some((id, data))
}

fn valid_webm(mut bytes: &[u8]) -> bool {
    let Some((0x1A45_DFA3, mut header)) = ebml_element(&mut bytes, false) else { return false };
    if header.len() > 4096 {
        return false;
    }
    let mut webm = false;
    while !header.is_empty() {
        let Some((id, data)) = ebml_element(&mut header, false) else { return false };
        if id == 0x4282 {
            if webm || data != b"webm" {
                return false;
            }
            webm = true;
        }
    }
    let Some((0x1853_8067, mut segment)) = ebml_element(&mut bytes, true) else { return false };
    if !webm || !bytes.is_empty() {
        return false;
    }
    let (mut info, mut tracks, mut cluster) = (false, false, false);
    let mut count = 0;
    while !segment.is_empty() {
        let Some((id, data)) = ebml_element(&mut segment, true) else { return false };
        count += 1;
        if count > 32_768 {
            return false;
        }
        match id {
            0x1549_A966 => info |= !data.is_empty(),
            0x1654_AE6B => tracks |= !data.is_empty(),
            0x1F43_B675 => cluster |= !data.is_empty(),
            // WebM has no attachments or linked chapters.
            0x1941_A469 | 0x1043_A770 => return false,
            _ => {}
        }
    }
    info && tracks && cluster
}
