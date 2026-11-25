//! Sample strokes drawn already, kept a while by what they were drawn from: the library asks for
//! a brush's again whenever it changes (every row's, after any brush is moved, renamed or edited),
//! and sharing a set or library wants every brush's at the file's size. A brush as it was drawn
//! finds its stroke here, whatever its place, and a brush changed in any way that shows draws
//! anew, as it isn't the brush its kept stroke was drawn from.

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use procraft_engine::brush::Brush;
use procraft_engine::zip::ZipReader;

/// How many bytes of kept strokes are held at most (each a PNG of the sample stroke's canvas,
/// some tens of KB): past it the least recently used go.
pub(crate) const KEPT_BYTES: usize = 32 << 20;

/// The stock Quick Look PNGs, rendered by the build's host tool and embedded for every platform.
static STOCK: LazyLock<Option<ZipReader<'static>>> = LazyLock::new(|| {
    let archive = ZipReader::new(include_bytes!("../../../assets/brushes/quick-look.zip"))?;
    (archive.read("stock-brushes")?.as_slice() == env!("PROCRAFT_STOCK_BRUSH_KEY").as_bytes()).then_some(archive)
});

// Match edits against this platform's stock brushes. The ZIP uses catalogue positions so the
// same build assets work even when a brush's calculated floating-point fields vary by platform.
static STOCK_POSITIONS: LazyLock<HashMap<u64, (usize, usize)>> = LazyLock::new(|| {
    crate::brush_library::LibrarySet::shipped(&crate::stock_library::shipped())
        .iter()
        .enumerate()
        .flat_map(|(set, row)| {
            row.brushes.iter().enumerate().map(move |(index, brush)| (look_key(brush), (set, index)))
        })
        .collect()
});

pub(crate) fn stock_png(key: u64) -> Option<Vec<u8>> {
    let archive = STOCK.as_ref()?;
    let (set, index) = STOCK_POSITIONS.get(&key)?;
    archive.read(&format!("{set}/{index}/QuickLook/Thumbnail.png"))
}

/// The version changes if the disk image format or the appearance key changes.
pub(crate) fn directory(library: &Path) -> PathBuf {
    library.join("brush-previews-v1")
}

fn cache_path(directory: &Path, key: u64) -> PathBuf {
    directory.join(format!("{key:016x}.png"))
}

/// The strokes kept, the most recently used last.
#[derive(Default)]
pub(crate) struct DrawnPreviews {
    kept: VecDeque<(u64, Arc<[u8]>)>,
    bytes: usize,
    /// How many asks found their stroke kept, and how many didn't (for the tests and the log).
    pub(crate) found: u64,
    pub(crate) missed: u64,
}

impl DrawnPreviews {
    /// The kept stroke drawn from a brush looking as `key` says (see [`look_key`]), as
    /// [`packed`] packs it; none if none is kept.
    pub(crate) fn find(&mut self, key: u64) -> Option<Arc<[u8]>> {
        let at = self.kept.iter().position(|(k, _)| *k == key)?;
        self.found += 1;
        let entry = self.kept.remove(at)?;
        let png = Arc::clone(&entry.1);
        self.kept.push_back(entry);
        Some(png)
    }

    /// Keeps `png`, the stroke drawn from a brush looking as `key` says, the least recently used
    /// let go past [`KEPT_BYTES`].
    pub(crate) fn keep(&mut self, key: u64, png: Arc<[u8]>) {
        if png.len() > KEPT_BYTES {
            return;
        }
        if let Some(at) = self.kept.iter().position(|(k, _)| *k == key)
            && let Some((_, old)) = self.kept.remove(at)
        {
            self.bytes -= old.len();
        }
        self.bytes += png.len();
        self.kept.push_back((key, png));
        while self.bytes > KEPT_BYTES {
            let Some((_, old)) = self.kept.pop_front() else { break };
            self.bytes -= old.len();
        }
    }

    /// Reads a generated preview from an earlier session.
    pub(crate) fn read(directory: &Path, key: u64) -> Option<Arc<[u8]>> {
        let mut file = fs::File::open(cache_path(directory, key)).ok()?.take(KEPT_BYTES as u64 + 1);
        let mut png = Vec::new();
        file.read_to_end(&mut png).ok()?;
        (png.len() <= KEPT_BYTES).then(|| png.into())
    }

    /// Keeps generated previews across sessions. If the directory goes over budget, older
    /// generated files are expendable because the brush settings are kept separately.
    pub(crate) fn write(directory: &Path, key: u64, png: &[u8]) {
        if png.len() > KEPT_BYTES || fs::create_dir_all(directory).is_err() {
            return;
        }
        let target = cache_path(directory, key);
        if crate::project::write_atomically(&target, |path| {
            fs::write(path, png).map_err(|e| crate::project::file_error("keeping a brush preview", e))
        })
        .is_err()
        {
            return;
        }
        let Ok(entries) = fs::read_dir(directory) else { return };
        let mut total = 0u64;
        let mut files: Vec<_> = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                path.extension().filter(|extension| *extension == "png")?;
                let metadata = entry.metadata().ok()?;
                metadata.is_file().then_some(())?;
                total = total.saturating_add(metadata.len());
                Some((metadata.modified().ok(), metadata.len(), path))
            })
            .collect();
        if total <= KEPT_BYTES as u64 {
            return;
        }
        files.sort_unstable();
        for (_, size, path) in files {
            if path != target && fs::remove_file(path).is_ok() {
                total = total.saturating_sub(size);
                if total <= KEPT_BYTES as u64 {
                    break;
                }
            }
        }
    }

    /// How many strokes are kept and their bytes.
    #[cfg(test)]
    pub(crate) fn held(&self) -> (usize, usize) {
        (self.kept.len(), self.bytes)
    }
}

