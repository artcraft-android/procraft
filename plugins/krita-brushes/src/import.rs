//! Brushes made of a Krita file, and read again from what was kept of each.
//!
//! A preset (`.kpp`) is a PNG: its picture the preset's preview, its settings XML in a text chunk
//! called `preset`, newer ones carrying the pictures they name. A bundle is a zip of presets
//! (`paintoppresets/`) and pictures (`brushes/`, `patterns/`). What a brush keeps is a record of
//! its settings (field 1) and the pictures it names that its settings don't carry (field 2 each:
//! its file name, field 1, and bytes, field 2).

use crate::brush::Brush;
use crate::files::{Zip, png_picture_only, png_texts};
use crate::preset::Preset;
use crate::rec::{Record, fields};

/// The preset a `.kpp` holds: its XML text.
fn preset_text(kpp: &[u8]) -> Option<String> {
    png_texts(kpp).into_iter().find(|(k, _)| k == "preset").map(|(_, t)| t)
}

/// The brushes made of a file called `name` (`bytes`): a bundle's or a preset's, as the record the
/// app takes; empty for none.
pub fn import(name: &str, bytes: &[u8]) -> Vec<u8> {
    let _ = name;
    let mut made = Record::new();
    if let Some(zip) = Zip::read(bytes) {
        let names: Vec<String> = zip.names().map(str::to_owned).collect();
        // the bundle's pictures, by their file names
        let find = |file: &str| -> Option<Vec<u8>> {
            let plain = file.rsplit(['/', '\\']).next().unwrap_or(file);
            let found = names.iter().find(|n| {
                (n.starts_with("brushes/") || n.starts_with("patterns/")) && n.rsplit('/').next() == Some(plain)
            })?;
            zip.file(found)
        };
        for n in names.iter().filter(|n| n.starts_with("paintoppresets/") && n.ends_with(".kpp")) {
            if let Some(kpp) = zip.file(n) {
                add(&mut made, &kpp, &find);
            }
        }
    } else if bytes.starts_with(b"\x89PNG") {
        add(&mut made, bytes, &|_| None);
    }
    made.bytes()
}

/// A brush made of preset `kpp`, its pictures found in it or by `find`, if it is one this engine
/// draws.
fn add(made: &mut Record, kpp: &[u8], find: &dyn Fn(&str) -> Option<Vec<u8>>) {
    let Some(text) = preset_text(kpp) else { return };
    let Some(preset) = Preset::read(&text) else { return };
    if preset.paintop != "paintbrush" {
        return;
    }
    let mut data = Record::new();
    data.field(1, text.as_bytes());
    for file in preset.wanted_files() {
        if preset.resource(&file).is_none()
            && let Some(bytes) = find(&file)
        {
            let mut r = Record::new();
            r.field(1, file.as_bytes()).field(2, &bytes);
            data.field(2, &r.bytes());
        }
    }
    let mut brush = Record::new();
    brush.field(1, preset.name.as_bytes()).field(2, &data.bytes());
    if let Some(png) = png_picture_only(kpp) {
        brush.field(3, &png);
    }
    made.field(1, &brush.bytes());
}

/// The brush kept as `data`, read again ready to draw.
pub fn load(data: &[u8]) -> Option<Brush> {
    let mut text = None;
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for (id, value) in fields(data) {
        match id {
            1 => text = Some(String::from_utf8_lossy(value).into_owned()),
            2 => {
                let (mut name, mut bytes) = (String::new(), Vec::new());
                for (f, v) in fields(value) {
                    match f {
                        1 => name = String::from_utf8_lossy(v).into_owned(),
                        2 => bytes = v.to_vec(),
                        _ => {}
                    }
                }
                files.push((name, bytes));
            }
            _ => {}
        }
    }
    let preset = Preset::read(&text?)?;
    let find = |file: &str| -> Option<Vec<u8>> {
        if let Some(r) = preset.resource(file) {
            return Some(r.bytes.clone());
        }
        let plain = file.rsplit(['/', '\\']).next().unwrap_or(file);
        files.iter().find(|(n, _)| n == file || n.rsplit(['/', '\\']).next() == Some(plain)).map(|(_, b)| b.clone())
    };
    Brush::read(&preset, &find)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Begin, Sample, Stroke};
    use std::rc::Rc;

    /// A Krita file's brushes made, read again and each drawn along a wave, with how long it took:
    /// `KRITA_FILE=x.bundle cargo test --release real_file -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn a_real_file_s_brushes_draw() {
        let Ok(path) = std::env::var("KRITA_FILE") else { return };
        let bytes = std::fs::read(&path).unwrap();
        let t = std::time::Instant::now();
        let made = import(&path, &bytes);
        let brushes: Vec<(String, Vec<u8>)> = fields(&made)
            .map(|(_, b)| {
                let mut name = String::new();
                let mut data = Vec::new();
                for (f, v) in fields(b) {
                    if f == 1 {
                        name = String::from_utf8_lossy(v).into_owned();
                    }
                    if f == 2 {
                        data = v.to_vec();
                    }
                }
                (name, data)
            })
            .collect();
        println!("{} brushes made in {:?}", brushes.len(), t.elapsed());
        for (name, data) in brushes {
            let t = std::time::Instant::now();
            let Some(brush) = load(&data) else {
                println!("  {name}: not read");
                continue;
            };
            let look = brush.look().len();
            let loaded = t.elapsed();
            let mut stroke = Stroke::new(
                Rc::new(brush),
                Begin { colour: [0.0; 3], secondary: [1.0; 3], diameter: 1.0, start_diameter: 1.0, seed: 7 },
            );
            let samples: Vec<Sample> = (0..120)
                .map(|i| Sample {
                    x: 100.0 + i as f32 * 7.0,
                    y: 300.0 + (i as f32 * 0.05).sin() * 40.0,
                    pressure: (i as f32 / 120.0 * 3.14).sin().max(0.05),
                    ms: i as f64 * 8.0,
                    ..Sample::default()
                })
                .collect();
            let t2 = std::time::Instant::now();
            stroke.add(&samples);
            println!("  {name}: look {look} B in {loaded:?}, {} dabs in {:?}", stroke.out.len(), t2.elapsed());
        }
    }
}
