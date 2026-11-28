//! Exports stopped by Cancel, given up on when the canvas stays held, and leaving no file either
//! way (see `export_run.rs`). Before, an export once begun ran to its end however long it took,
//! and waited for the canvas for good: its card stayed up with Cancel doing nothing (#419).

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::export_run::CANCELLED;
use crate::tests::{sample, session};
use crate::{CanvasSession, MobileError, PointerKind};

/// Paint on a layer with a mask, and a group of two layers: what a PSD and a package write
/// differently from a flat picture.
fn masked_and_grouped(s: &CanvasSession) {
    s.stroke_begin(PointerKind::Finger, vec![sample(10.0, 10.0, 0.0)]);
    s.stroke_end(vec![]);
    let lower = s.layers().into_iter().find(|l| l.current).unwrap().id;
    s.add_mask(lower).unwrap();
    s.add_layer();
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 30.0, 0.0)]);
    s.stroke_end(vec![]);
    assert!(s.pick_layer(lower));
    assert_ne!(s.group_picked(), 0);
}

/// A folder of its own for a test's files.
fn folder(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("procraft-export-run-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Every file left in `dir` (written, or begun beside its place and left there).
fn left_in(dir: &std::path::Path) -> Vec<String> {
    let mut left: Vec<String> = std::fs::read_dir(dir)
        .map(|d| d.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    left.sort();
    left
}

fn was_cancelled<T: std::fmt::Debug>(made: &Result<T, MobileError>) -> bool {
    matches!(made, Err(MobileError::File { reason }) if reason == CANCELLED)
}

/// An export as the tests make it.
type Export = Box<dyn Fn(&CanvasSession) -> Result<(), MobileError>>;

fn job(export: impl Fn(&CanvasSession) -> Result<(), MobileError> + 'static) -> Export {
    Box::new(export)
}

/// Each export the apps make, by name, writing into `dir`.
fn exports(dir: &std::path::Path) -> Vec<(&'static str, Export)> {
    let at = |name: &str| dir.join(name).to_string_lossy().into_owned();
    let (png, jpg, psd, tif, pdf, layers_pdf, package, gif, apng, layers) = (
        at("a.png"),
        at("a.jpg"),
        at("a.psd"),
        at("a.tif"),
        at("a.pdf"),
        at("layers.pdf"),
        at("a.procraft"),
        at("a.gif"),
        at("a-animated.png"),
        at("layers"),
    );
    vec![
        ("PNG", job(move |s: &CanvasSession| s.export_png(png.clone()))),
        ("JPEG", job(move |s: &CanvasSession| s.export_jpeg(jpg.clone()))),
        ("PSD", job(move |s: &CanvasSession| s.export_psd(psd.clone()))),
        ("TIFF", job(move |s: &CanvasSession| s.export_tiff(tif.clone()))),
        ("PDF", job(move |s: &CanvasSession| s.export_pdf(pdf.clone(), false, 90))),
        ("PDF of the layers", job(move |s: &CanvasSession| s.export_pdf(layers_pdf.clone(), true, 90))),
        ("package", job(move |s: &CanvasSession| s.export_package(package.clone(), 1, None))),
        ("GIF", job(move |s: &CanvasSession| s.export_gif(gif.clone(), 64, 12.0, false, false, false))),
        ("APNG", job(move |s: &CanvasSession| s.export_apng(apng.clone(), 64, 12.0, false))),
        ("layer PNGs", job(move |s: &CanvasSession| s.export_layer_pngs(layers.clone()).map(|_| ()))),
    ]
}

/// A Cancel that comes after the app has made an export ready (its card up) but before the core
/// has begun it stops every kind of export as it begins, leaving nothing; the next one made ready
/// is written whole.
#[test]
fn every_export_cancelled_before_it_begins_stops_and_leaves_no_file() {
    let Some(s) = session(64, 64) else { return };
    masked_and_grouped(&s);
    let dir = folder("armed");
    for (name, export) in exports(&dir) {
        s.reset_export_progress();
        s.cancel_export();
        let started = Instant::now();
        let made = export(&s);
        assert!(was_cancelled(&made), "{name}: {made:?}");
        assert!(started.elapsed() < Duration::from_secs(1), "{name}: {:?}", started.elapsed());
        assert_eq!(left_in(&dir), Vec::<String>::new(), "{name} left a file");
        assert_eq!(s.export_stage(), "", "{name}'s stage cleared");
    }
    for (name, export) in exports(&dir) {
        s.reset_export_progress();
        export(&s).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
    assert_eq!(left_in(&dir).len(), 10, "{:?}", left_in(&dir));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The desktop exports without making each ready first: a Cancel stops the one under way only,
/// never the next.
#[test]
fn an_export_after_a_cancelled_one_goes_on_without_being_made_ready() {
    let Some(s) = session(64, 64) else { return };
    let dir = folder("unarmed");
    let path = dir.join("a.png").to_string_lossy().into_owned();
    // a Cancel with none under way: the next goes on
    s.cancel_export();
    s.export_png(path.clone()).unwrap();
    s.export_png(path.clone()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// An export cancelled part way through its layers stops at the next one, its file never put in
/// place (nor anything begun beside it), long before it would have ended. The card's own Cancel
/// goes at once whatever the core does (see `ExportRunner.kt`); this is the work itself stopping.
#[test]
fn an_export_cancelled_part_way_stops_at_its_next_layer_and_leaves_no_file() {
    let Some(s) = session(2048, 64) else { return };
    for _ in 0..23 {
        s.add_layer();
    }
    let dir = folder("midway");
    let path = dir.join("a.psd").to_string_lossy().into_owned();
    // the whole of it, for how long it takes
    let started = Instant::now();
    s.reset_export_progress();
    s.export_psd(path.clone()).unwrap();
    let whole = started.elapsed();
    std::fs::remove_file(&path).unwrap();
    // again, cancelled once its second layer is being read
    s.reset_export_progress();
    let exporting = {
        let (s, path) = (Arc::clone(&s), path.clone());
        std::thread::spawn(move || {
            let started = Instant::now();
            let made = s.export_psd(path);
            (made, started.elapsed())
        })
    };
    let waited = Instant::now();
    while !s.export_stage().starts_with("reading layers (2 of") && !exporting.is_finished() {
        assert!(waited.elapsed() < Duration::from_secs(30), "never reached its second layer: {}", s.export_stage());
        std::thread::sleep(Duration::from_micros(200));
    }
    let cancelled_at = Instant::now();
    s.cancel_export();
    let (made, took) = exporting.join().unwrap();
    let stopped = cancelled_at.elapsed();
    println!(
        "24 layers of 2048×2048 as a PSD: whole {} ms; cancelled at its second layer, stopped {} ms later ({} ms in all)",
        whole.as_millis(),
        stopped.as_millis(),
        took.as_millis()
    );
    assert!(was_cancelled(&made), "{made:?}");
    assert!(stopped < Duration::from_millis(1000), "stopped {stopped:?} after its Cancel");
    assert!(took < whole, "cancelled {took:?}, whole {whole:?}");
    assert_eq!(left_in(&dir), Vec::<String>::new());
    let _ = std::fs::remove_dir_all(&dir);
}

/// An export waiting for a canvas something else holds gives up after its most, saying the canvas
/// stayed busy (before, it waited for good: a frame stuck on the GPU left the card up forever);
/// and a Cancel meanwhile stops it at once.
#[test]
fn an_export_waits_for_a_held_canvas_no_longer_than_its_most_and_not_past_a_cancel() {
    let Some(s) = session(64, 64) else { return };
    let dir = folder("held");
    let path = dir.join("a.png").to_string_lossy().into_owned();
    let most = Duration::from_millis(300);
    s.shared.exports.set_lock_most(most);
    let hold = |s: &Arc<CanvasSession>, ms: u64| {
        let s = Arc::clone(s);
        let (took, taken) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || {
            let guard = s.shared.lock();
            took.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(ms));
            drop(guard);
        });
        taken.recv().unwrap();
        holder
    };
    // held past its most: given up on, said why
    let holder = hold(&s, 3000);
    let started = Instant::now();
    s.reset_export_progress();
    let made = s.export_png(path.clone());
    let gave_up = started.elapsed();
    println!("held canvas: given up after {} ms (most {} ms)", gave_up.as_millis(), most.as_millis());
    match &made {
        Err(MobileError::File { reason }) => assert!(reason.contains("stayed busy"), "{reason}"),
        other => panic!("not given up on: {other:?}"),
    }
    assert!(gave_up >= most && gave_up < most + Duration::from_millis(700), "{gave_up:?}");
    holder.join().unwrap();
    // a Cancel while it waits: stopped at once
    s.shared.exports.set_lock_most(Duration::from_secs(30));
    let holder = hold(&s, 3000);
    s.reset_export_progress();
    let exporting = {
        let (s, path) = (Arc::clone(&s), path.clone());
        std::thread::spawn(move || s.export_png(path))
    };
    std::thread::sleep(Duration::from_millis(100));
    let cancelled_at = Instant::now();
    s.cancel_export();
    let made = exporting.join().unwrap();
    let stopped = cancelled_at.elapsed();
    println!("held canvas: stopped {} ms after its Cancel", stopped.as_millis());
    assert!(was_cancelled(&made), "{made:?}");
    assert!(stopped < Duration::from_millis(300), "{stopped:?}");
    holder.join().unwrap();
    // free again: written
    s.reset_export_progress();
    s.export_png(path.clone()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// An artwork not open (the gallery's share opens it just to write it, never shown) stops at
/// Cancel as one open does, leaving nothing, and is written whole after.
#[test]
fn an_artwork_written_offscreen_from_the_gallery_stops_at_cancel() {
    let Some(s) = session(512, 64) else { return };
    masked_and_grouped(&s);
    for _ in 0..5 {
        s.add_layer();
    }
    let dir = folder("offscreen");
    let artwork = dir.join("artwork");
    s.save(artwork.to_string_lossy().into_owned()).unwrap();
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let opened = CanvasSession::open(artwork.to_string_lossy().into_owned()).unwrap();
    opened.reset_export_progress();
    assert!(opened.prepare_offscreen());
    let psd = out.join("a.psd").to_string_lossy().into_owned();
    // cancelled as it began: nothing written
    opened.cancel_export();
    assert!(was_cancelled(&opened.export_psd(psd.clone())));
    assert_eq!(left_in(&out), Vec::<String>::new());
    // cancelled part way, from another thread: stopped, nothing written
    opened.reset_export_progress();
    let exporting = {
        let (o, psd) = (Arc::clone(&opened), psd.clone());
        std::thread::spawn(move || o.export_psd(psd))
    };
    let waited = Instant::now();
    while !opened.export_stage().starts_with("reading layers") && !exporting.is_finished() {
        assert!(waited.elapsed() < Duration::from_secs(30), "never read a layer");
        std::thread::sleep(Duration::from_micros(200));
    }
    opened.cancel_export();
    let made = exporting.join().unwrap();
    // (it may have ended before the Cancel came, on a fast GPU: then whole)
    if was_cancelled(&made) {
        assert_eq!(left_in(&out), Vec::<String>::new());
    } else {
        made.unwrap();
        std::fs::remove_file(&psd).unwrap();
    }
    // asked again: written whole
    opened.reset_export_progress();
    opened.export_psd(psd.clone()).unwrap();
    assert_eq!(left_in(&out), vec!["a.psd".to_owned()]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// While an export runs, its stage names where it is (for the app's log when it is cancelled or
/// seems stuck); none once it has ended.
#[test]
fn an_exports_stage_names_where_it_is_while_it_runs() {
    let Some(s) = session(1024, 64) else { return };
    for _ in 0..7 {
        s.add_layer();
    }
    let dir = folder("stage");
    let path = dir.join("a.psd").to_string_lossy().into_owned();
    let exporting = {
        let (s, path) = (Arc::clone(&s), path.clone());
        std::thread::spawn(move || s.export_psd(path))
    };
    let mut seen = std::collections::BTreeSet::new();
    while !exporting.is_finished() {
        let stage = s.export_stage();
        if let Some(kind) = stage.split(" (").next().filter(|k| !k.is_empty()) {
            seen.insert(kind.to_owned());
        }
    }
    exporting.join().unwrap().unwrap();
    println!("stages seen: {seen:?}");
    assert!(seen.contains("reading layers"), "{seen:?}");
    assert_eq!(s.export_stage(), "");
    let _ = std::fs::remove_dir_all(&dir);
}
