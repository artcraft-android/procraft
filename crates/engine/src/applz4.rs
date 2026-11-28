//! LZ4 in a block framing: blocks of up to 64 KiB, each `bv41`
//! (compressed: raw size, compressed size, LZ4 data that may refer back into earlier blocks) or
//! `bv4-` (stored: raw size, bytes), ended by `bv4$`. The blocks written here never refer back,
//! so long data's blocks are packed and unpacked side by side; a block written elsewhere that
//! refers back is decoded after the blocks before it.

/// Raw bytes per block written.
const BLOCK: usize = 64 * 1024;

/// How long data is at least for its blocks to be packed or unpacked side by side (less is done
/// sooner on one thread than shared out).
const SIDE_BY_SIDE: usize = 1 << 20;

/// How much of long data is packed as one share of the work (whole blocks).
const SHARE: usize = 8 * BLOCK;

/// A block of a stream: its LZ4 data and how long it comes out, or its bytes as stored.
#[derive(Clone, Copy)]
enum Block<'a> {
    Packed(&'a [u8], usize),
    Stored(&'a [u8]),
}

impl Block<'_> {
    /// How long it comes out.
    fn raw(&self) -> usize {
        match self {
            Block::Packed(_, raw) => *raw,
            Block::Stored(bytes) => bytes.len(),
        }
    }

    /// Decodes it into the end of `out`, from `at` (as long as it comes out), its matches reaching
    /// back as far as `out` goes; none if it is damaged or comes out another length.
    fn put(&self, out: &mut [u8], at: usize) -> Option<()> {
        match self {
            Block::Packed(src, _) => unpack_block(src, out, at),
            Block::Stored(bytes) => {
                let to = out.get_mut(at..)?;
                if to.len() != bytes.len() {
                    return None;
                }
                to.copy_from_slice(bytes);
                Some(())
            }
        }
    }
}

/// Decodes a framed stream; `None` if it is damaged or would come out longer than `limit`.
pub fn decode(data: &[u8], limit: usize) -> Option<Vec<u8>> {
    let (blocks, total) = blocks(data, limit)?;
    if total >= SIDE_BY_SIDE && blocks.len() > 1 && crate::par::cores() > 1 && !crate::par::in_worker() {
        let mut out = vec![0u8; total];
        let alone = side_by_side(&blocks, &mut out)?;
        // the rest in order, each reaching back into those before it
        let mut at = 0;
        for (block, done) in blocks.iter().zip(alone) {
            let end = at + block.raw();
            if !done {
                block.put(out.get_mut(..end)?, at)?;
            }
            at = end;
        }
        return Some(out);
    }
    // one after another, each block's room made as it comes (written while it is in the cache)
    let mut out = Vec::with_capacity(total);
    for block in &blocks {
        match block {
            Block::Stored(bytes) => out.extend_from_slice(bytes),
            Block::Packed(src, raw) => {
                let at = out.len();
                out.resize(at + raw, 0);
                unpack_block(src, &mut out, at)?;
            }
        }
    }
    Some(out)
}

/// A stream's blocks and how long they come out together; none if its framing is damaged or it
/// would come out longer than `limit`.
fn blocks(data: &[u8], limit: usize) -> Option<(Vec<Block<'_>>, usize)> {
    let mut blocks = Vec::new();
    let (mut at, mut total) = (0usize, 0usize);
    loop {
        let magic = data.get(at..at.checked_add(4)?)?;
        let raw = match magic {
            b"bv4$" => return Some((blocks, total)),
            b"bv41" | b"bv4-" => le(data, at + 4)?,
            _ => return None,
        };
        total = total.checked_add(raw).filter(|&t| t <= limit)?;
        if magic == b"bv41" {
            let comp = le(data, at + 8)?;
            let start = at + 12;
            blocks.push(Block::Packed(data.get(start..start.checked_add(comp)?)?, raw));
            at = start + comp;
        } else {
            let start = at + 8;
            blocks.push(Block::Stored(data.get(start..start.checked_add(raw)?)?));
            at = start + raw;
        }
    }
}

/// Decodes each of `blocks` on its own into its part of `out` (theirs, one after another), side
/// by side: whether each one was (one that refers back into the blocks before it isn't).
fn side_by_side(blocks: &[Block], out: &mut [u8]) -> Option<Vec<bool>> {
    let mut jobs: Vec<(Block, &mut [u8], bool)> = Vec::with_capacity(blocks.len());
    let mut rest = out;
    for block in blocks {
        let (part, after) = rest.split_at_mut_checked(block.raw())?;
        jobs.push((*block, part, false));
        rest = after;
    }
    crate::par::each_part(&mut jobs, 1, |part| {
        for (block, to, done) in part {
            *done = block.put(to, 0).is_some();
        }
    });
    Some(jobs.into_iter().map(|(_, _, done)| done).collect())
}

fn le(b: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?)).ok()
}

/// Decodes one LZ4 block into the end of `out`, from `at`, its matches reaching back as far as
/// `out` goes; none if it is damaged or doesn't come out exactly that long. As [`decode_block`]
/// reads one: it ends where its data does, or with a run of literals that fills `out`.
fn unpack_block(src: &[u8], out: &mut [u8], mut at: usize) -> Option<()> {
    let end = out.len();
    let mut i = 0;
    while i < src.len() {
        let token = *src.get(i)?;
        i += 1;
        // most sequences: both lengths in the token, with room to spare (16 literals and an
        // offset to read, 16 literals and a short match's 32 bytes to write), copied in fixed sizes
        if token >> 4 != 15 && token & 15 != 15 && i + 18 <= src.len() && at + 48 <= end {
            let from = src.get(i..).and_then(<[u8]>::first_chunk::<16>)?;
            *out.get_mut(at..).and_then(<[u8]>::first_chunk_mut::<16>)? = *from;
            i += usize::from(token >> 4);
            at += usize::from(token >> 4);
            let offset = usize::from(u16::from_le_bytes(*src.get(i..).and_then(<[u8]>::first_chunk::<2>)?));
            i += 2;
            let length = usize::from(token & 15) + 4;
            if offset == 0 || offset > at {
                return None;
            }
            short_copy_back(out, at, offset, length)?;
            at += length;
            continue;
        }
        let mut literals = usize::from(token >> 4);
        if literals == 15 {
            literals = literals.checked_add(run(src, &mut i)?)?;
        }
        out.get_mut(at..at.checked_add(literals)?)?.copy_from_slice(src.get(i..i.checked_add(literals)?)?);
        i += literals;
        at += literals;
        if i >= src.len() || at >= end {
            break;
        }
        let offset = usize::from(u16::from_le_bytes(*src.get(i..).and_then(<[u8]>::first_chunk::<2>)?));
        i += 2;
        let mut length = usize::from(token & 15);
        if length == 15 {
            length = length.checked_add(run(src, &mut i)?)?;
        }
        length = length.checked_add(4)?;
        if offset == 0 || offset > at || at.checked_add(length)? > end {
            return None;
        }
        copy_back(out, at, offset, length)?;
        at += length;
    }
    (at == end).then_some(())
}

