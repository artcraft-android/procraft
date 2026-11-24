//! A video clip draws the picture its source hands over for the time shown, in its rect, posed
//! and faded as its clip is (skipped where the machine has no GPU).
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use procraft_engine::document::Document;
use procraft_engine::timeline::{Media, MediaId, MediaKind, Timeline};
use procraft_gpu::{Canvas, Gpu, VideoPicture, VideoSource};

const SIDE: u32 = 8;

fn gpu() -> Option<Gpu> {
    match Gpu::headless() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("skipped: {e}");
            None
        }
    }
}

/// Red for its first half second, blue after; counts the pictures asked for.
#[derive(Default)]
struct Flashes {
    asked: AtomicUsize,
    times: Mutex<Vec<f64>>,
}

impl VideoSource for Flashes {
    fn picture(&self, media: MediaId, seconds: f64) -> Option<VideoPicture> {
        assert_eq!(media, MediaId(7));
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.times.lock().unwrap().push(seconds);
        let colour = if seconds < 0.5 { [255, 0, 0, 255] } else { [0, 0, 255, 255] };
        Some(VideoPicture { width: 4, height: 4, rgba: colour.repeat(16) })
    }
}

fn pixel(rgba: &[u8], x: u32, y: u32) -> [u8; 4] {
    let at = ((y * SIDE + x) * 4) as usize;
    rgba[at..at + 4].try_into().unwrap()
}

#[test]
fn a_video_clip_draws_its_picture_for_the_time_shown() {
    let Some(gpu) = gpu() else { return };
    let mut doc = Document::new(SIDE, SIDE);
    let mut tl = Timeline::new(10, 1);
    tl.add_media(Media {
        id: MediaId(7),
        name: "Animatic".into(),
        kind: MediaKind::Video,
        seconds: 2.0,
        extension: "mov".into(),
        size: [4, 4],
    });
    let (_, clip) = tl.add_video(MediaId(7), 0, (SIDE, SIDE)).unwrap();
    assert!(tl.set_video_rect(clip, [2.0, 2.0, 4.0, 4.0]));
    doc.timeline = Some(tl);
    doc.background_visible = false;
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let source = Arc::new(Flashes::default());
    canvas.set_video_source(Some(source.clone()));
    let mut shown = |canvas: &mut Canvas, frame: u32| {
        canvas.set_timeline_frame(frame);
        canvas.invalidate();
        canvas.render_finished(&gpu, &mut doc);
        canvas.read_composite(&gpu).unwrap()
    };
    let first = shown(&mut canvas, 0);
    assert_eq!(pixel(&first, 2, 2), [255, 0, 0, 255]);
    assert_eq!(pixel(&first, 5, 5), [255, 0, 0, 255]);
    assert_eq!(pixel(&first, 0, 0)[3], 0, "outside its rect: nothing");
    assert_eq!(pixel(&first, 6, 6)[3], 0);
    assert_eq!(pixel(&first, 1, 3)[3], 0);
    // the same frame again: the picture held is used
    let asked = source.asked.load(Ordering::SeqCst);
    shown(&mut canvas, 0);
    assert_eq!(source.asked.load(Ordering::SeqCst), asked);
    // seven frames in at 10 a second
    let later = shown(&mut canvas, 7);
    assert_eq!(pixel(&later, 4, 4), [0, 0, 255, 255]);
    assert!(source.times.lock().unwrap().iter().any(|t| (t - 0.7).abs() < 1e-9));
    // past its end, nothing
    let after = shown(&mut canvas, 25);
    assert_eq!(pixel(&after, 4, 4)[3], 0);
}

/// As [`Flashes`], but each picture takes a while to read, as a decoder's does.
#[derive(Default)]
struct Slow(Flashes);

impl VideoSource for Slow {
    fn picture(&self, media: MediaId, seconds: f64) -> Option<VideoPicture> {
        std::thread::sleep(std::time::Duration::from_millis(60));
        self.0.picture(media, seconds)
    }
}

#[test]
fn pictures_read_aside_never_hold_a_render_up_and_a_render_read_back_still_waits() {
    let Some(gpu) = gpu() else { return };
    let mut doc = Document::new(SIDE, SIDE);
    let mut tl = Timeline::new(10, 1);
    tl.add_media(Media {
        id: MediaId(7),
        name: "Animatic".into(),
        kind: MediaKind::Video,
        seconds: 2.0,
        extension: "mov".into(),
        size: [4, 4],
    });
    tl.add_video(MediaId(7), 0, (SIDE, SIDE)).unwrap();
    doc.timeline = Some(tl);
    doc.background_visible = false;
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let source = Arc::new(Slow::default());
    canvas.set_video_source(Some(source.clone()));
    canvas.read_videos_aside();
    assert!(!canvas.videos_pending());

    // a frame drawn to show: it asks for the picture and goes on without it
    let started = std::time::Instant::now();
    canvas.set_timeline_frame(0);
    canvas.invalidate();
    canvas.render(&gpu, &mut doc, None);
    assert!(started.elapsed() < std::time::Duration::from_millis(50), "the render waited for the picture");
    assert!(canvas.videos_pending());
    // scrubbed on meanwhile: only the newest time asked for is read after the one under way
    for frame in 1..=7 {
        canvas.set_timeline_frame(frame);
        canvas.invalidate();
        canvas.render(&gpu, &mut doc, None);
    }
    // the app renders again while one is on its way; then the frame shows the picture for its time
    let mut renders = 0;
    while canvas.videos_pending() {
        std::thread::sleep(std::time::Duration::from_millis(5));
        canvas.render(&gpu, &mut doc, None);
        renders += 1;
        assert!(renders < 2000, "the picture never came");
    }
    canvas.render_finished(&gpu, &mut doc);
    assert_eq!(pixel(&canvas.read_composite(&gpu).unwrap(), 4, 4), [0, 0, 255, 255]);
    let asked = source.0.asked.load(Ordering::SeqCst);
    assert!(asked <= 3, "every frame scrubbed past was read: {asked}");
    assert!(source.0.times.lock().unwrap().iter().any(|t| (t - 0.7).abs() < 1e-9));

    // a render that is read back (an export's) waits for its frame's own picture
    canvas.set_timeline_frame(2);
    canvas.invalidate();
    canvas.render_finished(&gpu, &mut doc);
    assert_eq!(pixel(&canvas.read_composite(&gpu).unwrap(), 4, 4), [255, 0, 0, 255]);
    // and nothing read aside for another time takes its place afterwards
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        canvas.render(&gpu, &mut doc, None);
    }
    assert!(!canvas.videos_pending());
    canvas.render_finished(&gpu, &mut doc);
    assert_eq!(pixel(&canvas.read_composite(&gpu).unwrap(), 4, 4), [255, 0, 0, 255]);
}
