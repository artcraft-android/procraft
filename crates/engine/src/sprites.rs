//! Sprite sheets: an animation's frames laid out on one picture, in a grid or a strip, with the
//! JSON a game engine reads to cut them out again (TexturePacker's "JSON (Array)" shape, which
//! Phaser, PixiJS, Godot importers and others read, with each frame's duration as Aseprite adds).
//!
//! Frames go in one at a time, as they are drawn, and only what each shows is kept: trimmed to
//! what is painted if asked, and a frame the same as one before kept once.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// The most pixels across or down a sheet may be (what most image readers and GPUs still open).
pub const MOST_SIDE: u32 = 32_768;
/// The most pixels a sheet may hold in all (a gigabyte of RGBA).
pub const MOST_PIXELS: u64 = 1 << 28;

/// How the frames are laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrangement {
    /// Rows of frames, left to right then top to bottom. Columns or rows 0 is worked out: with
    /// both 0 the grid is as near square as it can be; with one given, the other is as many as
    /// the frames need; with both given, columns win and the rows are as many as needed.
    Grid { columns: u32, rows: u32 },
    /// One row, left to right.
    Across,
    /// One column, top to bottom.
    Down,
}

impl Arrangement {
    /// How many columns `count` frames take.
    pub fn columns(self, count: u32) -> u32 {
        let count = count.max(1);
        match self {
            Arrangement::Across => count,
            Arrangement::Down => 1,
            Arrangement::Grid { columns, .. } if columns > 0 => columns.min(count),
            Arrangement::Grid { rows, .. } if rows > 0 => count.div_ceil(rows.min(count)),
            Arrangement::Grid { .. } => {
                // the smallest square that holds them all
                let mut side = (f64::from(count).sqrt()) as u32;
                while side.saturating_mul(side) < count {
                    side += 1;
                }
                side.max(1)
            }
        }
    }
}

/// What a sheet is asked to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SheetOptions {
    pub arrangement: Arrangement,
    /// Clear pixels between neighbouring frames (none round the outside).
    pub padding: u32,
    /// Each frame cut down to what is painted in it, where it was kept in the JSON.
    pub trim: bool,
    /// A frame the same as one before (a held drawing, a cycle) shown by the same place on the
    /// sheet; consecutive ones one entry, lasting as long as they all do. Off, every frame is a
    /// place of its own, so a sheet with frames all one size can be cut evenly with no JSON.
    pub once: bool,
}

/// A rectangle in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// One picture kept for the sheet: what part of the frame it is, and its pixels.
#[derive(Clone, Debug, PartialEq)]
struct Cell {
    /// Where in the frame it was cut from (all of it, untrimmed).
    source: Rect,
    /// Straight RGBA, `source.w` × `source.h`.
    rgba: Vec<u8>,
}

/// One entry in the JSON: a cell shown for so many of the animation's frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub cell: usize,
    /// The animation's frame it starts at.
    pub start: u32,
    /// How many of the animation's frames it lasts.
    pub frames: u32,
}

/// A sheet being put together, a frame at a time.
#[derive(Clone, Debug)]
pub struct SheetBuilder {
    options: SheetOptions,
    /// The frames' size; all are the same.
    size: Option<(u32, u32)>,
    cells: Vec<Cell>,
    /// Cells by a hash of their pixels, to find one met before.
    seen: HashMap<u64, Vec<usize>>,
    entries: Vec<Entry>,
    frames: u32,
}

/// What went wrong making a sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SheetError {
    NoFrames,
    /// A frame not the size of the first.
    FrameSize,
    /// Wider, taller or larger than [`MOST_SIDE`] and [`MOST_PIXELS`] allow.
    TooLarge {
        width: u64,
        height: u64,
    },
}

impl std::fmt::Display for SheetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SheetError::NoFrames => write!(f, "there are no frames"),
            SheetError::FrameSize => write!(f, "the frames differ in size"),
            SheetError::TooLarge { width, height } => {
                write!(f, "the sheet would be {width} × {height} pixels, too large to make")
            }
        }
    }
}