/// What a length's run of extra bytes adds to it (each 255 but the last), from `i` on.
fn run(src: &[u8], i: &mut usize) -> Option<usize> {
    let mut n = 0usize;
    loop {
        let b = *src.get(*i)?;
        *i += 1;
        n = n.saturating_add(usize::from(b));
        if b != 255 {
            return Some(n);
        }
    }
}

/// Writes `length` bytes of `out` at `at` copied from `offset` before it, as if one at a time: a
/// match nearer than its length repeats what it begins with. A short one with room past it as
/// [`short_copy_back`]; a long one nearer than its length as whole runs: its first period, then
/// from the match's own start runs doubling in length (whole periods each, so they line up),
/// rather than a byte at a time (a layer's clear stretches are matches thousands of bytes long).
fn copy_back(out: &mut [u8], at: usize, offset: usize, length: usize) -> Option<()> {
    let from = at.checked_sub(offset)?;
    if length <= 32 && out.len().saturating_sub(at) >= 40 {
        return short_copy_back(out, at, offset, length);
    }
    if offset >= length {
        return within(out, from, at, length);
    }
    if offset == 1 {
        let b = *out.get(from)?;
        out.get_mut(at..at.checked_add(length)?)?.fill(b);
        return Some(());
    }
    within(out, from, at, offset)?;
    let (mut written, mut left) = (offset, length - offset);
    while left > 0 {
        let n = left.min(written);
        within(out, at, at + written, n)?;
        written += n;
        left -= n;
    }
    Some(())
}

/// [`copy_back`] of a match of 32 bytes at most with room for 40 from `at` (32 from it, for a
/// match of 18 at most): in copies of a fixed size, what they write past its end overwritten
/// next.
#[inline(always)]
fn short_copy_back(out: &mut [u8], at: usize, offset: usize, length: usize) -> Option<()> {
    let from = at.checked_sub(offset)?;
    if offset >= length {
        // apart: as one copy
        within(out, from, at, 32)
    } else if offset >= 8 {
        // 8 at a time, each copied from bytes wholly before it
        let mut k = 0;
        while k < length {
            within(out, from + k, at + k, 8)?;
            k += 8;
        }
        Some(())
    } else {
        // nearer than 8: its first period repeated to fill 8 bytes, written a whole number of
        // periods apart
        let mut period = *out.get(from..).and_then(<[u8]>::first_chunk::<8>)?;
        for k in offset..8 {
            *period.get_mut(k)? = *period.get(k - offset)?;
        }
        let step = 8 - 8 % offset.max(1);
        let mut k = 0;
        while k < length {
            *out.get_mut(at + k..).and_then(<[u8]>::first_chunk_mut::<8>)? = period;
            k += step;
        }
        Some(())
    }
}

/// Copies `n` bytes of `out` from `from` to `to` (as if through a buffer); none if either is past
/// its end.
#[inline(always)]
fn within(out: &mut [u8], from: usize, to: usize, n: usize) -> Option<()> {
    if from.checked_add(n)? > out.len() || to.checked_add(n)? > out.len() {
        return None;
    }
    out.copy_within(from..from + n, to);
    Some(())
}

/// One bare LZ4 block (no framing), whatever its length up to `limit`; `None` if it is damaged or
/// longer.
pub fn raw_block(src: &[u8], limit: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    decode_block(src, &mut out, limit)?;
    Some(out)
}

/// Decodes an LZ4 block onto `out`, to the block's end or until `out` holds `limit` bytes.
fn decode_block(src: &[u8], out: &mut Vec<u8>, limit: usize) -> Option<()> {
    let goal = limit;
    let mut i = 0;
    while i < src.len() {
        let token = *src.get(i)?;
        i += 1;
        let mut literals = usize::from(token >> 4);
        if literals == 15 {
            loop {
                let b = *src.get(i)?;
                i += 1;
                literals += usize::from(b);
                if b != 255 {
                    break;
                }
            }
        }
        out.extend_from_slice(src.get(i..i.checked_add(literals)?)?);
        i += literals;
        if i >= src.len() || out.len() >= goal {
            break;
        }
        let offset = usize::from(u16::from_le_bytes(src.get(i..i + 2)?.try_into().ok()?));
        i += 2;
        let mut length = usize::from(token & 15);
        if length == 15 {
            loop {
                let b = *src.get(i)?;
                i += 1;
                length += usize::from(b);
                if b != 255 {
                    break;
                }
            }
        }
        length += 4;
        let start = out.len().checked_sub(offset)?;
        if offset == 0 || out.len().checked_add(length)? > goal {
            return None;
        }
        copy_match(out, start, length);
    }
    Some(())
}

/// Appends `length` bytes to `out` copied from `start` on, as if one at a time: a match nearer than
/// its length repeats what it begins with. Done as whole runs: its first period, then from the
/// match's own start runs doubling in length (whole periods each, so they line up), rather than a
/// byte at a time (a layer's clear stretches are matches thousands of bytes long).
fn copy_match(out: &mut Vec<u8>, start: usize, length: usize) {
    out.reserve(length);
    let offset = out.len() - start;
    let first = length.min(offset);
    out.extend_from_within(start..start + first);
    let at = out.len() - first;
    let (mut written, mut left) = (first, length - first);
    while left > 0 {
        let n = left.min(written);
        out.extend_from_within(at..at + n);
        written += n;
        left -= n;
    }
}