/// The sample stroke's canvas (premultiplied RGBA8, `width` × `height`) packed to be kept: a PNG
/// of its coverage alone where it is white wherever it shows (the colour of each pixel its
/// coverage within [`WHITE_WITHIN`], as nearly every brush's is), else of its pixels as they are.
/// None if it can't be. What it gives back unpacked is what is shown, kept or just drawn.
pub(crate) fn packed(width: u32, height: u32, rgba: &[u8]) -> Option<Arc<[u8]>> {
    let (pixels, _) = rgba.as_chunks::<4>();
    let near = |c: u8, a: u8| c.abs_diff(a) <= WHITE_WITHIN;
    let png = if pixels.iter().all(|&[r, g, b, a]| near(r, a) && near(g, a) && near(b, a)) {
        let coverage: Vec<u8> = pixels.iter().map(|p| p[3]).collect();
        crate::brush_files::sized_gray_png(width, height, &coverage)
    } else {
        crate::project::png_of(width, height, rgba).ok()?
    };
    Some(png.into())
}

/// The canvas a kept stroke was packed from (see [`packed`]), if it is `width` × `height`.
pub(crate) fn unpacked(width: u32, height: u32, png: &[u8]) -> Option<Vec<u8>> {
    let (w, h, channels, pixels) = crate::brush_files::decode_picture(png)?;
    if (w, h) != (width as usize, height as usize) {
        return None;
    }
    match channels {
        1 => Some(pixels.iter().flat_map(|&v| [v; 4]).collect()),
        4 => Some(pixels),
        _ => None,
    }
}

/// How far a white stroke's colour may stray from its coverage, in levels, and be kept as its
/// coverage alone (the GPU rounds the two apart by a level here and there).
const WHITE_WITHIN: u8 = 2;

/// What a brush's sample stroke is drawn from, as a number: everything about it that can show in
/// the stroke, not what can't (its name, its place, its notches and sliders, its author, the
/// preview it came with, the point it resets to).
pub(crate) fn look_key(brush: &Brush) -> u64 {
    let mut into = HashWriter(0xcbf2_9ce4_8422_2325);
    // The key must be reproducible across app launches.
    let _ = std::fmt::Write::write_fmt(&mut into, format_args!("{:?}", looks(brush)));
    into.0
}

/// `brush` without what doesn't show in its stroke (see [`look_key`]), its second the same.
fn looks(brush: &Brush) -> Brush {
    let mut grain_file = brush.grain_file_now().cloned();
    if let Some(full) = grain_file.as_mut().and_then(|file| file.full.as_mut()) {
        full.pixels = None;
    }
    let mut shape_file = brush.shape_file_now().cloned();
    if let Some(file) = &mut shape_file {
        file.full.pixels = None;
    }
    let mut visual = Brush {
        grain_file,
        shape_file,
        name: String::new(),
        notches: Default::default(),
        sliders: [None; 3],
        thumbnail: None,
        author: None,
        reset_point: None,
        secondary: brush.secondary.as_deref().map(|second| Box::new(looks(second))),
        ..brush.clone()
    };
    // An explicitly stored default must match the same control left unset. Keep wet_pull
    // because its presence also tells the renderer that this brush mixes wet paint.
    let keys: Vec<_> = visual.extra.keys().filter(|key| *key != "wet_pull").cloned().collect();
    for key in keys {
        if let Some(value) = visual.extra.remove(&key)
            && value != visual.setting(&key)
        {
            visual.extra.insert(key, value);
        }
    }
    visual
}

/// Text written into a hash.
struct HashWriter(u64);

impl std::fmt::Write for HashWriter {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        for byte in s.bytes() {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn past_the_most_bytes_the_least_recently_used_strokes_go() {
        let mut kept = DrawnPreviews::default();
        let third: Arc<[u8]> = vec![0u8; KEPT_BYTES / 3].into();
        for key in 1..=3 {
            kept.keep(key, Arc::clone(&third));
        }
        assert_eq!(kept.held(), (3, 3 * (KEPT_BYTES / 3)));
        // 1 used again, so 2 is the least recently used when a fourth comes
        assert!(kept.find(1).is_some());
        kept.keep(4, Arc::clone(&third));
        assert!(kept.find(2).is_none());
        assert!([1, 3, 4].iter().all(|&k| kept.find(k).is_some()));
        assert!(kept.held().1 <= KEPT_BYTES);
        // kept again under the same key: replaced, not held twice
        kept.keep(4, vec![1u8; 10].into());
        assert_eq!(kept.held(), (3, 2 * (KEPT_BYTES / 3) + 10));
        assert_eq!(kept.find(4).as_deref(), Some(&[1u8; 10][..]));
        // one bigger than all may hold is never kept
        kept.keep(5, vec![0u8; KEPT_BYTES + 1].into());
        assert!(kept.find(5).is_none());
        assert_eq!(kept.found, 5);
    }