/// A sheet made: its picture and where each cell is on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    pub width: u32,
    pub height: u32,
    /// Straight RGBA.
    pub rgba: Vec<u8>,
    /// The frames' own size.
    pub frame_size: (u32, u32),
    /// Each cell: where it is on the sheet, and where in its frame it came from.
    pub places: Vec<(Rect, Rect)>,
    pub entries: Vec<Entry>,
}

impl SheetBuilder {
    pub fn new(options: SheetOptions) -> SheetBuilder {
        SheetBuilder { options, size: None, cells: Vec::new(), seen: HashMap::new(), entries: Vec::new(), frames: 0 }
    }

    /// Adds the animation's next frame, `width` × `height` straight RGBA.
    pub fn add(&mut self, width: u32, height: u32, rgba: &[u8]) -> Result<(), SheetError> {
        if *self.size.get_or_insert((width, height)) != (width, height)
            || rgba.len() != width as usize * height as usize * 4
        {
            return Err(SheetError::FrameSize);
        }
        let source = if self.options.trim {
            painted(width, height, rgba).unwrap_or(Rect { x: 0, y: 0, w: 1, h: 1 })
        } else {
            Rect { x: 0, y: 0, w: width, h: height }
        };
        let cut = crop(width, rgba, source);
        let frame = self.frames;
        self.frames += 1;
        let found = if self.options.once {
            let hash = hash_of(source, &cut);
            let found = self.seen.get(&hash).and_then(|cells| {
                cells.iter().copied().find(|&c| self.cells.get(c).is_some_and(|k| k.source == source && k.rgba == cut))
            });
            if found.is_none() {
                self.seen.entry(hash).or_default().push(self.cells.len());
            }
            found
        } else {
            None
        };
        let cell = match found {
            Some(c) => c,
            None => {
                self.cells.push(Cell { source, rgba: cut });
                self.cells.len() - 1
            }
        };
        match self.entries.last_mut() {
            // held: the entry before lasts a frame longer
            Some(last) if self.options.once && last.cell == cell => last.frames += 1,
            _ => self.entries.push(Entry { cell, start: frame, frames: 1 }),
        }
        Ok(())
    }

    /// How many of the animation's frames have been added.
    pub fn frames(&self) -> u32 {
        self.frames
    }

    /// The sheet: the cells laid out as asked.
    pub fn finish(self) -> Result<Sheet, SheetError> {
        let Some(frame_size) = self.size else { return Err(SheetError::NoFrames) };
        let sizes: Vec<(u32, u32)> = self.cells.iter().map(|c| (c.source.w, c.source.h)).collect();
        let (width, height, spots) = arrange(&sizes, self.options.arrangement, self.options.padding)?;
        let mut rgba = vec![0u8; width as usize * height as usize * 4];
        let mut places = Vec::with_capacity(self.cells.len());
        for (cell, (x, y)) in self.cells.iter().zip(spots) {
            let at = Rect { x, y, w: cell.source.w, h: cell.source.h };
            paste(&mut rgba, width, &cell.rgba, at);
            places.push((at, cell.source));
        }
        Ok(Sheet { width, height, rgba, frame_size, places, entries: self.entries })
    }
}

/// A sheet's width and height, and where each cell's top left is on it.
pub type Arranged = (u32, u32, Vec<(u32, u32)>);

