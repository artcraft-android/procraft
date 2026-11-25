//! Playback for the selected gallery page. Videos open in a local player on desktop.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use procraft_core::{Store, StoreGalleryEntry, StoreGalleryFrame, StoreGalleryImage, StoreVideo};

#[derive(Default)]
pub struct GalleryView {
    entry: Option<StoreGalleryEntry>,
    shown: Content,
    previous_videos: VecDeque<Video>,
}

#[derive(Default)]
enum Content {
    #[default]
    Empty,
    Loading(mpsc::Receiver<Result<Media, String>>),
    Image(Clip),
    Video(Video),
    Failed(String),
}

enum Media {
    Image(StoreGalleryImage),
    Video(Video),
}

struct Video {
    folder: tempfile::TempDir,
}

struct Clip {
    frames: Vec<StoreGalleryFrame>,
    texture: egui::TextureHandle,
    at: usize,
    next: Instant,
}

impl GalleryView {
    pub fn select(&mut self, entry: Option<&StoreGalleryEntry>) {
        if self.entry.as_ref() == entry {
            return;
        }
        if let Content::Video(video) = std::mem::take(&mut self.shown) {
            self.previous_videos.push_back(video);
        }
        while self.previous_videos.len() > 2 {
            self.previous_videos.pop_front();
        }
        self.entry = entry.cloned();
    }

    pub fn draw(
        &mut self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        rect: egui::Rect,
        store: &Arc<Store>,
        ink: egui::Color32,
        hint: egui::Color32,
    ) {
        let Some(entry) = self.entry.clone() else { return };
        let ctx = ui.ctx();
        if let Content::Loading(reply) = &self.shown {
            match reply.try_recv() {
                Ok(Ok(Media::Image(image))) => {
                    self.shown = image.failure.map_or_else(
                        || {
                            Clip::new(ctx, image.frames)
                                .map_or(Content::Failed("The image is unavailable.".to_owned()), Content::Image)
                        },
                        Content::Failed,
                    )
                }
                Ok(Ok(Media::Video(video))) => {
                    let launched = open_video(video.folder.path());
                    self.shown = Content::Video(video);
                    if let Err(error) = launched {
                        crate::files::tell("Can't play video", &error.to_string());
                    }
                }
                Ok(Err(reason)) => self.shown = Content::Failed(reason),
                Err(mpsc::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(50)),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.shown = Content::Failed("The gallery item couldn't be loaded.".to_owned())
                }
            }
        }
        if let Content::Image(clip) = &mut self.shown {
            if rect.intersects(painter.clip_rect()) {
                clip.draw(painter, rect);
            }
            return;
        }
        let action = |message: &str| {
            let button = egui::Rect::from_center_size(rect.center(), egui::vec2(235.0, 46.0));
            painter.rect_filled(button, 8.0, egui::Color32::from_black_alpha(30));
            crate::look::text(
                painter,
                button.center(),
                egui::Align2::CENTER_CENTER,
                message,
                17.0,
                crate::look::Weight::Medium,
                ink,
            );
            ui.interact(
                button.intersect(painter.clip_rect()),
                ui.scope_id().with("gallery media"),
                egui::Sense::click(),
            )
            .clicked()
        };
        match &self.shown {
            Content::Empty => {
                if entry.video.is_none() || action("Play video") {
                    self.load(store, entry);
                    ctx.request_repaint();
                }
            }
            Content::Loading(_) => show_message(painter, rect, "Loading gallery media…", hint),
            Content::Failed(reason) => {
                show_message(painter, rect, reason, hint);
                if action("Retry") {
                    self.load(store, entry);
                    ctx.request_repaint();
                }
            }
            Content::Video(video) => {
                if action("Play video again")
                    && let Err(error) = open_video(video.folder.path())
                {
                    crate::files::tell("Can't play video", &error.to_string());
                }
            }
            Content::Image(_) => {}
        }
    }

    fn load(&mut self, store: &Arc<Store>, entry: StoreGalleryEntry) {
        let store = Arc::clone(store);
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let media = if let Some(format) = entry.video {
                download_video(&store, entry.source, entry.url, format).map(Media::Video)
            } else {
                Ok(Media::Image(store.fetch_gallery_image_from(entry.source, entry.url)))
            };
            let _ = send.send(media);
        });
        self.shown = Content::Loading(receive);
    }
}

impl Clip {
    fn new(ctx: &egui::Context, frames: Vec<StoreGalleryFrame>) -> Option<Self> {
        let frame = frames.first()?;
        let texture = ctx.load_texture(
            "Store gallery",
            crate::pictures::colour_image(&frame.image),
            egui::TextureOptions::LINEAR,
        );
        let next = Instant::now() + Duration::from_millis(u64::from(frame.delay_ms));
        Some(Self { frames, texture, at: 0, next })
    }

    fn draw(&mut self, painter: &egui::Painter, rect: egui::Rect) {
        if self.frames.len() > 1 {
            let now = Instant::now();
            if now >= self.next {
                self.at = (self.at + 1) % self.frames.len();
                if let Some(frame) = self.frames.get(self.at) {
                    self.texture.set(crate::pictures::colour_image(&frame.image), egui::TextureOptions::LINEAR);
                    self.next = now + Duration::from_millis(u64::from(frame.delay_ms));
                }
            }
            painter.ctx().request_repaint_after(self.next.saturating_duration_since(now));
        }
        paint_picture(painter, rect, &self.texture);
    }
}

pub fn paint_picture(painter: &egui::Painter, art: egui::Rect, picture: &egui::TextureHandle) {
    let fit = picture.size_vec2() * (art.size() / picture.size_vec2()).min_elem();
    let placed = egui::Rect::from_center_size(art.center(), fit);
    crate::store::picture_rounded(
        painter,
        picture,
        placed,
        16.0,
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
    );
}

fn show_message(painter: &egui::Painter, rect: egui::Rect, message: &str, hint: egui::Color32) {
    let note = crate::look::wrapped(painter, message, 15.0, crate::look::Weight::Regular, hint, rect.width() - 48.0);
    painter.galley(egui::pos2(rect.center().x - note.size().x / 2.0, rect.top() + rect.height() * 0.64), note, hint);
}

fn download_video(store: &Store, source: String, address: String, format: StoreVideo) -> Result<Video, String> {
    let folder = tempfile::tempdir().map_err(|e| format!("The video couldn't be staged: {e}"))?;
    let filename = format!("demo.{}", format.extension());
    let path = folder.path().join(&filename);
    if let Some(why) = store.download_store_video_from(source, address, path.to_string_lossy().into_owned()) {
        return Err(why);
    }
    let page = format!(
        concat!(
            "<!doctype html><meta charset=utf-8><title>ProCraft Store demo</title>",
            "<meta http-equiv=Content-Security-Policy content=\"default-src 'none'; media-src file:; style-src 'unsafe-inline'\">",
            "<body style=\"background:#171717;color:white;margin:0;display:grid;place-items:center;min-height:100vh\">",
            "<video controls autoplay playsinline style=\"max-width:100vw;max-height:100vh\" src=\"{}\"></video>"
        ),
        filename
    );
    std::fs::write(folder.path().join("play.html"), page)
        .map_err(|e| format!("The player couldn't be prepared: {e}"))?;
    Ok(Video { folder })
}

fn open_video(folder: &Path) -> std::io::Result<()> {
    let opener = if cfg!(windows) {
        "explorer.exe"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener).arg(folder.join("play.html")).spawn().map(|_| ())
}
