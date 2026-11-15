//! Brush files people bring in, run through what the iPad does with them: read, added to the
//! library (kept in a folder of its own), the library opened again from that folder as the next
//! artwork does, and every brush's preview drawn. `MAKE2D_BRUSH_FILES` (paths, one a line) names
//! the files; with none, the test does nothing.

use make2d_engine::view::View;
use make2d_gpu::Gpu;

use crate::CanvasSession;
use crate::brush_files::read_brushes;

fn session(dir: &str) -> Option<std::sync::Arc<CanvasSession>> {
    let gpu = Gpu::headless().ok()?;
    let s = CanvasSession::new(256, 256);
    {
        let mut st = s.shared.lock();
        st.window_size = (256, 256);
        st.view = Some(View::fit((256, 256), (256, 256)));
        st.gpu = Some(gpu);
        st.ensure_canvas();
    }
    let _ = s.open_brushes(dir.to_owned());
    Some(s)
}

#[test]
fn brush_files_named_by_make2d_brush_files_are_added_and_drawn() {
    let Ok(list) = std::env::var("MAKE2D_BRUSH_FILES") else { return };
    for path in list.lines().filter(|l| !l.trim().is_empty()) {
        let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned());
        let dir = std::env::temp_dir().join(format!("brush-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let dir = dir.to_string_lossy().into_owned();
        eprintln!("== {path}");
        let bytes = std::fs::read(path).unwrap_or_default();
        let t = std::time::Instant::now();
        let read = std::panic::catch_unwind(|| read_brushes(bytes.clone(), name.clone()));
        let Ok(read) = read else {
            eprintln!("   PANIC reading");
            continue;
        };
        let Some(read) = read else {
            eprintln!("   none read ({} ms)", t.elapsed().as_millis());
            continue;
        };
        eprintln!("   read in {} ms, library: {}", t.elapsed().as_millis(), read.is_library());
        let Some(s) = session(&dir) else { return };
        let added = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.add_brushes_read(read, 0)));
        eprintln!("   added: {added:?}");
        // the next artwork: the library opened again from its folder
        let again = std::panic::catch_unwind(|| session(&dir).map(|s| s.brush_library()));
        match again {
            Ok(Some(sets)) => {
                eprintln!("   opened again: {} sets", sets.len());
                let s = session(&dir);
                if let (Some(s), Some(first)) = (s, sets.first()) {
                    for b in &first.brushes {
                        let drawn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            s.brush_preview(b.set, b.index, 530, 162).is_some()
                        }));
                        if !matches!(drawn, Ok(true)) {
                            eprintln!("   preview of {:?}: {drawn:?}", b.name);
                        }
                    }
                }
            }
            other => eprintln!("   opening again: {:?}", other.map(|o| o.map(|s| s.len()))),
        }
    }
}

/// A thread with less stack than a session's state takes (some 130 KB), as an iPad's background
/// queue has too little for an unoptimised build's copies of it.
fn on_small_stack<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    std::thread::Builder::new().stack_size(96 * 1024).spawn(work).ok()?.join().ok()
}

#[test]
fn a_session_is_built_on_a_thread_with_little_stack() {
    let width = on_small_stack(|| crate::CanvasSession::new(8, 8).shared.lock().doc.width());
    assert_eq!(width, Some(8));
}

#[test]
fn the_brushes_are_read_ahead_on_a_thread_with_little_stack() {
    // a library kept in a folder, as one is once brushes are brought in
    let dir = std::env::temp_dir().join(format!("roomy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let dir = dir.to_string_lossy().into_owned();
    {
        let s = crate::CanvasSession::new(8, 8);
        let _ = s.open_brushes(dir.clone());
        s.new_brush_set();
        s.keep_brushes(dir.clone());
        let (library, _) = crate::brush_libraries::file_in_use(std::path::Path::new(&dir));
        assert!(library.is_file(), "a library kept");
    }
    let at = dir.clone();
    assert_eq!(on_small_stack(move || crate::brush_library::prepare_brushes(at)), Some(()));
    let _ = std::fs::remove_dir_all(&dir);
}