/// Where cells of `sizes` go, in order, laid out as `arrangement` with `padding` between them:
/// the sheet's width and height, and each cell's top left. Each column is as wide as its widest
/// cell and each row as tall as its tallest, so cells all one size make an even grid.
pub fn arrange(sizes: &[(u32, u32)], arrangement: Arrangement, padding: u32) -> Result<Arranged, SheetError> {
    if sizes.is_empty() {
        return Err(SheetError::NoFrames);
    }
    let count = u32::try_from(sizes.len()).unwrap_or(u32::MAX);
    let columns = arrangement.columns(count) as usize;
    let rows = sizes.len().div_ceil(columns);
    let mut widths = vec![0u64; columns];
    let mut heights = vec![0u64; rows];
    for (i, &(w, h)) in sizes.iter().enumerate() {
        if let Some(c) = widths.get_mut(i % columns) {
            *c = (*c).max(u64::from(w));
        }
        if let Some(r) = heights.get_mut(i / columns) {
            *r = (*r).max(u64::from(h));
        }
    }
    let gap = u64::from(padding);
    let total = |v: &[u64]| v.iter().sum::<u64>() + gap * (v.len() as u64).saturating_sub(1);
    let (width, height) = (total(&widths), total(&heights));
    if width > u64::from(MOST_SIDE) || height > u64::from(MOST_SIDE) || width * height > MOST_PIXELS {
        return Err(SheetError::TooLarge { width, height });
    }
    // where each column and row starts
    let starts = |v: &[u64]| {
        v.iter()
            .scan(0u64, |at, size| {
                let here = *at;
                *at += size + gap;
                Some(here as u32)
            })
            .collect::<Vec<u32>>()
    };
    let (xs, ys) = (starts(&widths), starts(&heights));
    let spots = (0..sizes.len())
        .map(|i| (xs.get(i % columns).copied().unwrap_or(0), ys.get(i / columns).copied().unwrap_or(0)))
        .collect();
    Ok((width as u32, height as u32, spots))
}

/// The smallest rectangle holding every pixel of `rgba` (straight, `width` × `height`) that isn't
/// wholly clear; none if all of it is.
pub fn painted(width: u32, height: u32, rgba: &[u8]) -> Option<Rect> {
    let w = width as usize;
    let (mut left, mut top, mut right, mut bottom) = (usize::MAX, usize::MAX, 0, 0);
    for (y, row) in rgba.chunks_exact(w.max(1) * 4).take(height as usize).enumerate() {
        let Some(first) = row.as_chunks::<4>().0.iter().position(|p| p[3] != 0) else { continue };
        let last = row.as_chunks::<4>().0.iter().rposition(|p| p[3] != 0).unwrap_or(first);
        left = left.min(first);
        right = right.max(last);
        top = top.min(y);
        bottom = y;
    }
    (top != usize::MAX).then(|| Rect {
        x: left as u32,
        y: top as u32,
        w: (right - left + 1) as u32,
        h: (bottom - top + 1) as u32,
    })
}

/// The part `r` of `rgba` (`width` across), as its own pixels.
fn crop(width: u32, rgba: &[u8], r: Rect) -> Vec<u8> {
    let stride = width as usize * 4;
    let mut out = Vec::with_capacity(r.w as usize * r.h as usize * 4);
    for y in r.y..r.y + r.h {
        let from = y as usize * stride + r.x as usize * 4;
        match rgba.get(from..from + r.w as usize * 4) {
            Some(row) => out.extend_from_slice(row),
            None => out.resize(out.len() + r.w as usize * 4, 0),
        }
    }
    out
}

/// `cell` (`at.w` × `at.h`) put on `sheet` (`width` across) at `at`.
fn paste(sheet: &mut [u8], width: u32, cell: &[u8], at: Rect) {
    let stride = width as usize * 4;
    let row = at.w as usize * 4;
    for (y, line) in cell.chunks_exact(row.max(1)).take(at.h as usize).enumerate() {
        let to = (at.y as usize + y) * stride + at.x as usize * 4;
        if let Some(dest) = sheet.get_mut(to..to + row) {
            dest.copy_from_slice(line);
        }
    }
}

fn hash_of(source: Rect, rgba: &[u8]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (source.x, source.y, source.w, source.h).hash(&mut h);
    rgba.hash(&mut h);
    h.finish()
}

/// When frame `frame` starts, in whole milliseconds, at `fps` frames a second (rounded, so
/// durations added up never drift from the animation's length).
fn ms_at(frame: u32, fps: f32) -> u64 {
    let fps = if fps.is_finite() && fps > 0.0 { f64::from(fps) } else { 1.0 };
    (f64::from(frame) * 1000.0 / fps).round() as u64
}

