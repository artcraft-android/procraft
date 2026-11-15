//! Brushes made of a Krita file, and read again from what was kept of each.
//!
//! A preset (`.kpp`) is a PNG: its picture the preset's preview, its settings XML in a text chunk
//! called `preset`, newer ones carrying the pictures they name. A bundle is a zip of presets
//! (`paintoppresets/`) and pictures (`brushes/`, `patterns/`). What a brush keeps is a record of
//! its settings (field 1) and the pictures it names (field 2 each: its file name, field 1, and
//! bytes, field 2): those the preset carries taken out of its settings and unpacked once, so that a
//! brush read again needn't pass over them (one kept with them in its settings reads as well).

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
        // the bundle's pictures, by their file names, each unpacked once
        let unpacked = std::cell::RefCell::new(std::collections::HashMap::<String, Option<Vec<u8>>>::new());
        let find = |file: &str| -> Option<Vec<u8>> {
            let plain = file.rsplit(['/', '\\']).next().unwrap_or(file);
            let found = names.iter().find(|n| {
                (n.starts_with("brushes/") || n.starts_with("patterns/")) && n.rsplit('/').next() == Some(plain)
            })?;
            unpacked.borrow_mut().entry(found.clone()).or_insert_with(|| zip.file(found)).clone()
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
    let (settings, pattern) = without_pattern(&without_resources(&text), &preset);
    data.field(1, settings.as_bytes());
    let mut added = Vec::new();
    if let Some((name, bytes)) = pattern {
        let mut r = Record::new();
        r.field(1, name.as_bytes()).field(2, &bytes);
        data.field(2, &r.bytes());
        added.push(name);
    }
    for file in preset.wanted_files().into_iter().filter(|f| !added.contains(f)) {
        let bytes = match preset.resource(&file) {
            Some(r) => r.bytes(),
            None => find(&file),
        };
        if let Some(bytes) = bytes {
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

/// A preset's text without the pictures it carries (its `resources`).
fn without_resources(text: &str) -> String {
    let Some(from) = text.find("<resources") else { return text.to_owned() };
    let to = match text[from..].find("</resources>") {
        Some(end) => from + end + "</resources>".len(),
        // an empty one
        None => match text[from..].find("/>") {
            Some(end) => from + end + 2,
            None => return text.to_owned(),
        },
    };
    let mut out = String::with_capacity(text.len() - (to - from));
    out.push_str(&text[..from]);
    out.push_str(&text[to..]);
    out
}

/// A preset's text without the texture's picture its settings carry (older presets carry it in a
/// setting, in base64), and that picture unpacked, by the name its settings give it (a setting
/// naming it put in where none does).
fn without_pattern(text: &str, preset: &Preset) -> (String, Option<(String, Vec<u8>)>) {
    const KEY: &str = "name=\"Texture/Pattern/Pattern\"";
    let carried = preset.get("Texture/Pattern/Pattern").and_then(crate::files::base64).filter(|b| !b.is_empty());
    let (Some(bytes), Some(at)) = (carried, text.find(KEY)) else { return (text.to_owned(), None) };
    let (Some(from), Some(end)) = (text[..at].rfind("<param"), text[at..].find("</param>")) else {
        return (text.to_owned(), None);
    };
    let to = at + end + "</param>".len();
    let named = ["Texture/Pattern/PatternFileName", "Texture/Pattern/Name"]
        .iter()
        .find_map(|k| preset.get(k).map(str::trim).filter(|n| !n.is_empty()));
    let name = named.unwrap_or("pattern").to_owned();
    let naming = if named.is_none() {
        format!(r#"<param name="Texture/Pattern/PatternFileName" type="string"><![CDATA[{name}]]></param>"#)
    } else {
        String::new()
    };
    (format!("{}{naming}{}", &text[..from], &text[to..]), Some((name, bytes)))
}

/// How much of a picture a preset carries is read when its look isn't made: its header (a pipe's
/// first cell's too).
const LIGHT_HEAD: usize = 4096;

/// The brush kept as `data`, read again ready to draw; `light`: its look not made (the app holds
/// it).
pub fn load(data: &[u8], light: bool) -> Option<Brush> {
    let mut text = None;
    let mut files: Vec<(String, &[u8])> = Vec::new();
    for (id, value) in fields(data) {
        match id {
            1 => text = Some(String::from_utf8_lossy(value).into_owned()),
            2 => {
                let (mut name, mut bytes) = (String::new(), &[][..]);
                for (f, v) in fields(value) {
                    match f {
                        1 => name = String::from_utf8_lossy(v).into_owned(),
                        2 => bytes = v,
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
        // its look not made, only the headers of the pictures it carries are read
        if let Some(r) = preset.resource(file) {
            return if light { r.head(LIGHT_HEAD) } else { r.bytes() };
        }
        let plain = file.rsplit(['/', '\\']).next().unwrap_or(file);
        files.iter().find(|(n, _)| n == file || n.rsplit(['/', '\\']).next() == Some(plain)).map(|(_, b)| b.to_vec())
    };
    Brush::read(&preset, &find, light)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Begin, Sample, Stroke};
    use std::rc::Rc;

    /// A GIMP brush `w` x `h`, grey `v`.
    fn gbr(w: u32, h: u32, v: u8) -> Vec<u8> {
        let mut b = Vec::new();
        for x in [29, 2, w, h, 1] {
            b.extend_from_slice(&u32::to_be_bytes(x));
        }
        b.extend_from_slice(b"GIMP");
        b.extend_from_slice(&u32::to_be_bytes(10));
        b.push(0);
        b.extend(std::iter::repeat_n(v, (w * h) as usize));
        b
    }

    fn base64_of(bytes: &[u8]) -> String {
        const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let b = [chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
            let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
            for i in 0..4 {
                out.push(if i <= chunk.len() { ABC[((n >> (18 - 6 * i)) & 63) as usize] as char } else { '=' });
            }
        }
        out
    }

    /// A texture's picture carried in a setting comes out of it as a file of the brush's, by its name.
    #[test]
    fn a_texture_carried_in_a_setting_leaves_it_unpacked() {
        let text = r#"<Preset name="P" paintopid="paintbrush"><param name="Texture/Pattern/Enabled"><![CDATA[true]]></param><param name="Texture/Pattern/Pattern" type="bytearray"><![CDATA[aGVsbG8=]]></param><param name="Texture/Pattern/Name"><![CDATA[paper]]></param></Preset>"#;
        let preset = Preset::read(text).unwrap();
        let (settings, pattern) = without_pattern(text, &preset);
        assert!(!settings.contains("aGVsbG8") && settings.contains("Texture/Pattern/Name"));
        assert_eq!(pattern, Some(("paper".to_owned(), b"hello".to_vec())));
        let again = Preset::read(&settings).unwrap();
        assert_eq!(again.get("Texture/Pattern/Pattern"), None);
        assert_eq!(again.wanted_files(), ["paper"]);
    }

    /// The pictures a preset carries come out of its settings as files of the brush's, unpacked.
    #[test]
    fn a_preset_s_pictures_leave_its_settings_unpacked() {
        let xml = r#"<Preset name="P" paintopid="paintbrush"><resources><resource name="t" filename="t.png"><![CDATA[aGVsbG8=]]></resource></resources><param name="brush_definition"><![CDATA[<Brush type="png_brush" filename="t.png"/>]]></param></Preset>"#;
        let mut kpp = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut text = b"preset\0".to_vec();
        text.extend_from_slice(xml.as_bytes());
        kpp.extend_from_slice(&(text.len() as u32).to_be_bytes());
        kpp.extend_from_slice(b"tEXt");
        kpp.extend_from_slice(&text);
        kpp.extend_from_slice(&[0; 4]);
        let made = import("p.kpp", &kpp);
        let (_, brush) = fields(&made).next().unwrap();
        let (_, data) = fields(brush).find(|(f, _)| *f == 2).unwrap();
        let settings =
            fields(data).find(|(f, _)| *f == 1).map(|(_, v)| String::from_utf8_lossy(v).into_owned()).unwrap();
        assert!(!settings.contains("resource") && settings.contains("brush_definition"));
        let (_, file) = fields(data).find(|(f, _)| *f == 2).unwrap();
        let file: Vec<(u16, &[u8])> = fields(file).collect();
        assert_eq!(file, [(1, &b"t.png"[..]), (2, &b"hello"[..])]);
    }

    /// Read only to draw (the app holding its look), a brush lays the same dabs as read whole: a pipe
    /// it carries, larger than the part of it read, picks among as many cells.
    #[test]
    fn a_brush_read_light_lays_the_dabs_it_does_read_whole() {
        let mut pipe = b"cells\n40 ncells:40 dim:1 rank0:40 sel0:random\n".to_vec();
        for i in 0..40 {
            pipe.extend(gbr(30, 20, i as u8));
        }
        let xml = format!(
            r#"<Preset name="Pipe" paintopid="paintbrush"><resources><resource name="cells" filename="cells.gih"><![CDATA[{}]]></resource></resources><param name="brush_definition" type="string"><![CDATA[<Brush type="gbr_brush" filename="cells.gih" spacing="0.5" scale="1" angle="0"/>]]></param><param name="Texture/Pattern/Enabled" type="string"><![CDATA[true]]></param><param name="Texture/Pattern/PatternFileName" type="string"><![CDATA[cells.gih]]></param></Preset>"#,
            base64_of(&pipe)
        );
        let mut data = Record::new();
        data.field(1, xml.as_bytes());
        let data = data.bytes();
        let draw = |light: bool| {
            let brush = load(&data, light).unwrap();
            assert_eq!(brush.tip.masks.len(), 40);
            assert_eq!(brush.tip.masks.iter().all(|m| m.pixels.is_empty()), light);
            let mut stroke = Stroke::new(
                Rc::new(brush),
                Begin { colour: [0.0; 3], secondary: [1.0; 3], diameter: 1.0, start_diameter: 1.0, seed: 3 },
            );
            let samples: Vec<Sample> = (0..60)
                .map(|i| Sample {
                    x: 10.0 + i as f32 * 4.0,
                    y: 50.0,
                    pressure: 0.8,
                    ms: i as f64 * 8.0,
                    ..Sample::default()
                })
                .collect();
            stroke.add(&samples);
            let mut out = Vec::new();
            stroke.out.iter().for_each(|d| d.write(&mut out));
            out
        };
        let whole = draw(false);
        assert!(whole.len() > 64 * 10);
        assert_eq!(draw(true), whole);
    }

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
            let Some(brush) = load(&data, false) else {
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
                    pressure: (i as f32 / 120.0 * std::f32::consts::PI).sin().max(0.05),
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
