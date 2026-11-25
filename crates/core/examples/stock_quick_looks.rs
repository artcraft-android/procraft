//! Renders the library's bundled brushes into a ZIP of Quick Look PNGs.

use std::io;
use std::path::PathBuf;

use procraft_core::CanvasSession;
use procraft_engine::zip::{Ready, ZipWriter};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).map(PathBuf::from).ok_or_else(|| {
        io::Error::other("usage: cargo run -p procraft-core --example stock_quick_looks -- <output.zip>")
    })?;
    let session = CanvasSession::new(64, 64);
    if !session.prepare_offscreen() {
        return Err(io::Error::other("could not open an offscreen renderer").into());
    }
    let mut archive = ZipWriter::new();
    archive.add("stock-brushes", env!("PROCRAFT_STOCK_BRUSH_KEY").as_bytes());
    let mut count = 0;
    for set in session.brush_library() {
        eprintln!("Rendering {}", set.name);
        for brush in set.brushes {
            let image = session
                // Match the library's preview dimensions so it can use the PNG without resizing.
                .render_stock_quick_look(brush.set, brush.index, 530, 162)
                .ok_or_else(|| io::Error::other(format!("could not render {}", brush.name)))?;
            let png = png(&image)?;
            let name = format!("{}/{}/QuickLook/Thumbnail.png", brush.set, brush.index);
            archive.add_ready(&name, Ready::of(&png, false));
            count += 1;
        }
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, archive.finish())?;
    eprintln!("Rendered {count} Quick Look PNGs");
    Ok(())
}

fn png(image: &procraft_core::ThumbnailImage) -> Result<Vec<u8>, png::EncodingError> {
    let mut pixels = Vec::new();
    let mut encoder_type = png::ColorType::GrayscaleAlpha;
    let (rgba, _) = image.rgba.as_chunks::<4>();
    if rgba.iter().all(|[r, g, b, a]| r.abs_diff(*a) <= 2 && g.abs_diff(*a) <= 2 && b.abs_diff(*a) <= 2) {
        pixels.reserve(rgba.len() * 2);
        for &[_, _, _, alpha] in rgba {
            pixels.extend([255, alpha]);
        }
    } else {
        encoder_type = png::ColorType::Rgba;
        pixels.reserve(image.rgba.len());
        for &[r, g, b, alpha] in rgba {
            let straight = |v| {
                if alpha == 0 {
                    0
                } else {
                    ((u32::from(v) * 255 + u32::from(alpha) / 2) / u32::from(alpha)).min(255) as u8
                }
            };
            pixels.extend([straight(r), straight(g), straight(b), alpha]);
        }
    }
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, image.width, image.height);
    encoder.set_color(encoder_type);
    encoder.set_compression(png::Compression::Balanced);
    encoder.write_header()?.write_image_data(&pixels)?;
    Ok(bytes)
}