impl Sheet {
    /// The sheet's JSON: each entry in playing order with where it is on the sheet (`frame`),
    /// whether it was trimmed and from where in the frame (`spriteSourceSize`), the frame's size
    /// (`sourceSize`) and how long it shows in milliseconds at `fps` (`duration`); then `meta`,
    /// naming the picture `image` beside it. Each entry is named `name` and its number from 0,
    /// as `name 0`... (with `name` empty, just the number).
    pub fn json(&self, image: &str, name: &str, fps: f32) -> String {
        let (fw, fh) = self.frame_size;
        let frames: Vec<serde_json::Value> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let (at, source) = self.places.get(e.cell)?;
                let trimmed = (source.x, source.y, source.w, source.h) != (0, 0, fw, fh);
                let duration = ms_at(e.start + e.frames, fps) - ms_at(e.start, fps);
                let label = if name.is_empty() { i.to_string() } else { format!("{name} {i}") };
                Some(serde_json::json!({
                    "filename": label,
                    "frame": { "x": at.x, "y": at.y, "w": at.w, "h": at.h },
                    "rotated": false,
                    "trimmed": trimmed,
                    "spriteSourceSize": { "x": source.x, "y": source.y, "w": source.w, "h": source.h },
                    "sourceSize": { "w": fw, "h": fh },
                    "duration": duration,
                }))
            })
            .collect();
        let doc = serde_json::json!({
            "frames": frames,
            "meta": {
                "app": "Make2D",
                "version": "1.0",
                "image": image,
                "format": "RGBA8888",
                "size": { "w": self.width, "h": self.height },
                "scale": "1",
            },
        });
        serde_json::to_string_pretty(&doc).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `w` × `h` frame, clear but for a square of colour `c` at `at`, `side` across.
    fn frame(w: u32, h: u32, at: (u32, u32), side: u32, c: u8) -> Vec<u8> {
        let mut px = vec![0u8; (w * h * 4) as usize];
        for y in at.1..at.1 + side {
            for x in at.0..at.0 + side {
                let i = ((y * w + x) * 4) as usize;
                px[i..i + 4].copy_from_slice(&[c, 0, 0, 255]);
            }
        }
        px
    }

    fn options(arrangement: Arrangement) -> SheetOptions {
        SheetOptions { arrangement, padding: 0, trim: false, once: false }
    }

    #[test]
    fn a_grid_is_as_near_square_as_can_be_unless_told() {
        let grid = |columns, rows| Arrangement::Grid { columns, rows };
        assert_eq!(grid(0, 0).columns(1), 1);
        assert_eq!(grid(0, 0).columns(4), 2);
        assert_eq!(grid(0, 0).columns(5), 3);
        assert_eq!(grid(0, 0).columns(9), 3);
        assert_eq!(grid(0, 0).columns(10), 4);
        // columns given; never more than there are frames
        assert_eq!(grid(4, 0).columns(10), 4);
        assert_eq!(grid(12, 0).columns(10), 10);
        // rows given: as many columns as fill them
        assert_eq!(grid(0, 2).columns(10), 5);
        assert_eq!(grid(0, 3).columns(10), 4);
        // both: columns win
        assert_eq!(grid(2, 2).columns(10), 2);
        assert_eq!(Arrangement::Across.columns(7), 7);
        assert_eq!(Arrangement::Down.columns(7), 1);
    }

    #[test]
    fn cells_all_one_size_make_an_even_grid_with_padding_between() {
        let (w, h, spots) = arrange(&[(10, 6); 5], Arrangement::Grid { columns: 3, rows: 0 }, 2).unwrap();
        // 3 across with 2 gaps, 2 down with 1
        assert_eq!((w, h), (34, 14));
        assert_eq!(spots, [(0, 0), (12, 0), (24, 0), (0, 8), (12, 8)]);
        let (w, h, spots) = arrange(&[(10, 6); 3], Arrangement::Across, 0).unwrap();
        assert_eq!((w, h, spots), (30, 6, vec![(0, 0), (10, 0), (20, 0)]));
        let (w, h, spots) = arrange(&[(10, 6); 3], Arrangement::Down, 1).unwrap();
        assert_eq!((w, h, spots), (10, 20, vec![(0, 0), (0, 7), (0, 14)]));
    }

    #[test]
    fn columns_and_rows_take_their_largest_cell() {
        let sizes = [(4, 2), (8, 3), (2, 9), (5, 1)];
        let (w, h, spots) = arrange(&sizes, Arrangement::Grid { columns: 2, rows: 0 }, 1).unwrap();
        // columns 4 and 8 wide, rows 3 and 9 tall
        assert_eq!((w, h), (4 + 1 + 8, 3 + 1 + 9));
        assert_eq!(spots, [(0, 0), (5, 0), (0, 4), (5, 4)]);
        // a trimmed strip packs tight
        let (w, h, spots) = arrange(&sizes, Arrangement::Across, 0).unwrap();
        assert_eq!((w, h, spots), (19, 9, vec![(0, 0), (4, 0), (12, 0), (14, 0)]));
    }

    #[test]
    fn a_sheet_too_large_is_refused() {
        assert_eq!(arrange(&[], Arrangement::Across, 0), Err(SheetError::NoFrames));
        let wide = arrange(&[(4000, 10); 9], Arrangement::Across, 0);
        assert_eq!(wide, Err(SheetError::TooLarge { width: 36_000, height: 10 }));
        let big = arrange(&[(20_000, 20_000)], Arrangement::Across, 0);
        assert!(matches!(big, Err(SheetError::TooLarge { .. })));
    }

    #[test]
    fn what_is_painted_is_found() {
        let px = frame(8, 6, (2, 1), 3, 9);
        assert_eq!(painted(8, 6, &px), Some(Rect { x: 2, y: 1, w: 3, h: 3 }));
        assert_eq!(painted(8, 6, &[0; 8 * 6 * 4]), None);
        let mut corner = vec![0u8; 8 * 6 * 4];
        corner[(5 * 8 + 7) * 4 + 3] = 1;
        assert_eq!(painted(8, 6, &corner), Some(Rect { x: 7, y: 5, w: 1, h: 1 }));
    }

    #[test]
    fn frames_go_on_the_sheet_where_it_says() {
        let mut b = SheetBuilder::new(options(Arrangement::Grid { columns: 2, rows: 0 }));
        for c in 1..=3 {
            b.add(4, 4, &frame(4, 4, (c - 1, 0), 1, c as u8)).unwrap();
        }
        let s = b.finish().unwrap();
        assert_eq!((s.width, s.height), (8, 8));
        assert_eq!(s.places.len(), 3);
        // frame 3 (colour 3 at 2,0 of its frame) is at the start of the second row
        let (at, source) = s.places[2];
        assert_eq!((at, source), (Rect { x: 0, y: 4, w: 4, h: 4 }, Rect { x: 0, y: 0, w: 4, h: 4 }));
        assert_eq!(s.rgba[((4 * 8 + 2) * 4) as usize], 3);
        // frame 2's square in the top right cell
        assert_eq!(s.rgba[((4 + 1) * 4) as usize], 2);
        assert!(s.entries.iter().all(|e| e.frames == 1));
    }

    #[test]
    fn trimmed_frames_keep_where_they_came_from() {
        let mut b = SheetBuilder::new(SheetOptions { trim: true, ..options(Arrangement::Across) });
        b.add(10, 10, &frame(10, 10, (6, 2), 2, 1)).unwrap();
        b.add(10, 10, &frame(10, 10, (1, 5), 3, 2)).unwrap();
        // nothing painted: a single clear pixel
        b.add(10, 10, &[0; 400]).unwrap();
        let s = b.finish().unwrap();
        assert_eq!((s.width, s.height), (2 + 3 + 1, 3));
        assert_eq!(s.places[0], (Rect { x: 0, y: 0, w: 2, h: 2 }, Rect { x: 6, y: 2, w: 2, h: 2 }));
        assert_eq!(s.places[1], (Rect { x: 2, y: 0, w: 3, h: 3 }, Rect { x: 1, y: 5, w: 3, h: 3 }));
        assert_eq!(s.places[2], (Rect { x: 5, y: 0, w: 1, h: 1 }, Rect { x: 0, y: 0, w: 1, h: 1 }));
        // the second square's pixels, all of it
        for y in 0..3 {
            for x in 2..5 {
                assert_eq!(s.rgba[((y * 6 + x) * 4) as usize], 2);
            }
        }
        let json: serde_json::Value = serde_json::from_str(&s.json("walk.png", "walk", 12.0)).unwrap();
        let first = &json["frames"][0];
        assert_eq!(first["trimmed"], true);
        assert_eq!(first["spriteSourceSize"], serde_json::json!({ "x": 6, "y": 2, "w": 2, "h": 2 }));
        assert_eq!(first["sourceSize"], serde_json::json!({ "w": 10, "h": 10 }));
        assert_eq!(first["frame"], serde_json::json!({ "x": 0, "y": 0, "w": 2, "h": 2 }));
    }

    #[test]
    fn held_frames_are_one_entry_lasting_as_long_as_they_do() {
        let a = frame(4, 4, (0, 0), 2, 1);
        let b = frame(4, 4, (2, 2), 2, 2);
        // a held 3, b held 2, a again (a cycle)
        let order = [&a, &a, &a, &b, &b, &a];
        let mut once = SheetBuilder::new(SheetOptions { once: true, ..options(Arrangement::Across) });
        let mut each = SheetBuilder::new(options(Arrangement::Across));
        for f in order {
            once.add(4, 4, f).unwrap();
            each.add(4, 4, f).unwrap();
        }
        let once = once.finish().unwrap();
        assert_eq!(once.places.len(), 2);
        assert_eq!(
            once.entries,
            [
                Entry { cell: 0, start: 0, frames: 3 },
                Entry { cell: 1, start: 3, frames: 2 },
                Entry { cell: 0, start: 5, frames: 1 }
            ]
        );
        assert_eq!(once.width, 8);
        let each = each.finish().unwrap();
        assert_eq!(each.places.len(), 6);
        assert_eq!(each.width, 24);

        // at 12 fps: 250 ms, 167 ms (rounded so they add up to the half second), 83 ms
        let json: serde_json::Value = serde_json::from_str(&once.json("run.png", "", 12.0)).unwrap();
        let frames = json["frames"].as_array().unwrap();
        assert_eq!(frames.len(), 3);
        let durations: Vec<u64> = frames.iter().map(|f| f["duration"].as_u64().unwrap()).collect();
        assert_eq!(durations, [250, 167, 83]);
        assert_eq!(durations.iter().sum::<u64>(), 500);
        // the cycle's last entry points back at the first cell
        assert_eq!(frames[2]["frame"], frames[0]["frame"]);
        assert_eq!(frames[1]["filename"], "1");
        assert_eq!(json["meta"]["image"], "run.png");
        assert_eq!(json["meta"]["size"], serde_json::json!({ "w": 8, "h": 4 }));
        assert_eq!(json["meta"]["format"], "RGBA8888");

        let json: serde_json::Value = serde_json::from_str(&each.json("run.png", "run", 12.0)).unwrap();
        assert_eq!(json["frames"].as_array().unwrap().len(), 6);
        assert_eq!(json["frames"][5]["filename"], "run 5");
        assert_eq!(json["frames"][5]["frame"]["x"], 20);
        assert_eq!(json["frames"][0]["trimmed"], false);
    }

    #[test]
    fn frames_of_another_size_are_refused() {
        let b = SheetBuilder::new(options(Arrangement::Across));
        assert_eq!(b.finish(), Err(SheetError::NoFrames));
        let mut b = SheetBuilder::new(options(Arrangement::Across));
        b.add(2, 2, &[0; 16]).unwrap();
        assert_eq!(b.add(3, 2, &[0; 24]), Err(SheetError::FrameSize));
        assert_eq!(b.add(2, 2, &[0; 15]), Err(SheetError::FrameSize));
        assert_eq!(b.frames(), 1);
    }
}
