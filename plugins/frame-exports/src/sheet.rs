//! Sprite sheets: the animation's frames laid out on one picture, in a grid or a strip, with the
//! JSON a game engine reads to cut them out again (TexturePacker's "JSON (Array)" shape, which
//! Phaser, PixiJS, Godot importers and others read, with each frame's duration as Aseprite adds).
//!
//! Frames come in one at a time, as the app draws them, and only what each shows is kept: cut to
//! what is painted if asked, and a frame the same as one before kept once.

use crate::{Artwork, FRAME, File, Picture};

/// The most pixels across or down a sheet may be (what most image readers and GPUs still open).
const MOST_SIDE: u64 = 32_768;
/// The most pixels a sheet may hold in all (a gigabyte of RGBA).
const MOST_PIXELS: u64 = 1 << 28;

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

/// What a sheet is asked to be, as its controls are set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub arrangement: Arrangement,
    /// Clear pixels between neighbouring frames (none round the outside).
    pub padding: u32,
    /// Each frame cut down to what is painted in it, where it was kept in the JSON.
    pub trim: bool,
    /// A frame the same as one before (a held drawing, a cycle) shown by the same place on the
    /// sheet; consecutive ones one entry, lasting as long as they all do. Off, every frame is a
    /// place of its own, so a sheet with frames all one size can be cut evenly with no JSON.
    pub once: bool,
    /// The JSON beside the picture.
    pub json: bool,
}

impl Options {
    pub fn from_controls<const N: usize>(controls: &kit::Controls<N>) -> Options {
        let whole = |at: usize| controls.get(at).max(0.0).round() as u32;
        let arrangement = match whole(1) {
            1 => Arrangement::Across,
            2 => Arrangement::Down,
            _ => Arrangement::Grid { columns: whole(2), rows: whole(3) },
        };
        Options {
            arrangement,
            padding: whole(4).min(1024),
            trim: controls.on(5),
            once: controls.on(6),
            json: controls.on(7),
        }
    }
}

/// A rectangle in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// One picture on the sheet: what part of which frame, by the number the app keeps it by.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Cell {
    kept: u32,
    /// Where in the frame it is cut from (all of it, untrimmed).
    source: Rect,
    /// What it is known by, to find one met before: where it is painted, and the hash of that.
    key: (Option<[u32; 4]>, u64),
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
pub struct Sheet {
    art: Artwork,
    options: Options,
    cells: Vec<Cell>,
    entries: Vec<Entry>,
    frames: u32,
    /// The last frame's cell, for a held frame to show again.
    last: Option<usize>,
}

impl Sheet {
    pub fn new(art: Artwork, options: Options) -> Sheet {
        Sheet { art, options, cells: Vec::new(), entries: Vec::new(), frames: 0, last: None }
    }

    /// The animation's next frame told: the number the app is to keep it by, if it is new.
    pub fn add(&mut self, p: &Picture) -> Option<u32> {
        if p.kind != FRAME {
            return None;
        }
        let frame = self.frames;
        self.frames += 1;
        let (w, h) = (self.art.width, self.art.height);
        // held: the frame before again, not drawn twice
        let (cell, new) = match (p.held, self.last) {
            (true, Some(last)) if self.options.once => (last, false),
            (true, Some(last)) => {
                let again = self.cells.get(last).copied()?;
                self.cells.push(again);
                (self.cells.len() - 1, false)
            }
            _ => {
                let source = if self.options.trim {
                    p.painted.map_or(Rect { x: 0, y: 0, w: 1, h: 1 }, |[x, y, w, h]| Rect { x, y, w, h })
                } else {
                    Rect { x: 0, y: 0, w, h }
                };
                let key = (p.painted, p.hash);
                let found = self.options.once.then(|| self.cells.iter().position(|c| c.key == key)).flatten();
                match found {
                    Some(c) => (c, false),
                    None => {
                        self.cells.push(Cell { kept: frame, source, key });
                        (self.cells.len() - 1, true)
                    }
                }
            }
        };
        self.last = Some(cell);
        match self.entries.last_mut() {
            // held: the entry before lasts a frame longer
            Some(last) if self.options.once && last.cell == cell => last.frames += 1,
            _ => self.entries.push(Entry { cell, start: frame, frames: 1 }),
        }
        new.then_some(frame)
    }