    #[test]
    fn generated_pngs_are_reused_by_a_fresh_session() {
        let root = std::env::temp_dir().join(format!("procraft-edited-brush-preview-{}", std::process::id()));
        let path = root.to_string_lossy().into_owned();
        let original = crate::CanvasSession::new(64, 64);
        original.open_brushes(path.clone());
        original.studio_begin();
        original.set_studio_setting("preview_size".to_owned(), 0.4567);
        original.studio_end(true);
        let (set, index, lookup) = {
            let st = original.shared.lock();
            let (set, index) = st.brush_ref(st.tool);
            (set as u32, index as u32, look_key(&st.library[set].brushes[index]))
        };
        assert!(original.keep_brushes(path.clone()));
        drop(original);
        let cw = procraft_engine::preview::CANVAS.0 as u32;
        let ch = procraft_engine::preview::CANVAS.1 as u32;
        let pixels = vec![123u8; (cw * ch * 4) as usize];
        let png = packed(cw, ch, &pixels).unwrap();
        DrawnPreviews::write(&directory(&root), lookup, &png);
        let fresh = crate::CanvasSession::new(64, 64);
        assert!(fresh.open_brushes(path));
        assert_eq!(fresh.brush_preview(set, index, cw, ch).unwrap().rgba, pixels);
        assert!(fresh.preview_canvas_bytes().is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_white_stroke_is_kept_as_its_coverage_and_any_other_as_it_is() {
        let white: Vec<u8> = (0..64u32).flat_map(|i| [(i * 4) as u8; 4]).collect();
        let packed_white = packed(8, 8, &white).unwrap();
        assert_eq!(unpacked(8, 8, &packed_white).as_deref(), Some(&white[..]));
        // a level or two off its coverage: white still, given back as its coverage
        let mut near = white.clone();
        near[4 * 9] = near[4 * 9 + 3] + 2;
        assert_eq!(unpacked(8, 8, &packed(8, 8, &near).unwrap()).as_deref(), Some(&white[..]));
        let mut tinted = white.clone();
        tinted[4 * 9] = 200;
        let packed_tinted = packed(8, 8, &tinted).unwrap();
        assert_eq!(unpacked(8, 8, &packed_tinted).as_deref(), Some(&tinted[..]));
        // a quarter of the bytes or less, for a white one
        assert!(packed_white.len() < packed_tinted.len());
        assert!(unpacked(4, 16, &packed_white).is_none());
    }

    #[test]
    fn a_brush_s_look_leaves_out_what_its_stroke_never_shows() {
        let pen = Brush::round_pen();
        let key = look_key(&pen);
        assert_eq!(key, look_key(&pen.clone()));
        let renamed = Brush { name: "Another".to_owned(), sliders: [Some([0.2, 0.3]); 3], ..pen.clone() };
        assert_eq!(look_key(&renamed), key);
        let mut explicit_default = pen.clone();
        explicit_default.set_setting("preview_size", pen.setting("preview_size"));
        assert_eq!(look_key(&explicit_default), key);
        let mut spaced = pen.clone();
        spaced.set_setting("spacing", 0.5);
        assert_ne!(look_key(&spaced), key);
        // a second counts, its name doesn't
        let dual = Brush { secondary: Some(Box::new(pen.clone())), ..pen.clone() };
        let named =
            Brush { secondary: Some(Box::new(Brush { name: "Second".to_owned(), ..pen.clone() })), ..pen.clone() };
        assert_ne!(look_key(&dual), key);
        assert_eq!(look_key(&named), look_key(&dual));
        // a picture drawn the first time it is read looks the same before as after
        let source = procraft_engine::brush::Source::drawn_later(
            7,
            procraft_engine::brush::Pixels::later(|| Some(vec![9; procraft_engine::textures::TEXTURE_SIZE.pow(2)])),
        );
        let mut shaped = pen.clone();
        shaped.shape.source = Some(source.clone());
        let before = look_key(&shaped);
        let _ = source.pixels.shared();
        assert_eq!(look_key(&shaped), before);
        // A full-size shape has the same key after the library writes its pixels to disk.
        shaped.shape_file = Some(procraft_engine::brush::ShapeFile {
            source: source.id,
            full: procraft_engine::brush::FullGrain::new(2, vec![33; 4]).unwrap(),
        });
        let full = look_key(&shaped);
        assert_ne!(full, before);
        shaped.shape_file.as_mut().unwrap().full.pixels = None;
        assert_eq!(look_key(&shaped), full);
    }
}