/// Encodes `data` in the framing, each block compressed (or stored, when that is smaller): long
/// data a few blocks a share, side by side, each block as it would be alone.
pub fn encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 4 + 16);
    let mut table = Table::default();
    if data.len() >= SIDE_BY_SIDE && crate::par::cores() > 1 && !crate::par::in_worker() {
        let shares: Vec<&[u8]> = data.chunks(SHARE).collect();
        let packed = crate::par::side_by_side(&shares, |share| {
            let (mut packed, mut table) = (Vec::with_capacity(share.len() / 4), Table::default());
            for block in share.chunks(BLOCK) {
                put_block(&mut packed, block, &mut table);
            }
            packed
        });
        for (share, packed) in shares.iter().zip(packed) {
            match packed {
                Some(p) => out.extend_from_slice(&p),
                // (one whose thread stopped packed here)
                None => {
                    for block in share.chunks(BLOCK) {
                        put_block(&mut out, block, &mut table);
                    }
                }
            }
        }
    } else {
        for block in data.chunks(BLOCK) {
            put_block(&mut out, block, &mut table);
        }
    }
    out.extend_from_slice(b"bv4$");
    out
}

/// One block in the framing onto `out`: compressed, or stored when that is smaller.
fn put_block(out: &mut Vec<u8>, block: &[u8], table: &mut Table) {
    let start = out.len();
    out.extend_from_slice(b"bv41");
    out.extend_from_slice(&(block.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    compress_block(block, table, out);
    let packed = out.len() - start - 12;
    if packed < block.len() {
        if let Some(size) = out.get_mut(start + 8..start + 12) {
            size.copy_from_slice(&(packed as u32).to_le_bytes());
        }
    } else {
        out.truncate(start);
        out.extend_from_slice(b"bv4-");
        out.extend_from_slice(&(block.len() as u32).to_le_bytes());
        out.extend_from_slice(block);
    }
}

/// [`encode`] of data given a piece at a time (rows read from a buffer, say), so that it is never
/// all in memory at once: a block's worth is held at most. What [`Packer::finish`] gives is what
/// `encode` gives of the pieces joined.
#[derive(Default)]
pub struct Packer {
    out: Vec<u8>,
    pending: Vec<u8>,
    table: Table,
}

impl Packer {
    /// The next piece.
    pub fn push(&mut self, mut piece: &[u8]) {
        while !piece.is_empty() {
            let room = BLOCK - self.pending.len();
            let (now, rest) = piece.split_at(room.min(piece.len()));
            self.pending.extend_from_slice(now);
            piece = rest;
            if self.pending.len() == BLOCK {
                put_block(&mut self.out, &self.pending, &mut self.table);
                self.pending.clear();
            }
        }
    }

    /// The stream, ended, and no bigger than it needs to be.
    pub fn finish(mut self) -> Vec<u8> {
        if !self.pending.is_empty() {
            put_block(&mut self.out, &self.pending, &mut self.table);
        }
        self.out.extend_from_slice(b"bv4$");
        self.out.shrink_to_fit();
        self.out
    }
}

const HASH_BITS: u32 = 14;

/// Where each hash of 4 bytes was last seen in a block, kept from block to block (made once, not
/// for each).
struct Table(Box<[u32; 1 << HASH_BITS]>);

impl Default for Table {
    fn default() -> Table {
        let made = vec![0; 1 << HASH_BITS].into_boxed_slice().try_into();
        Table(made.unwrap_or_else(|_| Box::new([0; 1 << HASH_BITS])))
    }
}

/// One LZ4 block onto `out`, matches found greedily through a hash of 4-byte sequences (`table`,
/// cleared first: each block is packed as if alone).
fn compress_block(src: &[u8], table: &mut Table, out: &mut Vec<u8>) {
    // the format's rules: the last 5 bytes are literals, and no match starts in the last 12
    const LAST_LITERALS: usize = 5;
    const MATCH_LIMIT: usize = 12;
    out.reserve(src.len() / 2 + 16);
    let table = &mut *table.0;
    table.fill(0);
    let read4 = |at: usize| src.get(at..).and_then(<[u8]>::first_chunk::<4>).map(|b| u32::from_le_bytes(*b));
    let hash = |v: u32| (v.wrapping_mul(2_654_435_761) >> (32 - HASH_BITS)) as usize;
    let mut anchor = 0;
    let mut i = 0;
    let end = src.len().saturating_sub(MATCH_LIMIT);
    while i < end {
        let Some(v) = read4(i) else { break };
        let Some(slot) = table.get_mut(hash(v)) else { break };
        let candidate = *slot as usize;
        *slot = i as u32;
        if candidate < i && i - candidate <= 0xFFFF && read4(candidate) == Some(v) {
            // extend the match, stopping short of the block's literal tail
            let max = src.len() - LAST_LITERALS;
            let mut len = 4;
            // eight bytes at a time while they agree (a clear stretch matches for thousands), the
            // first that differs found in the eight, else a byte at a time near the end
            let read8 = |at: usize| src.get(at..).and_then(<[u8]>::first_chunk::<8>).map(|b| u64::from_le_bytes(*b));
            let mut differs = false;
            while i + len + 8 <= max {
                let (Some(a), Some(b)) = (read8(candidate + len), read8(i + len)) else { break };
                let d = a ^ b;
                if d != 0 {
                    len += (d.trailing_zeros() / 8) as usize;
                    differs = true;
                    break;
                }
                len += 8;
            }
            while !differs && i + len < max && src.get(candidate + len) == src.get(i + len) {
                len += 1;
            }
            emit(out, src.get(anchor..i).unwrap_or(&[]), Some(((i - candidate) as u16, len)));
            i += len;
            anchor = i;
        } else {
            i += 1;
        }
    }
    emit(out, src.get(anchor..).unwrap_or(&[]), None);
}

/// A sequence: `literals`, then a match (offset, length) if any.
fn emit(out: &mut Vec<u8>, literals: &[u8], m: Option<(u16, usize)>) {
    let lit = literals.len();
    let ml = m.map_or(0, |(_, l)| l - 4);
    let token = ((lit.min(15) as u8) << 4) | ml.min(15) as u8;
    out.push(token);
    if lit >= 15 {
        let mut rest = lit - 15;
        while rest >= 255 {
            out.push(255);
            rest -= 255;
        }
        out.push(rest as u8);
    }
    out.extend_from_slice(literals);
    if let Some((offset, _)) = m {
        out.extend_from_slice(&offset.to_le_bytes());
        if ml >= 15 {
            let mut rest = ml - 15;
            while rest >= 255 {
                out.push(255);
                rest -= 255;
            }
            out.push(rest as u8);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stream decoded as it was before: a block at a time onto a growing buffer, through
    /// [`decode_block`] (which bare blocks still go through).
    fn decode_before(data: &[u8], limit: usize) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        let mut at = 0;
        loop {
            match data.get(at..at + 4)? {
                b"bv4$" => return Some(out),
                b"bv41" => {
                    let raw = le(data, at + 4)?;
                    let comp = le(data, at + 8)?;
                    if out.len().checked_add(raw)? > limit {
                        return None;
                    }
                    let block = data.get(at + 12..at.checked_add(12)?.checked_add(comp)?)?;
                    let goal = out.len() + raw;
                    out.reserve(raw);
                    decode_block(block, &mut out, goal)?;
                    if out.len() != goal {
                        return None;
                    }
                    at += 12 + comp;
                }
                b"bv4-" => {
                    let raw = le(data, at + 4)?;
                    if out.len().checked_add(raw)? > limit {
                        return None;
                    }
                    out.extend_from_slice(data.get(at + 8..at.checked_add(8)?.checked_add(raw)?)?);
                    at += 8 + raw;
                }
                _ => return None,
            }
        }
    }

    /// Data encoded as it was before: each block on its own, with a table of its own.
    fn encode_before(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for block in data.chunks(BLOCK) {
            let packed = compress_block_before(block);
            if packed.len() < block.len() {
                out.extend_from_slice(b"bv41");
                out.extend_from_slice(&(block.len() as u32).to_le_bytes());
                out.extend_from_slice(&(packed.len() as u32).to_le_bytes());
                out.extend_from_slice(&packed);
            } else {
                out.extend_from_slice(b"bv4-");
                out.extend_from_slice(&(block.len() as u32).to_le_bytes());
                out.extend_from_slice(block);
            }
        }
        out.extend_from_slice(b"bv4$");
        out
    }

    /// [`compress_block`] as it was: a table of its own.
    fn compress_block_before(src: &[u8]) -> Vec<u8> {
        const LAST_LITERALS: usize = 5;
        const MATCH_LIMIT: usize = 12;
        let mut out = Vec::with_capacity(src.len() / 2 + 16);
        let mut table = vec![0usize; 1 << HASH_BITS];
        let read4 = |at: usize| src.get(at..at + 4).and_then(|b| <[u8; 4]>::try_from(b).ok()).map(u32::from_le_bytes);
        let hash = |v: u32| (v.wrapping_mul(2_654_435_761) >> (32 - HASH_BITS)) as usize;
        let mut anchor = 0;
        let mut i = 0;
        let end = src.len().saturating_sub(MATCH_LIMIT);
        while i < end {
            let Some(v) = read4(i) else { break };
            let h = hash(v);
            let candidate = table.get(h).copied().unwrap_or(0);
            if let Some(slot) = table.get_mut(h) {
                *slot = i;
            }
            if candidate < i && i - candidate <= 0xFFFF && read4(candidate) == Some(v) {
                let max = src.len() - LAST_LITERALS;
                let mut len = 4;
                let read8 =
                    |at: usize| src.get(at..at + 8).and_then(|b| <[u8; 8]>::try_from(b).ok()).map(u64::from_le_bytes);
                let mut differs = false;
                while i + len + 8 <= max {
                    let (Some(a), Some(b)) = (read8(candidate + len), read8(i + len)) else { break };
                    let d = a ^ b;
                    if d != 0 {
                        len += (d.trailing_zeros() / 8) as usize;
                        differs = true;
                        break;
                    }
                    len += 8;
                }
                while !differs && i + len < max && src.get(candidate + len) == src.get(i + len) {
                    len += 1;
                }
                emit(&mut out, src.get(anchor..i).unwrap_or(&[]), Some(((i - candidate) as u16, len)));
                i += len;
                anchor = i;
            } else {
                i += 1;
            }
        }
        emit(&mut out, src.get(anchor..).unwrap_or(&[]), None);
        out
    }

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn f(&mut self) -> f32 {
            (self.next() >> 40) as f32 / (1u64 << 24) as f32
        }
    }

    const SIDE: usize = 256;

    /// Premultiplied paint `rgb` at coverage `a` over the pixel at (x, y) of a tile.
    fn put(t: &mut [u8], x: usize, y: usize, rgb: [f32; 3], a: f32) {
        let i = (y * SIDE + x) * 4;
        let a = a.clamp(0.0, 1.0);
        for c in 0..3 {
            let d = f32::from(t[i + c]) / 255.0;
            t[i + c] = ((rgb[c] * a + d * (1.0 - a)) * 255.0 + 0.5) as u8;
        }
        let da = f32::from(t[i + 3]) / 255.0;
        t[i + 3] = ((a + da * (1.0 - a)) * 255.0 + 0.5) as u8;
    }

    /// Tiles of a layer, `SIDE` square, as layers hold them.
    fn empty(_: u64) -> Vec<u8> {
        vec![0; SIDE * SIDE * 4]
    }

    fn flat(_: u64) -> Vec<u8> {
        [230u8, 214, 190, 255].repeat(SIDE * SIDE)
    }

    /// Ink lines: a few anti-aliased rings 3 to 7 pixels wide over clear.
    fn line_art(seed: u64) -> Vec<u8> {
        let mut r = Rng(seed | 1);
        let mut t = empty(0);
        for _ in 0..4 {
            let (cx, cy, rad, w) = (r.f() * 256.0, r.f() * 256.0, 40.0 + r.f() * 150.0, 1.5 + r.f() * 2.0);
            for y in 0..SIDE {
                for x in 0..SIDE {
                    let d = ((x as f32 - cx).hypot(y as f32 - cy) - rad).abs();
                    let a = (w + 0.5 - d).clamp(0.0, 1.0);
                    if a > 0.0 {
                        put(&mut t, x, y, [0.08, 0.06, 0.1], a);
                    }
                }
            }
        }
        t
    }

    /// A soft painting: an opaque gradient under a dozen airbrushed blobs.
    fn soft(seed: u64) -> Vec<u8> {
        let mut r = Rng(seed | 1);
        let mut t = empty(0);
        for y in 0..SIDE {
            for x in 0..SIDE {
                let (u, v) = (x as f32 / 255.0, y as f32 / 255.0);
                put(&mut t, x, y, [0.55 + 0.3 * u, 0.4 + 0.2 * v, 0.35 + 0.1 * (u - v)], 1.0);
            }
        }
        for _ in 0..12 {
            let (cx, cy, rad) = (r.f() * 256.0, r.f() * 256.0, 20.0 + r.f() * 90.0);
            let (col, op) = ([r.f(), r.f(), r.f()], 0.2 + r.f() * 0.4);
            for y in 0..SIDE {
                for x in 0..SIDE {
                    let d = (x as f32 - cx).hypot(y as f32 - cy) / rad;
                    if d < 1.0 {
                        put(&mut t, x, y, col, op * (1.0 - d * d).powi(2));
                    }
                }
            }
        }
        t
    }

    /// A grainy brush's marks: partial coverage, each pixel's shaken by a paper grain.
    fn grainy(seed: u64) -> Vec<u8> {
        let mut r = Rng(seed | 1);
        let mut t = empty(0);
        for y in 0..SIDE {
            for x in 0..SIDE {
                let band = ((y as f32 / 40.0).sin() * 0.5 + 0.5) * ((x as f32 / 70.0).cos() * 0.5 + 0.5);
                let a = (band * 1.4 - 0.2) * (0.55 + 0.45 * r.f());
                if a > 0.01 {
                    put(&mut t, x, y, [0.2, 0.35, 0.6], a);
                }
            }
        }
        t
    }

    fn noise(seed: u64) -> Vec<u8> {
        let mut r = Rng(seed | 1);
        (0..SIDE * SIDE * 4).map(|_| r.next() as u8).collect()
    }

    type Kind = (&'static str, fn(u64) -> Vec<u8>);

    const KINDS: [Kind; 6] = [
        ("empty", empty),
        ("flat fill", flat),
        ("line art", line_art),
        ("soft paint", soft),
        ("grainy", grainy),
        ("noise", noise),
    ];

    /// A layer's rows, `tiles` tiles of `kind` square (each its own), as a parked layer keeps them.
    fn layer(kind: fn(u64) -> Vec<u8>, tiles: usize) -> Vec<u8> {
        let side = tiles * SIDE;
        let mut out = vec![0u8; side * side * 4];
        for ty in 0..tiles {
            for tx in 0..tiles {
                let t = kind(1000 + (ty * tiles + tx) as u64);
                for r in 0..SIDE {
                    let at = ((ty * SIDE + r) * side + tx * SIDE) * 4;
                    out[at..at + SIDE * 4].copy_from_slice(&t[r * SIDE * 4..(r + 1) * SIDE * 4]);
                }
            }
        }
        out
    }

    #[test]
    fn layer_like_data_round_trips_whatever_its_runs() {
        // clear stretches, painted runs and noise, at lengths around the eight-byte steps
        let mut data = Vec::new();
        let mut seed = 7u32;
        for k in 0..400usize {
            match k % 3 {
                0 => data.extend(std::iter::repeat_n(0u8, k * 13 % 997 + 1)),
                1 => data.extend([30u8, 60, 200, 255].repeat(k % 37 + 1)),
                _ => data.extend((0..k % 23 + 1).map(|_| {
                    seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                    (seed >> 16) as u8
                })),
            }
        }
        let packed = encode(&data);
        assert!(packed.len() < data.len() / 2);
        assert_eq!(decode(&packed, data.len()).as_deref(), Some(data.as_slice()));
    }

    #[test]
    fn a_match_is_copied_as_one_byte_at_a_time_would() {
        // the old way, a byte at a time, against runs: every offset and length up to 70
        for offset in 1..20 {
            for length in 1..70 {
                let mut seed: Vec<u8> = (0..30u8).map(|b| b.wrapping_mul(37).wrapping_add(11)).collect();
                let mut byte_wise = seed.clone();
                let start = seed.len() - offset;
                for k in 0..length {
                    let b = byte_wise[start + k];
                    byte_wise.push(b);
                }
                copy_match(&mut seed, start, length);
                assert_eq!(seed, byte_wise, "offset {offset}, length {length}");
            }
        }
    }

    /// A match copied back into a buffer is what a byte at a time makes of it, whatever its
    /// offset and length and however much room there is past it (the fixed-size copies write
    /// past its end where there is room, never past the buffer's; what they write there is
    /// overwritten next).
    #[test]
    fn a_match_copied_back_is_as_one_byte_at_a_time_would_make_it() {
        let lead: Vec<u8> = (0..120u8).map(|b| b.wrapping_mul(37).wrapping_add(11)).collect();
        let at = lead.len();
        for offset in 1..=lead.len() {
            for length in (4..=48).chain([63, 64, 65, 100, 255, 1000]) {
                let mut byte_wise = lead.clone();
                for k in 0..length {
                    let b = byte_wise[at - offset + k];
                    byte_wise.push(b);
                }
                for room in [0, 1, 7, 8, 15, 16, 31, 32, 33, 39, 40, 41, 64] {
                    let mut out = lead.clone();
                    out.resize(at + length + room, 0xEE);
                    assert_eq!(copy_back(&mut out, at, offset, length), Some(()));
                    assert_eq!(out[..at + length], byte_wise[..], "offset {offset}, length {length}, room {room}");
                    if length <= 32 && length + room >= 40 {
                        let mut short = lead.clone();
                        short.resize(at + length + room, 0xEE);
                        assert_eq!(short_copy_back(&mut short, at, offset, length), Some(()));
                        assert_eq!(short[..at + length], byte_wise[..], "short: offset {offset}, length {length}");
                    }
                }
            }
        }
        // no room for it, or from before the start: refused
        let mut out = vec![1u8; 10];
        assert_eq!(copy_back(&mut out, 8, 4, 5), None);
        assert_eq!(copy_back(&mut out, 3, 4, 2), None);
    }

    #[test]
    fn data_packed_a_piece_at_a_time_is_packed_as_if_whole() {
        let data: Vec<u8> = (0..300_000u32).map(|i| if (i / 5000) % 3 == 0 { 0 } else { (i % 241) as u8 }).collect();
        for piece in [1, 7, 4096, BLOCK - 1, BLOCK, BLOCK + 3, 100_000, 400_000] {
            let mut packer = Packer::default();
            for chunk in data.chunks(piece) {
                packer.push(chunk);
            }
            assert!(packer.finish() == encode(&data), "pieces of {piece}");
        }
        assert_eq!(Packer::default().finish(), encode(&[]));
    }

    #[test]
    fn data_round_trips() {
        let empty: Vec<u8> = vec![0; 256 * 256 * 4];
        let mut stroke = empty.clone();
        for (i, b) in stroke.iter_mut().enumerate() {
            if (i / 4) % 997 < 30 {
                *b = (i % 251) as u8;
            }
        }
        let noise: Vec<u8> = (0..70_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8).collect();
        for data in [&empty, &stroke, &noise, &vec![7u8; 3], &Vec::new()] {
            let packed = encode(data);
            assert_eq!(decode(&packed, data.len()).as_deref(), Some(data.as_slice()));
        }
        // an empty tile packs small
        assert!(encode(&empty).len() < 2_000, "{}", encode(&empty).len());
    }

    /// Every kind of content a layer holds (and pieces of it of odd lengths, and big brush
    /// pictures as they are kept) packs to the very bytes it packed to before, and unpacks to
    /// itself, by the decoder of before too.
    #[test]
    fn every_kind_of_content_packs_as_before_and_round_trips() {
        let mut all: Vec<(String, Vec<u8>)> = KINDS.iter().map(|(name, kind)| (name.to_string(), kind(11))).collect();
        let joined: Vec<u8> = KINDS.iter().flat_map(|(_, kind)| kind(5)).collect();
        for len in [1, 2, 11, 12, 13, 17, 64, BLOCK - 1, BLOCK, BLOCK + 1, 300_001] {
            all.push((format!("{len} bytes"), joined[..len].to_vec()));
        }
        all.push(("all kinds".into(), joined));
        let side = crate::textures::SHARPEST;
        all.push(("a big shape".into(), crate::textures::shape_at(crate::textures::shape::SPLATTER, side).unwrap()));
        all.push(("a big grain".into(), crate::textures::grain_at(crate::textures::grain::GRUNGE, side).unwrap()));
        for (name, data) in &all {
            let packed = encode(data);
            assert!(packed == encode_before(data), "{name}: packed otherwise than before");
            assert!(decode(&packed, data.len()).as_ref() == Some(data), "{name}");
            assert!(decode_before(&packed, data.len()).as_ref() == Some(data), "{name}: as before");
        }
    }

    /// Long data is packed side by side to the bytes one thread packs it to (and a piece at a time
    /// to the same), and unpacked side by side to what one thread unpacks; where no thread can be
    /// made, the same again.
    #[test]
    fn long_data_is_packed_and_unpacked_side_by_side_as_on_one_thread() {
        let data = layer(soft, 6);
        assert!(data.len() >= SIDE_BY_SIDE * 2);
        let packed = encode(&data);
        assert!(packed == crate::par::within_threads(1, || encode(&data)));
        assert!(packed == encode_before(&data));
        let mut packer = Packer::default();
        for row in data.chunks(6 * SIDE * 4) {
            packer.push(row);
        }
        assert!(packer.finish() == packed);
        assert!(decode(&packed, data.len()).as_ref() == Some(&data));
        assert!(crate::par::within_threads(1, || decode(&packed, data.len())).as_ref() == Some(&data));
        crate::par::threads_fail(true);
        let alone = (encode(&data), decode(&packed, data.len()));
        crate::par::threads_fail(false);
        assert!(alone.0 == packed);
        assert!(alone.1.as_ref() == Some(&data));
    }

    /// A stream of `blocks`, framed: each (its bytes, its LZ4 data) compressed, or stored where it
    /// has none.
    fn framed(blocks: &[(Vec<u8>, Option<Vec<u8>>)]) -> Vec<u8> {
        let mut s = Vec::new();
        for (raw, lz4) in blocks {
            match lz4 {
                Some(lz4) => {
                    s.extend_from_slice(b"bv41");
                    s.extend_from_slice(&(raw.len() as u32).to_le_bytes());
                    s.extend_from_slice(&(lz4.len() as u32).to_le_bytes());
                    s.extend_from_slice(lz4);
                }
                None => {
                    s.extend_from_slice(b"bv4-");
                    s.extend_from_slice(&(raw.len() as u32).to_le_bytes());
                    s.extend_from_slice(raw);
                }
            }
        }
        s.extend_from_slice(b"bv4$");
        s
    }

    /// The blocks of `data` as this encoder writes them: (its bytes, its LZ4 data).
    fn our_blocks(data: &[u8]) -> Vec<(Vec<u8>, Option<Vec<u8>>)> {
        data.chunks(BLOCK)
            .map(|b| {
                let mut lz4 = Vec::new();
                compress_block(b, &mut Table::default(), &mut lz4);
                (b.to_vec(), Some(lz4))
            })
            .collect()
    }

    /// The LZ4 data of a block of `raw` bytes that is one match `offset` back, into the blocks
    /// before it where `offset` reaches past its start.
    fn match_block(raw: usize, offset: u16) -> Vec<u8> {
        let mut lz4 = vec![0x0F];
        lz4.extend_from_slice(&offset.to_le_bytes());
        let mut rest = raw - 4 - 15;
        while rest >= 255 {
            lz4.push(255);
            rest -= 255;
        }
        lz4.push(rest as u8);
        lz4
    }

    #[test]
    fn blocks_may_refer_back_into_earlier_ones() {
        // two blocks: 8 literals, then a block that is one match into the first
        let mut s = b"bv41".to_vec();
        s.extend_from_slice(&8u32.to_le_bytes());
        s.extend_from_slice(&9u32.to_le_bytes());
        s.push(0x80);
        s.extend_from_slice(b"ABCDEFGH");
        s.extend_from_slice(b"bv41");
        s.extend_from_slice(&8u32.to_le_bytes());
        s.extend_from_slice(&4u32.to_le_bytes());
        s.push(0x04);
        s.extend_from_slice(&8u16.to_le_bytes());
        s.push(0x00);
        s.extend_from_slice(b"bv4$");
        // the second block's sequence: no literals, a match 8 back, length 8 (4 + 4); then the
        // block ends with an empty literal run
        assert_eq!(decode(&s, 100).as_deref(), Some(&b"ABCDEFGHABCDEFGH"[..]));
        assert_eq!(decode_before(&s, 100).as_deref(), Some(&b"ABCDEFGHABCDEFGH"[..]));
    }

    /// In a long stream unpacked side by side, a block that refers back into the ones before it
    /// (as streams written elsewhere may) can't be decoded on its own: it is decoded after them,
    /// and so is one after it that refers back into it; the rest come out as on one thread.
    #[test]
    fn a_block_that_refers_back_is_decoded_after_those_before_it() {
        let data = layer(grainy, 5);
        let mut blocks = our_blocks(&data);
        assert!(blocks.len() > 20);
        // blocks 7 and 8 each a match reaching 60000 back (into the block before, then into
        // itself), and the last one stored
        for k in [7, 8] {
            blocks[k].1 = Some(match_block(BLOCK, 60_000));
        }
        let last = blocks.len() - 1;
        blocks[last].1 = None;
        let stream = framed(&blocks);
        let before = decode_before(&stream, usize::MAX).unwrap();
        assert!(before[..7 * BLOCK] == data[..7 * BLOCK] && before[9 * BLOCK..] == data[9 * BLOCK..]);
        assert!(before[7 * BLOCK..9 * BLOCK] != data[7 * BLOCK..9 * BLOCK], "the blocks were replaced");
        // they are the ones that can't be decoded on their own
        let (parsed, total) = super::blocks(&stream, usize::MAX).unwrap();
        let mut out = vec![0u8; total];
        let alone = side_by_side(&parsed, &mut out).unwrap();
        let not_alone: Vec<usize> = alone.iter().enumerate().filter(|(_, done)| !**done).map(|(k, _)| k).collect();
        assert_eq!(not_alone, [7, 8]);
        assert!(decode(&stream, usize::MAX) == Some(before.clone()));
        assert!(crate::par::within_threads(1, || decode(&stream, usize::MAX)) == Some(before));
    }

    #[test]
    fn damaged_streams_are_refused() {
        assert_eq!(decode(b"nope", 10), None);
        assert_eq!(decode(b"bv41\x10\x00\x00\x00\xff\x00\x00\x00", 100), None);
        let packed = encode(&[1u8; 1000]);
        assert_eq!(decode(&packed, 10), None, "over the limit");
        assert_eq!(decode(packed.get(..packed.len() - 4).unwrap(), 1000), None, "no end");
    }

    /// Damaged or over-long streams are refused (none, never a panic) as the decoder of before
    /// refused them, and the odd ones it read are still read alike: cut short anywhere, a block's
    /// sizes wrong, matches from before the start or past a block's end, an offset of 0, runs of
    /// lengths past the data, too long for the limit; and bytes changed at random, in short
    /// streams and in long ones unpacked side by side.
    #[test]
    fn damaged_streams_are_refused_as_before() {
        let same = |s: &[u8], limit: usize, what: &str| {
            let (new, old) = (decode(s, limit), decode_before(s, limit));
            assert!(new == old, "{what}: {:?} where before {:?}", new.map(|v| v.len()), old.map(|v| v.len()));
            new
        };
        // cut short anywhere
        let short: Vec<u8> = line_art(3).into_iter().take(150_000).collect();
        let packed = encode(&short);
        for cut in 0..packed.len() {
            assert_eq!(same(&packed[..cut], usize::MAX, &format!("cut at {cut}")), None);
        }
        // a block's sizes wrong, or of no kind
        let blocks = our_blocks(&short);
        for k in 0..blocks.len() {
            let at = framed(&blocks[..k]).len() - 4;
            for (field, by) in [(4, 1i64), (4, -1), (4, 1 << 20), (4, -(BLOCK as i64)), (8, 1), (8, -1), (8, 1 << 30)] {
                let mut s = packed.clone();
                let v = i64::from(u32::from_le_bytes(s[at + field..at + field + 4].try_into().unwrap()));
                let wrong = (v + by).clamp(0, i64::from(u32::MAX)) as u32;
                s[at + field..at + field + 4].copy_from_slice(&wrong.to_le_bytes());
                assert_eq!(same(&s, usize::MAX, &format!("block {k}: size at {field} by {by}")), None);
            }
            let mut s = packed.clone();
            s[at..at + 4].copy_from_slice(b"bv4x");
            assert_eq!(same(&s, usize::MAX, "a block of no kind"), None);
        }
        // matches from before the start, past the block's end, 0 back; runs past the data
        let one = |lz4: &[u8], raw: usize| framed(&[(vec![0; raw], Some(lz4.to_vec()))]);
        for (lz4, raw, what) in [
            (&[0x10, b'a', 2, 0][..], 5, "a match from before the start"),
            (&[0x40, 1, 2, 3, 4, 9, 0, 0x00][..], 12, "a match from before the start, after literals"),
            (&[0x40, 1, 2, 3, 4, 4, 0][..], 6, "a match past the block's end"),
            (&[0x40, 1, 2, 3, 4, 0, 0][..], 12, "an offset of 0"),
            (&[0xF0, 255, 255, 255][..], 4000, "a run of literals' length past the data"),
            (&[0xF0, 255, 255, 255, 3, 1, 2][..], 4000, "literals past the data"),
            (&[0x1F, 7, 1, 0, 255, 255][..], 4000, "a run of a match's length past the data"),
            (&[0x10, 7, 1][..], 6, "half an offset"),
            (&[0x10, 7][..], 2, "a block shorter than it says"),
            (&[0x30, 7, 7, 7][..], 2, "literals past the block's end"),
        ] {
            assert_eq!(same(&one(lz4, raw), usize::MAX, what), None, "{what}");
        }
        // what the decoder of before read of odd blocks is read alike: one ending in a match,
        // one with bytes past its literals' end, an empty one
        assert!(same(&one(&[0x14, 7, 1, 0], 9), usize::MAX, "ends in a match").is_some());
        assert!(same(&one(&[0x30, 7, 7, 7, 9, 9, 9], 3), usize::MAX, "bytes past its end").is_some());
        assert!(same(&one(&[], 0), usize::MAX, "an empty block").is_some());
        // too long for the limit, by a byte
        assert_eq!(same(&packed, short.len() - 1, "over the limit"), None);
        assert!(same(&packed, short.len(), "at the limit").is_some());
        // bytes changed at random: short streams, and long ones unpacked side by side
        let long = layer(soft, 4);
        let streams = [packed, encode(&soft(4)), encode(&noise(1)[..300]), encode(&long)];
        let mut r = Rng(99);
        let mut decoded = 0;
        for (n, s) in streams.iter().enumerate() {
            let tries = if n == 3 { 30 } else { 400 };
            for _ in 0..tries {
                let mut d = s.clone();
                for _ in 0..1 + r.next() % 4 {
                    let at = r.next() as usize % d.len().max(1);
                    match r.next() % 4 {
                        0 => d[at] = r.next() as u8,
                        1 => d[at] ^= 1 << (r.next() % 8),
                        2 => d.truncate(at),
                        _ => d.insert(at, r.next() as u8),
                    }
                    if d.is_empty() {
                        break;
                    }
                }
                // (limits as the callers' are: a size changed to gigabytes is refused, not made)
                let limits: &[usize] = if n == 3 { &[long.len(), long.len() + BLOCK] } else { &[4 << 20, 70_000] };
                for &limit in limits {
                    decoded += usize::from(same(&d, limit, "changed at random").is_some());
                }
            }
        }
        // (some changes leave a stream that still reads: a literal changed)
        assert!(decoded > 0);
    }

    /// Fastest of `runs` runs of `f`, ms (the run least held up by other work on the computer).
    fn best_ms<T>(runs: usize, mut f: impl FnMut() -> T) -> f64 {
        let mut best = f64::MAX;
        for _ in 0..runs {
            let t = std::time::Instant::now();
            std::hint::black_box(f());
            best = best.min(t.elapsed().as_secs_f64() * 1e3);
        }
        best
    }

    /// How long packing and unpacking take, before and now: a tile of each kind on one thread,
    /// and a 4096-square layer (256 tiles) of each painted kind, on one thread and side by side;
    /// with what the layer's 64 MiB cost made and written on their own (the pages the system hands
    /// over, which no decoder saves). A layer of soft paint, the slowest, unpacks in 120 ms at
    /// most on the computer this was written on (150 to 350 ms before), two and a half times as
    /// fast as before at least, and 1.8 times as fast on one thread. A bench, in a release build:
    /// `cargo test --release -p procraft-engine applz4 -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn a_layer_unpacks_quicker_than_before() {
        if cfg!(debug_assertions) {
            eprintln!("skipped: its times mean something in a release build only");
            return;
        }
        eprintln!("a tile, {SIDE} square, on one thread, fastest of 25 runs (microseconds):");
        eprintln!(
            "{:<11} {:>6}  {:>9} {:>9} {:>6}  {:>9} {:>9} {:>6}",
            "", "packed", "packing", "before", "", "unpacking", "before", ""
        );
        for (name, kind) in KINDS {
            let t = kind(11);
            let packed = encode(&t);
            let (e, e0) = (best_ms(25, || encode(&t)), best_ms(25, || encode_before(&t)));
            let (d, d0) = (best_ms(25, || decode(&packed, t.len())), best_ms(25, || decode_before(&packed, t.len())));
            eprintln!(
                "{name:<11} {:>5.1}%  {:>9.0} {:>9.0} {:>5.2}x  {:>9.0} {:>9.0} {:>5.2}x",
                packed.len() as f64 * 100.0 / t.len() as f64,
                e * 1e3,
                e0 * 1e3,
                e0 / e,
                d * 1e3,
                d0 * 1e3,
                d0 / d
            );
        }
        let n = 4096 * 4096 * 4;
        let pages = best_ms(5, || {
            let mut v = vec![0u8; n];
            for k in (0..n).step_by(4096) {
                v[k] = 1;
            }
            v
        });
        eprintln!(
            "a 4096-square layer, fastest of 5 runs (ms; side by side on {} threads; its 64 MiB made and \
             written alone: {pages:.0} ms):",
            crate::par::cores()
        );
        eprintln!(
            "{:<11} {:>8} {:>8} {:>8}  {:>9} {:>8} {:>8}",
            "", "packing", "one", "before", "unpacking", "one", "before"
        );
        let painted: [Kind; 4] = [("soft paint", soft), ("grainy", grainy), ("line art", line_art), ("empty", empty)];
        for (name, kind) in painted {
            let l = layer(kind, 16);
            let packed = encode(&l);
            assert!(packed == encode_before(&l));
            assert!(decode(&packed, n).as_ref() == Some(&l));
            let e = best_ms(5, || encode(&l));
            let e1 = best_ms(5, || crate::par::within_threads(1, || encode(&l)));
            let e0 = best_ms(5, || encode_before(&l));
            let d = best_ms(5, || decode(&packed, n));
            let d1 = best_ms(5, || crate::par::within_threads(1, || decode(&packed, n)));
            let d0 = best_ms(5, || decode_before(&packed, n));
            eprintln!(
                "{name:<11} {e:>8.0} {e1:>8.0} {e0:>8.0}  {d:>9.0} {d1:>8.0} {d0:>8.0}   ({:.1}x, one {:.1}x)",
                d0 / d,
                d0 / d1
            );
            if name == "soft paint" {
                assert!(d <= 120.0, "a soft layer unpacked in {d:.0} ms");
                assert!(d0 / d >= 2.5, "a soft layer unpacked only {:.1} times as fast as before", d0 / d);
                assert!(d0 / d1 >= 1.8, "a soft layer unpacked on one thread only {:.1} times as fast", d0 / d1);
            }
        }
    }

    /// Every `.lz4` file in the folder `LZ4_SAMPLES` names (and the folders in it): tiles taken out
    /// of packages, say, written here or elsewhere, or big brush pictures as kept. Each decodes as
    /// it did before; how many of their blocks refer back into the ones before them is told. Run
    /// by hand with `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn streams_in_a_folder_decode_as_before() {
        let Ok(dir) = std::env::var("LZ4_SAMPLES") else {
            eprintln!("skipped: no LZ4_SAMPLES");
            return;
        };
        let (mut files, mut blocks_seen, mut back, mut bytes) = (0, 0, 0, 0);
        let (mut now, mut before) = (0.0, 0.0);
        let mut dirs = vec![std::path::PathBuf::from(dir)];
        while let Some(d) = dirs.pop() {
            for entry in std::fs::read_dir(&d).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "lz4") {
                    continue;
                }
                let data = std::fs::read(&path).unwrap();
                let limit = 1 << 28;
                let decoded = decode(&data, limit);
                assert!(decoded == decode_before(&data, limit), "{} decodes otherwise", path.display());
                if let Some((blocks, _)) = super::blocks(&data, limit) {
                    blocks_seen += blocks.len();
                    back += blocks.iter().filter(|b| b.put(&mut vec![0; b.raw()], 0).is_none()).count();
                }
                files += 1;
                bytes += decoded.map_or(0, |d| d.len());
                now += best_ms(3, || decode(&data, limit));
                before += best_ms(3, || decode_before(&data, limit));
            }
        }
        eprintln!(
            "{files} streams, {:.1} MB, decoded as before ({blocks_seen} blocks, {back} referring back); \
             {now:.1} ms, before {before:.1} ms",
            bytes as f64 / 1e6
        );
    }
}