    /// The sheet's files: its picture, and its JSON if asked for; none if there are no frames or
    /// it would be too large.
    pub fn finish(self) -> Option<(Vec<File>, String)> {
        let sizes: Vec<(u32, u32)> = self.cells.iter().map(|c| (c.source.w, c.source.h)).collect();
        let (width, height, spots) = arrange(&sizes, self.options.arrangement, self.options.padding)?;
        let name = plain(&self.art.name, "Animation");
        let image = format!("{name}.png");
        let mut places = Vec::new();
        let mut placed = Vec::new();
        for (cell, (x, y)) in self.cells.iter().zip(spots) {
            let s = cell.source;
            places.push([cell.kept, s.x, s.y, s.w, s.h, x, y]);
            placed.push(Rect { x, y, w: s.w, h: s.h });
        }
        let mut files = vec![File::Image(image.clone(), width, height, places)];
        if self.options.json {
            let json = self.json(&placed, &image, width, height);
            files.push(File::Bytes(format!("{name}.json"), json.into_bytes()));
        }
        let said = format!(
            "{} {} on a {width} × {height} sheet",
            self.frames,
            if self.frames == 1 { "frame" } else { "frames" }
        );
        Some((files, said))
    }

    /// The sheet's JSON: each entry in playing order with where it is on the sheet (`frame`),
    /// whether it was trimmed and from where in the frame (`spriteSourceSize`), the frame's size
    /// (`sourceSize`) and how long it shows in milliseconds (`duration`); then `meta`, naming the
    /// picture `image` beside it. Each entry is named as the files are, and its number from 0.
    fn json(&self, placed: &[Rect], image: &str, width: u32, height: u32) -> String {
        let (fw, fh) = (self.art.width, self.art.height);
        let fps = self.art.fps.max(1) as f32;
        // each entry named as the files are
        let name = plain(&self.art.name, "Animation");
        let mut frames = Vec::new();
        for (i, e) in self.entries.iter().enumerate() {
            let (Some(at), Some(cell)) = (placed.get(e.cell), self.cells.get(e.cell)) else { continue };
            let s = cell.source;
            let trimmed = (s.x, s.y, s.w, s.h) != (0, 0, fw, fh);
            let duration = ms_at(e.start + e.frames, fps) - ms_at(e.start, fps);
            let label = format!("{name} {i}");
            frames.push(format!(
                "    {{\n      \"filename\": {},\n      \"frame\": {{\n        \"x\": {},\n        \"y\": {},\n        \"w\": {},\n        \"h\": {}\n      }},\n      \"rotated\": false,\n      \"trimmed\": {trimmed},\n      \"spriteSourceSize\": {{\n        \"x\": {},\n        \"y\": {},\n        \"w\": {},\n        \"h\": {}\n      }},\n      \"sourceSize\": {{\n        \"w\": {fw},\n        \"h\": {fh}\n      }},\n      \"duration\": {duration}\n    }}",
                quoted(&label),
                at.x,
                at.y,
                at.w,
                at.h,
                s.x,
                s.y,
                s.w,
                s.h,
            ));
        }
        format!(
            "{{\n  \"frames\": [\n{}\n  ],\n  \"meta\": {{\n    \"app\": \"ProCraft\",\n    \"version\": \"1.0\",\n    \"image\": {},\n    \"format\": \"RGBA8888\",\n    \"size\": {{\n      \"w\": {width},\n      \"h\": {height}\n    }},\n    \"scale\": \"1\"\n  }}\n}}",
            frames.join(",\n"),
            quoted(image),
        )
    }
}

/// `text` as a JSON string.
fn quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `name` as a file's: ASCII letters, digits, `_` and `-`, anything else `_`; `fallback` if
/// nothing of it is left.
pub fn plain(name: &str, fallback: &str) -> String {
    crate::harmony::Names::plain(name, fallback)
}

/// When frame `frame` starts, in whole milliseconds, at `fps` frames a second (rounded, so
/// durations added up never drift from the animation's length).
fn ms_at(frame: u32, fps: f32) -> u64 {
    let fps = if fps.is_finite() && fps > 0.0 { f64::from(fps) } else { 1.0 };
    (f64::from(frame) * 1000.0 / fps).round() as u64
}

/// Where cells of `sizes` go, in order, laid out as `arrangement` with `padding` between them:
/// the sheet's width and height, and each cell's top left; none for no cells or a sheet too
/// large. Each column is as wide as its widest cell and each row as tall as its tallest, so cells
/// all one size make an even grid.
pub fn arrange(sizes: &[(u32, u32)], arrangement: Arrangement, padding: u32) -> Option<(u32, u32, Vec<(u32, u32)>)> {
    if sizes.is_empty() {
        return None;
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
    if width > MOST_SIDE || height > MOST_SIDE || width * height > MOST_PIXELS {
        return None;
    }
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
    Some((width as u32, height as u32, spots))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn art(frames: u32) -> Artwork {
        Artwork { width: 10, height: 8, fps: 12, frames, name: "Walk".into(), ..Artwork::default() }
    }

    fn frame(painted: Option<[u32; 4]>, hash: u64) -> Picture {
        Picture { kind: FRAME, painted, hash, ..Picture::default() }
    }

    fn held() -> Picture {
        Picture { kind: FRAME, held: true, ..Picture::default() }
    }

    fn options(arrangement: Arrangement, once: bool, trim: bool) -> Options {
        Options { arrangement, padding: 0, trim, once, json: true }
    }

    #[test]
    fn a_grid_is_as_near_square_as_it_can_be_and_columns_win() {
        assert_eq!(Arrangement::Grid { columns: 0, rows: 0 }.columns(5), 3);
        assert_eq!(Arrangement::Grid { columns: 0, rows: 2 }.columns(5), 3);
        assert_eq!(Arrangement::Grid { columns: 4, rows: 9 }.columns(5), 4);
        assert_eq!(Arrangement::Across.columns(5), 5);
        assert_eq!(Arrangement::Down.columns(5), 1);
        let (w, h, spots) = arrange(&[(10, 8); 5], Arrangement::Grid { columns: 0, rows: 0 }, 2).unwrap();
        assert_eq!((w, h), (34, 18));
        assert_eq!(spots[4], (12, 10));
        assert!(arrange(&[(40_000, 1)], Arrangement::Across, 0).is_none(), "too wide");
    }

    #[test]
    fn held_and_repeated_frames_are_kept_once_and_last_as_long_as_they_show() {
        let mut s = Sheet::new(art(5), options(Arrangement::Across, true, false));
        let a = Some([1, 1, 4, 4]);
        assert_eq!(s.add(&frame(a, 7)), Some(0));
        assert_eq!(s.add(&held()), None);
        assert_eq!(s.add(&frame(Some([2, 2, 4, 4]), 9)), Some(2));
        // a cycle back to the first: the same place on the sheet, an entry of its own
        assert_eq!(s.add(&frame(a, 7)), None);
        assert_eq!(
            s.entries,
            [
                Entry { cell: 0, start: 0, frames: 2 },
                Entry { cell: 1, start: 2, frames: 1 },
                Entry { cell: 0, start: 3, frames: 1 }
            ]
        );
        let (files, said) = s.finish().unwrap();
        assert_eq!(said, "4 frames on a 20 × 8 sheet");
        let File::Image(path, w, h, places) = &files[0] else { panic!() };
        assert_eq!((path.as_str(), *w, *h), ("Walk.png", 20, 8));
        assert_eq!(places, &[[0, 0, 0, 10, 8, 0, 0], [2, 0, 0, 10, 8, 10, 0]]);
        let File::Bytes(json_path, json) = &files[1] else { panic!() };
        assert_eq!(json_path, "Walk.json");
        let json = String::from_utf8(json.clone()).unwrap();
        // two frames at 12 a second: 167 ms
        assert!(json.contains("\"filename\": \"Walk 0\"") && json.contains("\"duration\": 167"), "{json}");
        assert!(json.contains("\"image\": \"Walk.png\""));
    }

    #[test]
    fn without_once_every_frame_has_a_place_and_trimmed_ones_say_from_where() {
        let mut s = Sheet::new(art(2), options(Arrangement::Down, false, true));
        assert_eq!(s.add(&frame(Some([3, 2, 4, 5]), 1)), Some(0));
        assert_eq!(s.add(&held()), None, "the frame before's picture, placed again");
        let (files, _) = s.finish().unwrap();
        let File::Image(_, w, h, places) = &files[0] else { panic!() };
        assert_eq!((*w, *h), (4, 10));
        assert_eq!(places, &[[0, 3, 2, 4, 5, 0, 0], [0, 3, 2, 4, 5, 0, 5]]);
        let File::Bytes(_, json) = &files[1] else { panic!() };
        let json = String::from_utf8(json.clone()).unwrap();
        assert!(json.contains("\"trimmed\": true"));
    }
}
