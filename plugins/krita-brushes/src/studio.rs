//! The Krita brush studio: a panel of the plugin's own (the app's `panel` permission) showing a
//! Krita brush's settings as Krita names them, a page at a time, the pages its engine has: a pixel
//! brush's tip, spacing, size, opacity and flow, turn and squash, scatter and mirroring, colour and
//! texture; a colour smudge brush's smudge, its radius and colour rate; a sketch brush's lines; a
//! spray brush's area, particles, their dynamics and colours; a tangent normal brush's tilt; a quick
//! brush's own; a bristle brush's bristles and ink; a hatching brush's hatching; a curve brush's
//! curves; a particle brush's particles; a grid brush's grid and colours. Each option comes with the
//! sensor it follows and the curve it takes. The app opens
//! it in place of its own brush studio for a brush this plugin's engine made; each change is
//! written back to the brush (the app's `brush.data` permission) and draws from the next stroke on.

use crate::curve::Curve;
use crate::preset::{Preset, Resource};
use crate::rec::{Record, fields};

#[cfg(target_arch = "wasm32")]
mod app {
    #[link(wasm_import_module = "procraft")]
    unsafe extern "C" {
        pub fn show_panel(at: i32, len: i32) -> i32;
        pub fn close_panel();
        pub fn brush_data_len() -> i32;
        pub fn read_brush_data(to: i32) -> i32;
        pub fn write_brush_data(from: i32, len: i32) -> i32;
    }
}

/// The brush's data, as the app keeps it for this plugin's engine; none without one to edit.
fn read_data() -> Option<Vec<u8>> {
    #[cfg(target_arch = "wasm32")]
    {
        let len = usize::try_from(unsafe { app::brush_data_len() }).ok()?;
        let mut data = vec![0_u8; len];
        if unsafe { app::read_brush_data(data.as_mut_ptr() as i32) } != 1 {
            return None;
        }
        Some(data)
    }
    #[cfg(not(target_arch = "wasm32"))]
    None
}

fn write_data(data: &[u8]) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        unsafe { app::write_brush_data(data.as_ptr() as i32, data.len() as i32) == 1 }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = data;
        false
    }
}

fn show(json: &str) {
    #[cfg(target_arch = "wasm32")]
    unsafe {
        app::show_panel(json.as_ptr() as i32, json.len() as i32);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = json;
}

fn close() {
    #[cfg(target_arch = "wasm32")]
    unsafe {
        app::close_panel();
    }
}

/// What a page shows.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Shows {
    Tip,
    Spacing,
    Size,
    OpacityFlow,
    SizeOpacity,
    TurnSquash,
    Turn,
    ScatterMirror,
    Colour,
    Texture,
    Smudge,
    SmudgeColour,
    Tangent,
    Sketch,
    Lines,
    Quick,
    SprayArea,
    Particles,
    ShapeDynamics,
    SprayColour,
    Bristles,
    Ink,
    Hatching,
    AngleCrosshatching,
    SeparationThickness,
    Curves,
    CurveOptions,
    Particles2,
    Grid,
    GridColour,
    Chalk,
    Dyna,
}

/// A pixel brush's pages, as the page row offers them.
const PIXEL_PAGES: [(Shows, &str); 8] = [
    (Shows::Tip, "Tip"),
    (Shows::Spacing, "Spacing"),
    (Shows::Size, "Size"),
    (Shows::OpacityFlow, "Opacity and flow"),
    (Shows::TurnSquash, "Turn and squash"),
    (Shows::ScatterMirror, "Scatter and mirror"),
    (Shows::Colour, "Colour"),
    (Shows::Texture, "Texture"),
];

/// The pages a brush of Krita's engine `paintop` has (each engine's own options), at most eight.
fn pages(paintop: &str) -> &'static [(Shows, &'static str)] {
    match paintop {
        "colorsmudge" => &[
            (Shows::Tip, "Tip"),
            (Shows::Spacing, "Spacing"),
            (Shows::SizeOpacity, "Size and opacity"),
            (Shows::TurnSquash, "Turn and squash"),
            (Shows::ScatterMirror, "Scatter and mirror"),
            (Shows::Smudge, "Smudge"),
            (Shows::SmudgeColour, "Radius and colour"),
            (Shows::Texture, "Texture"),
        ],
        "tangentnormal" => &[
            (Shows::Tip, "Tip"),
            (Shows::Spacing, "Spacing"),
            (Shows::Size, "Size"),
            (Shows::OpacityFlow, "Opacity and flow"),
            (Shows::TurnSquash, "Turn and squash"),
            (Shows::ScatterMirror, "Scatter and mirror"),
            (Shows::Tangent, "Tangent tilt"),
            (Shows::Texture, "Texture"),
        ],
        "sketchbrush" => &[
            (Shows::Tip, "Tip"),
            (Shows::Sketch, "Sketch"),
            (Shows::SizeOpacity, "Size and opacity"),
            (Shows::Lines, "Lines"),
            (Shows::Turn, "Turn"),
        ],
        "spraybrush" => &[
            (Shows::SprayArea, "Spray area"),
            (Shows::Particles, "Particles"),
            (Shows::ShapeDynamics, "Shape dynamics"),
            (Shows::SprayColour, "Colour"),
            (Shows::SizeOpacity, "Size and opacity"),
            (Shows::Turn, "Turn"),
            (Shows::Tip, "Tip"),
        ],
        "roundmarker" => &[(Shows::Quick, "Quick brush"), (Shows::Size, "Size")],
        "hairybrush" => &[
            (Shows::Bristles, "Bristles"),
            (Shows::Ink, "Ink"),
            (Shows::SizeOpacity, "Size and opacity"),
            (Shows::Turn, "Turn"),
            (Shows::Tip, "Tip"),
        ],
        "hatchingbrush" => &[
            (Shows::Tip, "Tip"),
            (Shows::Spacing, "Spacing"),
            (Shows::Hatching, "Hatching"),
            (Shows::AngleCrosshatching, "Angle and crosshatching"),
            (Shows::SeparationThickness, "Separation and thickness"),
            (Shows::SizeOpacity, "Size and opacity"),
        ],
        "curvebrush" => &[(Shows::Curves, "Curves"), (Shows::CurveOptions, "Options")],
        "particlebrush" => &[(Shows::Particles2, "Particles")],
        "gridbrush" => &[(Shows::Grid, "Grid"), (Shows::GridColour, "Colour")],
        "chalkbrush" => &[(Shows::Chalk, "Chalk")],
        "dynabrush" => &[(Shows::Dyna, "Dyna")],
        _ => &PIXEL_PAGES,
    }
}

/// The sensors an option can be set to follow here, by Krita's ids, and what the page calls them.
const SENSORS: [(&str, &str); 8] = [
    ("pressure", "Pressure"),
    ("speed", "Speed"),
    ("fade", "Fade"),
    ("distance", "Distance"),
    ("drawingangle", "Drawing angle"),
    ("xtilt", "Tilt"),
    ("rotation", "Rotation"),
    ("fuzzy", "Random"),
];

/// Curves to pick, and Krita's way of writing each.
const CURVES: [(&str, &str); 7] = [
    ("Straight", "0,0;1,1;"),
    ("Soft rise", "0,0;0.5,0.25;1,1;"),
    ("Strong rise", "0,0;0.5,0.75;1,1;"),
    ("Late", "0,0;0.75,0.1;1,1;"),
    ("Early", "0,0;0.25,0.9;1,1;"),
    ("Falling", "0,1;1,0;"),
    ("Half to whole", "0,0.5;1,1;"),
];

/// Krita's texturing modes the page offers, by their numbers.
const TEXTURE_MODES: [(u32, &str); 8] = [
    (0, "Multiply"),
    (1, "Subtract"),
    (4, "Darken"),
    (5, "Overlay"),
    (6, "Colour dodge"),
    (7, "Colour burn"),
    (12, "Height"),
    (13, "Linear height"),
];

/// What a row of the page shown does when touched.
#[derive(Clone, Debug, PartialEq)]
enum Does {
    Page,
    /// A setting set to the value (scaled from the slider by `per`).
    Number {
        key: String,
        per: f32,
    },
    Flag(String),
    /// A curve setting of its own (not an option's), set to the chosen one.
    PlainCurve(String),
    /// A hatching's crosshatching style, kept as a flag for each.
    HatchStyle,
    /// A setting set to the chosen one of `values`.
    Choose {
        key: String,
        values: &'static [&'static str],
    },
    /// A brush element's attribute (its tip's, or its mask generator's).
    Attribute {
        child: bool,
        name: String,
        per: f32,
    },
    AutoSpacing,
    Sensor(String),
    Curve(String),
    PaintingMode,
    ScatterAxes,
    MirrorAxes,
    TextureMode,
    Reset,
    Nothing,
}

/// The studio's state, in the copy of the code its panel runs in.
#[derive(Default)]
pub struct Studio {
    preset: Option<Preset>,
    /// The pictures kept beside the settings, and the settings as they came (to reset to).
    files: Vec<(String, Vec<u8>)>,
    original: Option<String>,
    page: usize,
    rows: Vec<Does>,
}

impl Studio {
    /// Opens on the brush the app hands over.
    pub fn open(&mut self) {
        let Some(data) = read_data() else {
            show(
                r#"{ "title": "Krita brush studio", "rows": [{ "type": "text", "text": "Pick a brush that came from a Krita file, then open its studio." }] }"#,
            );
            return;
        };
        self.read(&data);
        self.page = 0;
        self.show();
    }

    fn read(&mut self, data: &[u8]) {
        self.files.clear();
        self.original = None;
        let mut text = None;
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
                    self.files.push((name, bytes));
                }
                3 => self.original = Some(String::from_utf8_lossy(value).into_owned()),
                _ => {}
            }
        }
        self.preset = text.as_deref().and_then(Preset::read);
        if self.original.is_none() {
            self.original = text;
        }
    }

    /// The brush's data as it now is, written back to the app.
    fn save(&self) {
        let Some(p) = &self.preset else { return };
        let mut data = Record::new();
        data.field(1, p.to_xml().as_bytes());
        for (name, bytes) in &self.files {
            let mut r = Record::new();
            r.field(1, name.as_bytes()).field(2, bytes);
            data.field(2, &r.bytes());
        }
        if let Some(original) = &self.original {
            data.field(3, original.as_bytes());
        }
        write_data(&data.bytes());
    }

    /// A touch on row `row` with `value`.
    pub fn touched(&mut self, row: usize, value: f32) {
        let Some(does) = self.rows.get(row).cloned() else { return };
        if does == Does::Page {
            let count = self.preset.as_ref().map_or(1, |p| pages(&p.paintop).len());
            self.page = (value.max(0.0) as usize).min(count - 1);
            self.show();
            return;
        }
        if does == Does::Reset {
            if let Some(original) = self.original.clone() {
                self.preset = Preset::read(&original);
                self.save();
            }
            self.show();
            return;
        }
        let Some(p) = self.preset.as_mut() else {
            close();
            return;
        };
        change(p, &does, value);
        self.save();
        self.show();
    }

    fn show(&mut self) {
        let Some(p) = self.preset.as_ref() else { return };
        let mut page = Page::new(&p.name);
        let all = pages(&p.paintop);
        self.page = self.page.min(all.len() - 1);
        let names: Vec<&str> = all.iter().map(|(_, name)| *name).collect();
        page.choice(Does::Page, "Page", &names, self.page);
        match all[self.page].0 {
            Shows::Tip => tip_page(p, &self.files, &mut page),
            Shows::Spacing => spacing_page(p, &mut page),
            Shows::Size => option_rows(p, &mut page, "Size", "Size", true),
            Shows::OpacityFlow => {
                let wash = p.number("PaintOpAction", 2.0) != 1.0;
                page.choice(Does::PaintingMode, "Painting mode", &["Build up", "Wash"], usize::from(wash));
                option_rows(p, &mut page, "Opacity", "Opacity", false);
                option_rows(p, &mut page, "Flow", "Flow", false);
            }
            Shows::SizeOpacity => {
                option_rows(p, &mut page, "Size", "Size", true);
                option_rows(p, &mut page, "Opacity", "Opacity", false);
            }
            Shows::TurnSquash => {
                option_rows(p, &mut page, "Rotation", "Rotation", true);
                option_rows(p, &mut page, "Ratio", "Ratio", true);
            }
            Shows::Turn => option_rows(p, &mut page, "Rotation", "Rotation", true),
            Shows::Smudge => smudge_page(p, &mut page),
            Shows::SmudgeColour => {
                option_rows(p, &mut page, "SmudgeRadius", "Smudge radius", true);
                option_rows(p, &mut page, "ColorRate", "Colour rate", true);
                option_rows(p, &mut page, "Gradient", "Gradient", true);
            }
            Shows::Tangent => tangent_page(p, &mut page),
            Shows::Sketch => sketch_page(p, &mut page),
            Shows::Lines => {
                option_rows(p, &mut page, "Line width", "Line width", true);
                option_rows(p, &mut page, "Offset scale", "Offset", true);
                option_rows(p, &mut page, "Density", "Density", true);
            }
            Shows::Quick => quick_page(p, &mut page),
            Shows::SprayArea => spray_area_page(p, &mut page),
            Shows::Particles => particles_page(p, &mut page),
            Shows::ShapeDynamics => shape_dynamics_page(p, &mut page),
            Shows::SprayColour => spray_colour_page(p, &mut page),
            Shows::Bristles => bristles_page(p, &mut page),
            Shows::Ink => ink_page(p, &mut page),
            Shows::Hatching => hatching_page(p, &mut page),
            Shows::AngleCrosshatching => {
                option_rows(p, &mut page, "Angle", "Angle", true);
                option_rows(p, &mut page, "Crosshatching", "Crosshatching", true);
            }
            Shows::SeparationThickness => {
                option_rows(p, &mut page, "Separation", "Separation", true);
                option_rows(p, &mut page, "Thickness", "Thickness", true);
            }
            Shows::Curves => curves_page(p, &mut page),
            Shows::CurveOptions => {
                option_rows(p, &mut page, "Line width", "Line width", true);
                option_rows(p, &mut page, "Curves opacity", "Curves' opacity", true);
                option_rows(p, &mut page, "Opacity", "Opacity", false);
            }
            Shows::Particles2 => particle_page(p, &mut page),
            Shows::Grid => grid_page(p, &mut page),
            Shows::GridColour => grid_colour_page(p, &mut page),
            Shows::Chalk => {
                page.setting(p, "Chalk/radius", "Radius", (0.0, 100.0), 1.0, 5.0, " px");
                page.flag(p, "Chalk/inkDepletion", "Ink runs out", false);
                page.flag(p, "Chalk/opacity", "Opacity falls", false);
                page.flag(p, "Chalk/saturation", "Saturation falls", false);
                option_rows(p, &mut page, "Opacity", "Opacity", false);
            }
            Shows::Dyna => dyna_page(p, &mut page),
            Shows::ScatterMirror => {
                option_rows(p, &mut page, "Scatter", "Scatter", true);
                let (x, y) = (p.flag("Scattering/AxisX", true), p.flag("Scattering/AxisY", true));
                let axes = match (x, y) {
                    (true, false) => 1,
                    (false, true) => 2,
                    _ => 0,
                };
                page.choice(Does::ScatterAxes, "Scatter", &["Both ways", "Along", "Across"], axes);
                option_rows(p, &mut page, "Mirror", "Mirror", true);
                let (h, v) = (p.flag("HorizontalMirrorEnabled", false), p.flag("VerticalMirrorEnabled", false));
                let mirror = match (h, v) {
                    (true, true) => 2,
                    (false, true) => 1,
                    _ => 0,
                };
                page.choice(Does::MirrorAxes, "Mirror", &["Across", "Down", "Both"], mirror);
            }
            Shows::Colour => {
                option_rows(p, &mut page, "Darken", "Darken", true);
                option_rows(p, &mut page, "Mix", "Mix", true);
            }
            Shows::Texture => texture_page(p, &mut page),
        }
        self.rows = page.does.clone();
        show(&page.json());
    }
}

/// A page being written: its rows as the app's panel takes them, and what each does.
struct Page {
    title: String,
    rows: Vec<String>,
    does: Vec<Does>,
}

/// Text as JSON writes it.
fn quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A number as JSON writes it.
fn number(v: f32) -> String {
    if v.is_finite() { format!("{v}") } else { "0".to_owned() }
}

/// The most rows a page has.
const MOST_ROWS: usize = 16;

impl Page {
    fn new(name: &str) -> Page {
        let title: String = name.chars().take(40).collect();
        Page { title, rows: Vec::new(), does: Vec::new() }
    }

    fn row(&mut self, json: String, does: Does) {
        if self.rows.len() < MOST_ROWS {
            self.rows.push(json);
            self.does.push(does);
        }
    }

    fn text(&mut self, text: &str) {
        let text: String = text.chars().take(400).collect();
        self.row(format!(r#"{{ "type": "text", "text": {} }}"#, quoted(&text)), Does::Nothing);
    }

    #[allow(clippy::too_many_arguments)]
    fn slider(&mut self, does: Does, label: &str, least: f32, most: f32, step: f32, value: f32, unit: &str) {
        let value = value.clamp(least, most);
        self.row(
            format!(
                r#"{{ "type": "slider", "label": {}, "least": {}, "most": {}, "step": {}, "value": {}, "unit": {} }}"#,
                quoted(label),
                number(least),
                number(most),
                number(step),
                number(value),
                quoted(unit)
            ),
            does,
        );
    }

    fn switch(&mut self, does: Does, label: &str, on: bool) {
        self.row(format!(r#"{{ "type": "switch", "label": {}, "on": {on} }}"#, quoted(label)), does);
    }

    fn choice(&mut self, does: Does, label: &str, options: &[&str], chosen: usize) {
        let options: Vec<String> =
            options.iter().take(8).map(|o| quoted(&o.chars().take(24).collect::<String>())).collect();
        let chosen = chosen.min(options.len().saturating_sub(1));
        self.row(
            format!(
                r#"{{ "type": "choice", "label": {}, "options": [{}], "chosen": {chosen} }}"#,
                quoted(label),
                options.join(", ")
            ),
            does,
        );
    }

    fn button(&mut self, does: Does, label: &str) {
        self.row(format!(r#"{{ "type": "button", "label": {} }}"#, quoted(label)), does);
    }

    /// A slider for setting `key` of `p` (`otherwise` where it says none), shown as the setting
    /// divided by `per`.
    #[allow(clippy::too_many_arguments)]
    fn setting(
        &mut self,
        p: &Preset,
        key: &str,
        label: &str,
        (least, most): (f32, f32),
        per: f32,
        otherwise: f32,
        unit: &str,
    ) {
        let value = p.number(key, otherwise) / per;
        self.slider(Does::Number { key: key.to_owned(), per }, label, least, most, 1.0, value, unit);
    }

    /// A switch for flag `key` of `p`.
    fn flag(&mut self, p: &Preset, key: &str, label: &str, otherwise: bool) {
        self.switch(Does::Flag(key.to_owned()), label, p.flag(key, otherwise));
    }

    /// A choice of `names` for setting `key` of `p`, each written as the one of `values` at its place.
    fn chosen(
        &mut self,
        p: &Preset,
        key: &str,
        label: &str,
        names: &[&str],
        values: &'static [&'static str],
        otherwise: usize,
    ) {
        let now = p.get(key).map(str::trim);
        let at = now.and_then(|v| values.iter().position(|x| *x == v)).unwrap_or(otherwise);
        self.choice(Does::Choose { key: key.to_owned(), values }, label, names, at);
    }

    fn json(&self) -> String {
        format!(r#"{{ "title": {}, "rows": [{}] }}"#, quoted(&self.title), self.rows.join(", "))
    }
}

/// The preset's `Brush` element, its tip.
fn tip_element(p: &Preset) -> Option<crate::xml::Element> {
    p.element("brush_definition")
}

fn tip_page(p: &Preset, files: &[(String, Vec<u8>)], page: &mut Page) {
    let Some(tip) = tip_element(p) else {
        page.text("This brush's tip can't be read.");
        return;
    };
    let attr = |name: &str, child: bool| -> f32 {
        if child { tip.child("MaskGenerator").and_then(|g| g.number(name)) } else { tip.number(name) }.unwrap_or(0.0)
    };
    let set = |name: &str, child: bool, per: f32| Does::Attribute { child, name: name.to_owned(), per };
    if tip.attribute("type") == Some("auto_brush") {
        let g = tip.child("MaskGenerator");
        let shape = match g.and_then(|g| g.attribute("type")) {
            Some("rect") => "rectangle",
            _ => "circle",
        };
        let edge = match g.and_then(|g| g.attribute("id")) {
            Some("soft") => "soft",
            Some("gauss") => "Gaussian",
            _ => "default",
        };
        page.text(&format!("An auto tip: a {shape}, its edge {edge}."));
        page.slider(set("diameter", true, 1.0), "Diameter", 1.0, 1000.0, 1.0, attr("diameter", true), " px");
        page.slider(set("ratio", true, 0.01), "Ratio", 1.0, 100.0, 1.0, attr("ratio", true) * 100.0, "%");
        page.slider(set("hfade", true, 0.01), "Fade across", 0.0, 100.0, 1.0, attr("hfade", true) * 100.0, "%");
        page.slider(set("vfade", true, 0.01), "Fade down", 0.0, 100.0, 1.0, attr("vfade", true) * 100.0, "%");
        page.slider(set("spikes", true, 1.0), "Spikes", 2.0, 20.0, 1.0, attr("spikes", true).max(2.0), "");
        page.slider(set("density", false, 0.01), "Density", 0.0, 100.0, 1.0, attr("density", false) * 100.0, "%");
        page.slider(
            set("randomness", false, 0.01),
            "Randomness",
            0.0,
            100.0,
            1.0,
            attr("randomness", false) * 100.0,
            "%",
        );
    } else {
        let file = tip.attribute("filename").unwrap_or("its picture");
        // carried in the preset, or beside it
        let plain = |n: &str| n.rsplit(['/', '\\']).next().unwrap_or(n).to_owned();
        let carried = p.resource(file).is_some() || files.iter().any(|(n, _)| plain(n) == plain(file));
        if carried {
            page.text(&format!("A picture tip: {file}."));
        } else {
            page.text(&format!(
                "A picture tip: {file}, which the file doesn't hold. As in Krita, its default tip stands in: round, 40 px across."
            ));
        }
        page.slider(set("scale", false, 0.01), "Scale", 1.0, 1000.0, 1.0, attr("scale", false) * 100.0, "%");
    }
    let degrees = attr("angle", false).to_degrees().rem_euclid(360.0);
    page.slider(set("angle", false, std::f32::consts::PI / 180.0), "Angle", 0.0, 360.0, 1.0, degrees, "°");
    if p.get("MaskingBrush/Enabled").is_some() {
        page.switch(
            Does::Flag("MaskingBrush/Enabled".to_owned()),
            "Masking brush",
            p.flag("MaskingBrush/Enabled", false),
        );
    }
    page.button(Does::Reset, "Back to the preset as it came");
}

fn spacing_page(p: &Preset, page: &mut Page) {
    let Some(tip) = tip_element(p) else { return };
    let auto = matches!(tip.attribute("useAutoSpacing"), Some("1" | "true"));
    page.switch(Does::AutoSpacing, "Auto spacing", auto);
    if auto {
        let coeff = tip.number("autoSpacingCoeff").unwrap_or(1.0);
        page.slider(
            Does::Attribute { child: false, name: "autoSpacingCoeff".to_owned(), per: 0.01 },
            "Spacing",
            10.0,
            1000.0,
            1.0,
            coeff * 100.0,
            "%",
        );
    } else {
        let spacing = tip.number("spacing").unwrap_or(0.1);
        page.slider(
            Does::Attribute { child: false, name: "spacing".to_owned(), per: 0.01 },
            "Spacing",
            1.0,
            1000.0,
            1.0,
            spacing * 100.0,
            "%",
        );
    }
    page.switch(Does::Flag("Spacing/Isotropic".to_owned()), "The same every way", p.flag("Spacing/Isotropic", false));
    option_rows(p, page, "Spacing", "Spacing", true);
}

fn texture_page(p: &Preset, page: &mut Page) {
    let on = p.flag("Texture/Pattern/Enabled", false);
    page.switch(Does::Flag("Texture/Pattern/Enabled".to_owned()), "Texture", on);
    if let Some(name) = p.get("Texture/Pattern/Name").or_else(|| p.get("Texture/Pattern/PatternFileName")) {
        page.text(&format!("Its pattern: {}.", name.trim()));
    }
    let k = |key: &str, otherwise: f32| p.number(&format!("Texture/Pattern/{key}"), otherwise);
    let num = |key: &str, per: f32| Does::Number { key: format!("Texture/Pattern/{key}"), per };
    page.slider(num("Scale", 0.01), "Scale", 1.0, 1000.0, 1.0, k("Scale", 1.0) * 100.0, "%");
    page.slider(num("Brightness", 0.01), "Brightness", -100.0, 100.0, 1.0, k("Brightness", 0.0) * 100.0, "%");
    page.slider(num("Contrast", 0.01), "Contrast", 0.0, 200.0, 1.0, k("Contrast", 1.0) * 100.0, "%");
    let mode = k("TexturingMode", 0.0) as u32;
    let names: Vec<&str> = TEXTURE_MODES.iter().map(|(_, n)| *n).collect();
    let at = TEXTURE_MODES.iter().position(|(m, _)| *m == mode).unwrap_or(0);
    page.choice(Does::TextureMode, "Texturing", &names, at);
    page.switch(Does::Flag("Texture/Pattern/Invert".to_owned()), "Inverted", p.flag("Texture/Pattern/Invert", false));
    page.switch(
        Does::Flag("Texture/Pattern/UseSoftTexturing".to_owned()),
        "Soft texturing",
        p.flag("Texture/Pattern/UseSoftTexturing", false),
    );
    option_rows(p, page, "Texture/Strength/", "Strength", true);
}

/// A colour smudge brush's smudge: its length option, smearing or dulling, and how it lays.
fn smudge_page(p: &Preset, page: &mut Page) {
    option_rows(p, page, "SmudgeRate", "Smudge length", true);
    page.chosen(p, "SmudgeRateMode", "Smudge mode", &["Smearing", "Dulling"], &["0", "1"], 0);
    page.flag(p, "SmudgeRateSmearAlpha", "Smear alpha", true);
    page.flag(p, "SmudgeRateUseNewEngine", "Newer smudge engine", false);
    page.flag(p, "MergedPaint", "Overlay: picks up from every layer", false);
}

/// A tangent normal brush's tilt: which way each channel goes, what turns it, how much it leans.
fn tangent_page(p: &Preset, page: &mut Page) {
    const AXES: [&str; 6] = ["X+", "X-", "Y+", "Y-", "Z+", "Z-"];
    const AXIS_VALUES: &[&str] = &["0", "1", "2", "3", "4", "5"];
    page.chosen(p, "Tangent/swizzleRed", "Red", &AXES, AXIS_VALUES, 0);
    page.chosen(p, "Tangent/swizzleGreen", "Green", &AXES, AXIS_VALUES, 2);
    page.chosen(p, "Tangent/swizzleBlue", "Blue", &AXES, AXIS_VALUES, 4);
    page.chosen(
        p,
        "Tangent/directionType",
        "Its direction from",
        &["Tilt", "Drawing angle", "Rotation", "Tilt and drawing angle"],
        &["0", "1", "2", "3"],
        0,
    );
    page.setting(p, "Tangent/elevationSensitivity", "Elevation sensitivity", (0.0, 100.0), 1.0, 100.0, "%");
    page.setting(p, "Tangent/mixValue", "Drawing angle's share", (0.0, 100.0), 1.0, 50.0, "%");
}

/// A sketch brush's lines: how far they reach out, how likely, how wide, and what they do.
fn sketch_page(p: &Preset, page: &mut Page) {
    page.setting(p, "Sketch/offset", "Offset", (0.0, 200.0), 1.0, 30.0, "%");
    page.setting(p, "Sketch/probability", "Density", (0.0, 100.0), 0.01, 0.5, "%");
    page.setting(p, "Sketch/lineWidth", "Line width", (1.0, 100.0), 1.0, 1.0, " px");
    page.flag(p, "Sketch/makeConnection", "Line from where it was", true);
    page.flag(p, "Sketch/magnetify", "Magnetify", true);
    page.flag(p, "Sketch/randomRGB", "Random colour", false);
    page.flag(p, "Sketch/randomOpacity", "Random opacity", false);
    page.flag(p, "Sketch/distanceDensity", "Fewer lines further away", true);
    page.flag(p, "Sketch/distanceOpacity", "Fainter lines further away", false);
}

/// A quick brush's own settings: its diameter and spacing.
fn quick_page(p: &Preset, page: &mut Page) {
    page.setting(p, "diameter", "Diameter", (1.0, 1000.0), 1.0, 30.0, " px");
    let auto = p.flag("useAutoSpacing", false);
    page.flag(p, "useAutoSpacing", "Auto spacing", false);
    if auto {
        page.setting(p, "autoSpacingCoeff", "Spacing", (10.0, 1000.0), 0.01, 1.0, "%");
    } else {
        page.setting(p, "spacing", "Spacing", (1.0, 1000.0), 0.01, 0.02, "%");
    }
    option_rows(p, page, "Spacing", "Spacing", true);
}

/// A spray brush's area: its size, shape, spacing and how many particles it lays, spread how.
fn spray_area_page(p: &Preset, page: &mut Page) {
    page.setting(p, "Spray/diameter", "Diameter", (1.0, 1000.0), 1.0, 100.0, " px");
    page.setting(p, "Spray/aspect", "Aspect", (1.0, 300.0), 0.01, 1.0, "%");
    page.setting(p, "Spray/rotation", "Turn", (0.0, 360.0), 1.0, 0.0, "°");
    page.setting(p, "Spray/scale", "Scale", (1.0, 1000.0), 0.01, 1.0, "%");
    page.setting(p, "Spray/spacing", "Spacing", (1.0, 1000.0), 0.01, 0.5, "%");
    page.flag(p, "Spray/jitterMovement", "Jitter", false);
    page.setting(p, "Spray/jitterMoveAmount", "Jitter amount", (0.0, 500.0), 0.01, 0.0, "%");
    page.flag(p, "Spray/useDensity", "By coverage", false);
    if p.flag("Spray/useDensity", false) {
        page.setting(p, "Spray/coverage", "Coverage", (0.0, 100.0), 1.0, 10.0, "%");
    } else {
        page.setting(p, "Spray/particleCount", "Particles", (1.0, 1000.0), 1.0, 12.0, "");
    }
    let gaussian = match p.get("Spray/radialDistributionType").map(str::trim) {
        Some("gaussian") => true,
        Some(t) if !t.is_empty() => false,
        _ => p.flag("Spray/gaussianDistribution", false),
    };
    page.choice(
        Does::Choose { key: "Spray/radialDistributionType".to_owned(), values: &["uniform", "gaussian"] },
        "Spread",
        &["Even", "Gaussian"],
        usize::from(gaussian),
    );
    if gaussian {
        page.setting(p, "Spray/radialDistributionStdDeviation", "Deviation", (1.0, 100.0), 0.01, 0.5, "%");
    }
    page.flag(p, "Spray/radialDistributionCenterBiased", "More towards the middle", true);
}

/// A spray brush's particles: their shape and size, or the brush's tip.
fn particles_page(p: &Preset, page: &mut Page) {
    let own = p.flag("SprayShape/enabled", true);
    page.flag(p, "SprayShape/enabled", "A shape of their own (off: the tip)", true);
    if !own {
        page.text("Each particle is the brush's tip (see Tip).");
        return;
    }
    page.chosen(
        p,
        "SprayShape/shape",
        "Shape",
        &["Ellipse", "Rectangle", "Smooth pixel", "Pixel"],
        &["0", "1", "2", "3"],
        0,
    );
    page.setting(p, "SprayShape/width", "Width", (1.0, 1000.0), 1.0, 6.0, " px");
    page.setting(p, "SprayShape/height", "Height", (1.0, 1000.0), 1.0, 6.0, " px");
    page.flag(p, "SprayShape/proportional", "Sized by the diameter (%)", false);
}

/// A spray brush's particles' dynamics: their size and turn, at random, fixed or following.
fn shape_dynamics_page(p: &Preset, page: &mut Page) {
    // newer presets keep them under their own name
    let newer = p.get("ShapeDynamicsVersion").is_some_and(|v| v.trim() != "2.2");
    let at = |key: &str| if newer { format!("ShapeDynamics/{key}") } else { format!("SprayShape/{key}") };
    if newer {
        page.flag(p, &at("enabled"), "Shape dynamics", false);
    }
    page.flag(p, &at("randomSize"), "Random size", false);
    page.flag(p, &at("fixedRotation"), "Fixed turn", false);
    page.setting(p, &at("fixedAngle"), "Turn", (0.0, 360.0), 1.0, 0.0, "°");
    page.flag(p, &at("randomRotation"), "Random turn", false);
    page.setting(p, &at("randomRotationWeight"), "Random turn's weight", (0.0, 100.0), 0.01, 0.5, "%");
    page.flag(p, &at("followCursor"), "Follow the pen", false);
    page.setting(p, &at("followCursorWeigth"), "Following the pen", (0.0, 100.0), 0.01, 0.5, "%");
    page.flag(p, &at("followDrawingAngle"), "Follow the drawing angle", false);
    page.setting(p, &at("followDrawingAngleWeigth"), "Following the angle", (0.0, 100.0), 0.01, 0.5, "%");
}

/// A spray brush's particles' colours.
fn spray_colour_page(p: &Preset, page: &mut Page) {
    page.flag(p, "ColorOption/useRandomHSV", "Random hue, saturation, value", false);
    page.setting(p, "ColorOption/hue", "Hue", (-180.0, 180.0), 1.0, 0.0, "°");
    page.setting(p, "ColorOption/saturation", "Saturation", (-100.0, 100.0), 1.0, 0.0, "%");
    page.setting(p, "ColorOption/value", "Value", (-100.0, 100.0), 1.0, 0.0, "%");
    page.flag(p, "ColorOption/useRandomOpacity", "Random opacity", false);
    page.flag(p, "ColorOption/colorPerParticle", "Each particle its own colour", false);
    page.flag(p, "ColorOption/mixBgColor", "Mixed with the secondary colour", false);
}

/// The hatching styles a hatching brush offers, by the flag Krita keeps each as (none the first).
const HATCH_STYLES: [(&str, &str); 5] = [
    ("Hatching/bool_nocrosshatching", "None"),
    ("Hatching/bool_perpendicular", "Perpendicular"),
    ("Hatching/bool_minusthenplus", "Minus then plus"),
    ("Hatching/bool_plusthenminus", "Plus then minus"),
    ("Hatching/bool_moirepattern", "Moiré"),
];

/// A choice of a curve for setting `key` (one of its own shown as such).
fn curve_choice(p: &Preset, page: &mut Page, key: &str, label: &str) {
    let curve = p.get(key).unwrap_or("0,0;1,1;").trim().to_owned();
    let mut names: Vec<&str> = CURVES.iter().map(|(n, _)| *n).collect();
    let at = CURVES.iter().position(|(_, c)| same_curve(c, &curve));
    let chosen = at.unwrap_or_else(|| {
        names.push("Its own");
        names.len() - 1
    });
    page.choice(Does::PlainCurve(key.to_owned()), label, &names, chosen);
}

/// A bristle brush's bristles: how far apart, sheared, shaken, how many, and how they draw.
fn bristles_page(p: &Preset, page: &mut Page) {
    page.setting(p, "HairyBristle/scale", "Spread", (1.0, 1000.0), 0.01, 2.0, "%");
    page.setting(p, "HairyBristle/shear", "Shear", (-200.0, 200.0), 0.01, 0.0, "%");
    page.setting(p, "HairyBristle/random", "Randomness", (0.0, 1000.0), 0.01, 2.0, "%");
    page.setting(p, "HairyBristle/density", "Density", (0.0, 100.0), 1.0, 100.0, "%");
    page.flag(p, "HairyBristle/threshold", "Threshold by pressure", false);
    page.flag(p, "HairyBristle/isConnected", "Connected paths", false);
    page.flag(p, "HairyBristle/antialias", "Smooth", false);
    page.flag(p, "HairyBristle/useCompositing", "Bristles over each other", false);
    page.flag(p, "HairyBristle/useMousePressure", "Mouse pressure", false);
}

/// A bristle brush's ink: whether it runs out, how fast, and what that takes away.
fn ink_page(p: &Preset, page: &mut Page) {
    page.flag(p, "HairyInk/enabled", "Ink runs out", false);
    page.setting(p, "HairyInk/inkAmount", "Ink", (1.0, 10000.0), 1.0, 1024.0, "");
    curve_choice(p, page, "HairyInk/inkDepletionCurve", "Running out");
    page.flag(p, "HairyInk/useOpacity", "Opacity falls", true);
    page.flag(p, "HairyInk/useSaturation", "Saturation falls", false);
    page.flag(p, "HairyInk/useWeights", "By weights", false);
    page.setting(p, "HairyInk/pressureWeights", "Pressure's weight", (0.0, 100.0), 1.0, 50.0, "%");
    page.setting(p, "HairyInk/bristleLengthWeights", "Bristle length's weight", (0.0, 100.0), 1.0, 50.0, "%");
    page.setting(p, "HairyInk/bristleInkAmountWeight", "Ink's weight", (0.0, 100.0), 1.0, 50.0, "%");
    page.setting(p, "HairyInk/inkDepletionWeight", "Running out's weight", (0.0, 100.0), 1.0, 50.0, "%");
    page.flag(p, "HairyInk/soak", "Soak ink", false);
}

/// A hatching brush's hatching: its lines' angle, separation and thickness, where they start, and
/// how they cross.
fn hatching_page(p: &Preset, page: &mut Page) {
    page.setting(p, "Hatching/angle", "Angle", (-90.0, 90.0), 1.0, -60.0, "°");
    page.setting(p, "Hatching/separation", "Separation", (1.0, 100.0), 1.0, 6.0, " px");
    page.setting(p, "Hatching/thickness", "Thickness", (1.0, 100.0), 1.0, 1.0, " px");
    page.setting(p, "Hatching/origin_x", "Origin across", (-500.0, 500.0), 1.0, 50.0, " px");
    page.setting(p, "Hatching/origin_y", "Origin down", (-500.0, 500.0), 1.0, 50.0, " px");
    let style = if p.flag("Hatching/bool_moirepattern", false) {
        4
    } else {
        HATCH_STYLES.iter().take(4).position(|(key, _)| p.flag(key, *key == HATCH_STYLES[0].0)).unwrap_or(0)
    };
    let names: Vec<&str> = HATCH_STYLES.iter().map(|(_, n)| *n).collect();
    page.choice(Does::HatchStyle, "Crosshatching", &names, style);
    page.setting(p, "Hatching/separationintervals", "Separation's steps", (2.0, 7.0), 1.0, 2.0, "");
    page.flag(p, "Hatching/bool_antialias", "Smooth lines", false);
    page.flag(p, "Hatching/bool_opaquebackground", "Background behind them", false);
    page.flag(p, "Hatching/bool_subpixelprecision", "Between pixels", false);
}

/// A curve brush's curves: their width, how many points they go through, how they bend.
fn curves_page(p: &Preset, page: &mut Page) {
    page.setting(p, "Curve/lineWidth", "Line width", (1.0, 100.0), 1.0, 1.0, " px");
    page.flag(p, "Curve/makeConnection", "Line from where it was", false);
    page.setting(p, "Curve/strokeHistorySize", "Points a curve goes through", (3.0, 100.0), 1.0, 30.0, "");
    page.flag(p, "Curve/smoothing", "Smoothing", false);
    page.setting(p, "Curve/curvesOpacity", "Curves' opacity", (0.0, 100.0), 0.01, 0.0, "%");
}

/// A particle brush's particles: how many, how many steps, how they are pulled.
fn particle_page(p: &Preset, page: &mut Page) {
    page.setting(p, "Particle/count", "Particles", (1.0, 2000.0), 1.0, 50.0, "");
    page.setting(p, "Particle/iterations", "Steps", (1.0, 200.0), 1.0, 10.0, "");
    page.setting(p, "Particle/gravity", "Gravity", (0.0, 100.0), 0.01, 0.989, "%");
    page.setting(p, "Particle/weight", "Weight", (0.0, 100.0), 0.01, 0.2, "%");
    page.setting(p, "Particle/scaleX", "Pull across", (-200.0, 200.0), 0.01, 0.3, "%");
    page.setting(p, "Particle/scaleY", "Pull down", (-200.0, 200.0), 0.01, 0.3, "%");
}

/// A grid brush's grid: its reach, cells, offset, division and borders, and its cells' shape.
fn grid_page(p: &Preset, page: &mut Page) {
    page.setting(p, "Grid/diameter", "Diameter", (1.0, 1000.0), 1.0, 25.0, " px");
    page.setting(p, "Grid/gridWidth", "Cell width", (1.0, 1000.0), 1.0, 25.0, " px");
    page.setting(p, "Grid/gridHeight", "Cell height", (1.0, 1000.0), 1.0, 25.0, " px");
    page.setting(p, "Grid/scale", "Scale", (1.0, 1000.0), 0.01, 1.0, "%");
    page.setting(p, "Grid/horizontalOffset", "Offset across", (-500.0, 500.0), 1.0, 0.0, " px");
    page.setting(p, "Grid/verticalOffset", "Offset down", (-500.0, 500.0), 1.0, 0.0, " px");
    page.setting(p, "Grid/divisionLevel", "Division", (0.0, 20.0), 1.0, 0.0, "");
    page.flag(p, "Grid/pressureDivision", "Division by pressure", false);
    page.setting(p, "Grid/verticalBorder", "Border across", (0.0, 100.0), 1.0, 0.0, " px");
    page.setting(p, "Grid/horizontalBorder", "Border down", (0.0, 100.0), 1.0, 0.0, " px");
    page.flag(p, "Grid/randomBorder", "Random border", false);
    page.chosen(
        p,
        "GridShape/shape",
        "Shape",
        &["Ellipse", "Rectangle", "Pixel line", "Line", "Thick line"],
        &["0", "1", "2", "3", "4"],
        0,
    );
}

/// A dyna brush's weight and what it draws.
fn dyna_page(p: &Preset, page: &mut Page) {
    page.chosen(p, "Dyna/action", "Draws", &["Circles", "A ribbon", "Its outline", "Lines"], &["0", "1", "2", "3"], 0);
    page.setting(p, "Dyna/mass", "Mass", (0.0, 100.0), 0.01, 0.5, "%");
    page.setting(p, "Dyna/drag", "Drag", (0.0, 100.0), 0.01, 0.5, "%");
    page.setting(p, "Dyna/width", "Width", (0.0, 1000.0), 0.01, 1.0, "%");
    page.setting(p, "Dyna/widthRange", "Width range", (0.0, 100.0), 0.01, 0.0, "%");
    page.setting(p, "Dyna/diameter", "Circles' diameter", (1.0, 100.0), 1.0, 6.0, " px");
    page.flag(p, "Dyna/useTwoCirles", "Two circles", false);
    page.flag(p, "Dyna/useFixedAngle", "Fixed angle", false);
    page.setting(p, "Dyna/angle", "Angle", (0.0, 360.0), 1.0, 0.0, "°");
    page.setting(p, "Dyna/lineCount", "Lines", (1.0, 200.0), 1.0, 10.0, "");
    page.setting(p, "Dyna/lineSpacing", "Lines' spacing", (0.0, 100.0), 1.0, 1.0, "");
    page.flag(p, "Dyna/enableLine", "A line along it", false);
}

/// A grid brush's cells' colours.
fn grid_colour_page(p: &Preset, page: &mut Page) {
    page.flag(p, "ColorOption/sampleInputColor", "The colour under each cell", false);
    page.flag(p, "ColorOption/fillBackground", "Background behind them", false);
    page.flag(p, "ColorOption/mixBgColor", "Mixed with the secondary colour", false);
    page.flag(p, "ColorOption/useRandomHSV", "Random hue, saturation, value", false);
    page.setting(p, "ColorOption/hue", "Hue", (-180.0, 180.0), 1.0, 0.0, "°");
    page.setting(p, "ColorOption/saturation", "Saturation", (-100.0, 100.0), 1.0, 0.0, "%");
    page.setting(p, "ColorOption/value", "Value", (-100.0, 100.0), 1.0, 0.0, "%");
    page.flag(p, "ColorOption/useRandomOpacity", "Random opacity", false);
    page.flag(p, "ColorOption/colorPerParticle", "Each cell its own colour", false);
}

/// An option's rows: whether it follows the pen (where it can be switched off), its strength, the
/// sensor it follows and its curve.
fn option_rows(p: &Preset, page: &mut Page, id: &str, label: &str, checkable: bool) {
    if checkable {
        let key = format!("Pressure{id}");
        page.switch(Does::Flag(key.clone()), &format!("{label} follows the pen"), p.flag(&key, false));
    }
    let (per, most, unit) = match id {
        "Rotation" => (1.0 / 180.0, 180.0, "°"),
        "Spacing" | "Scatter" => (0.01, 500.0, "%"),
        // older presets keep the smudge radius in hundredths
        "SmudgeRadius" if p.number("SmudgeRadiusVersion", 1.0) < 2.0 => (1.0, 300.0, "%"),
        "SmudgeRadius" => (0.01, 300.0, "%"),
        _ => (0.01, 100.0, "%"),
    };
    let otherwise = match id {
        "Spacing" | "Scatter" => 5.0,
        "SmudgeRadius" => 0.0,
        _ => 1.0,
    };
    let strength = p.number(&format!("{id}Value"), otherwise);
    page.slider(Does::Number { key: format!("{id}Value"), per }, label, 0.0, most, 1.0, strength / per, unit);
    let sensors = sensor_ids(p, id);
    let chosen = sensors.first().and_then(|s| SENSORS.iter().position(|(k, _)| k == s)).unwrap_or(0);
    let names: Vec<&str> = SENSORS.iter().map(|(_, n)| *n).collect();
    page.choice(Does::Sensor(id.to_owned()), &format!("{label} by"), &names, chosen);
    if sensors.len() > 1 {
        page.text(&format!("{label} follows {} together.", sensors.join(", ")));
    }
    let curve = p.get(&format!("{id}commonCurve")).unwrap_or("0,0;1,1;").trim().to_owned();
    let mut names: Vec<&str> = CURVES.iter().map(|(n, _)| *n).collect();
    let at = CURVES.iter().position(|(_, c)| same_curve(c, &curve));
    let chosen = at.unwrap_or_else(|| {
        names.push("Its own");
        names.len() - 1
    });
    page.choice(Does::Curve(id.to_owned()), &format!("{label} curve"), &names, chosen);
}

/// Whether two curves are the same, near enough.
fn same_curve(a: &str, b: &str) -> bool {
    match (Curve::parse(a), Curve::parse(b)) {
        (None, None) => true,
        (Some(x), Some(y)) => (0..=16).all(|i| (x.at(i as f32 / 16.0) - y.at(i as f32 / 16.0)).abs() < 0.02),
        _ => false,
    }
}

/// The sensors option `id` follows, by Krita's ids.
fn sensor_ids(p: &Preset, id: &str) -> Vec<String> {
    let Some(root) = p.element(&format!("{id}Sensor")) else { return vec!["pressure".to_owned()] };
    if root.attribute("id") == Some("sensorslist") {
        root.elements().filter_map(|e| e.attribute("id").map(str::to_owned)).collect()
    } else {
        root.attribute("id").map(|s| vec![s.to_owned()]).unwrap_or_default()
    }
}

/// `p` changed as the row that does `does` was set to `value`.
fn change(p: &mut Preset, does: &Does, value: f32) {
    let set = |p: &mut Preset, key: &str, v: String| {
        p.params.insert(key.to_owned(), v);
    };
    match does {
        Does::Number { key, per } => set(p, key, number(value * per)),
        Does::Flag(key) => set(p, key, (value >= 0.5).to_string()),
        Does::PlainCurve(key) => {
            if let Some((_, curve)) = CURVES.get(value.max(0.0) as usize) {
                set(p, key, (*curve).to_owned());
            }
        }
        Does::HatchStyle => {
            let chosen = (value.max(0.0) as usize).min(HATCH_STYLES.len() - 1);
            for (i, (key, _)) in HATCH_STYLES.iter().enumerate() {
                set(p, key, (i == chosen).to_string());
            }
        }
        Does::Choose { key, values } => {
            if let Some(v) = values.get(value.max(0.0) as usize) {
                set(p, key, (*v).to_owned());
            }
        }
        Does::Attribute { child, name, per } => {
            if let Some(mut tip) = tip_element(p) {
                let v = number(value * per);
                let target = if *child { tip.child_mut("MaskGenerator") } else { Some(&mut tip) };
                if let Some(e) = target {
                    e.set_attribute(name, &v);
                }
                set(p, "brush_definition", tip.to_xml());
            }
        }
        Does::AutoSpacing => {
            if let Some(mut tip) = tip_element(p) {
                tip.set_attribute("useAutoSpacing", if value >= 0.5 { "1" } else { "0" });
                set(p, "brush_definition", tip.to_xml());
            }
        }
        Does::Sensor(id) => {
            let Some((sensor, _)) = SENSORS.get(value.max(0.0) as usize) else { return };
            set(p, &format!("{id}Sensor"), format!(r#"<!DOCTYPE params> <params id="{sensor}"/> "#));
            set(p, &format!("{id}UseSameCurve"), "true".to_owned());
            set(p, &format!("{id}UseCurve"), "true".to_owned());
        }
        Does::Curve(id) => {
            if let Some((_, curve)) = CURVES.get(value.max(0.0) as usize) {
                set(p, &format!("{id}commonCurve"), (*curve).to_owned());
                set(p, &format!("{id}UseSameCurve"), "true".to_owned());
            }
        }
        Does::PaintingMode => set(p, "PaintOpAction", if value >= 0.5 { "2" } else { "1" }.to_owned()),
        Does::ScatterAxes => {
            let (x, y) = match value as u32 {
                1 => (true, false),
                2 => (false, true),
                _ => (true, true),
            };
            set(p, "Scattering/AxisX", x.to_string());
            set(p, "Scattering/AxisY", y.to_string());
        }
        Does::MirrorAxes => {
            let (h, v) = match value as u32 {
                1 => (false, true),
                2 => (true, true),
                _ => (true, false),
            };
            set(p, "HorizontalMirrorEnabled", h.to_string());
            set(p, "VerticalMirrorEnabled", v.to_string());
        }
        Does::TextureMode => {
            if let Some((mode, _)) = TEXTURE_MODES.get(value.max(0.0) as usize) {
                set(p, "Texture/Pattern/TexturingMode", mode.to_string());
            }
        }
        Does::Page | Does::Reset | Does::Nothing => {}
    }
}

impl Preset {
    /// Its XML again, as Krita writes a preset: its settings, and the pictures it carries.
    pub fn to_xml(&self) -> String {
        let escape = |t: &str| t.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;");
        let mut out = format!(r#"<Preset name="{}" paintopid="{}">"#, escape(&self.name), escape(&self.paintop));
        for (name, value) in &self.params {
            // a value can't hold the end of its CDATA: split it there
            let value = value.replace("]]>", "]]]]><![CDATA[>");
            out.push_str(&format!(r#"<param name="{}" type="string"><![CDATA[{value}]]></param>"#, escape(name)));
        }
        if !self.resources.is_empty() {
            out.push_str("<resources>");
            for Resource { file, name, base64 } in &self.resources {
                out.push_str(&format!(
                    r#"<resource name="{}" filename="{}"><![CDATA[{base64}]]></resource>"#,
                    escape(name),
                    escape(file),
                ));
            }
            out.push_str("</resources>");
        }
        out.push_str("</Preset>");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preset_written_again_reads_the_same() {
        let text = r#"<Preset name="A &amp; B" paintopid="paintbrush"><param name="SizeValue" type="string"><![CDATA[0.5]]></param><param name="brush_definition" type="string"><![CDATA[<Brush type="auto_brush" spacing="0.1"><MaskGenerator diameter="40"/></Brush>]]></param><resources><resource name="t" filename="t.png" type="brushes"><![CDATA[aGVsbG8=]]></resource></resources></Preset>"#;
        let p = Preset::read(text).unwrap();
        let again = Preset::read(&p.to_xml()).unwrap();
        assert_eq!(p, again);
    }

    #[test]
    fn a_change_on_a_page_reaches_the_brush_s_settings() {
        let text = r#"<Preset name="A" paintopid="paintbrush"><param name="brush_definition" type="string"><![CDATA[<Brush type="auto_brush" spacing="0.1"><MaskGenerator type="circle" id="default" diameter="40"/></Brush>]]></param></Preset>"#;
        let mut studio = Studio { preset: Preset::read(text), ..Studio::default() };
        studio.show();
        // the diameter on the tip page
        let row =
            studio.rows.iter().position(|d| matches!(d, Does::Attribute { name, .. } if name == "diameter")).unwrap();
        studio.touched(row, 120.0);
        let tip = tip_element(studio.preset.as_ref().unwrap()).unwrap();
        assert_eq!(tip.child("MaskGenerator").unwrap().number("diameter"), Some(120.0));
        // the size page: its curve
        studio.touched(0, 2.0);
        let curve = studio.rows.iter().position(|d| matches!(d, Does::Curve(_))).unwrap();
        studio.touched(curve, 5.0);
        assert_eq!(studio.preset.as_ref().unwrap().get("SizecommonCurve"), Some("0,1;1,0;"));
        // every page holds up as a panel
        for page in 0..PIXEL_PAGES.len() {
            studio.touched(0, page as f32);
            assert!(studio.rows.len() <= MOST_ROWS);
        }
    }

    /// The studio of a brush of `engine` with `params`, its pages each shown once, and the setting
    /// the first row doing `wanted` on its page `page` writes when set to `value`.
    fn set_on_page(engine: &str, params: &str, page: &str, wanted: fn(&Does) -> bool, value: f32) -> Preset {
        let text = format!(
            r#"<Preset name="E" paintopid="{engine}"><param name="brush_definition" type="string"><![CDATA[<Brush type="auto_brush" spacing="0.1"><MaskGenerator type="circle" id="default" diameter="40"/></Brush>]]></param>{params}</Preset>"#
        );
        let mut studio = Studio { preset: Preset::read(&text), ..Studio::default() };
        studio.show();
        let all = pages(engine);
        assert!(all.len() <= 8, "{engine}: a choice holds at most eight");
        for (at, (_, name)) in all.iter().enumerate() {
            studio.touched(0, at as f32);
            assert!(studio.rows.len() <= MOST_ROWS, "{engine}: {name}");
        }
        let at = all.iter().position(|(_, name)| *name == page).unwrap();
        studio.touched(0, at as f32);
        let row = studio.rows.iter().position(wanted).unwrap_or_else(|| panic!("{engine}: {page}"));
        studio.touched(row, value);
        studio.preset.unwrap()
    }

    /// Each engine's brush has its own settings on its own pages, each option with its sensor and
    /// curve as a pixel brush's.
    #[test]
    fn each_engine_s_settings_are_on_its_pages() {
        // colour smudge: dulling, and its smudge radius as older presets keep it (hundredths)
        let p = set_on_page(
            "colorsmudge",
            "",
            "Smudge",
            |d| matches!(d, Does::Choose { key, .. } if key == "SmudgeRateMode"),
            1.0,
        );
        assert_eq!(p.get("SmudgeRateMode"), Some("1"));
        let p = set_on_page(
            "colorsmudge",
            "",
            "Radius and colour",
            |d| matches!(d, Does::Number { key, .. } if key == "SmudgeRadiusValue"),
            50.0,
        );
        assert_eq!(p.number("SmudgeRadiusValue", 0.0), 50.0);
        let p = set_on_page(
            "colorsmudge",
            "",
            "Radius and colour",
            |d| matches!(d, Does::Curve(id) if id == "ColorRate"),
            5.0,
        );
        assert_eq!(p.get("ColorRatecommonCurve"), Some("0,1;1,0;"));
        // tangent normal: the red channel down the Y axis
        let p = set_on_page("colorsmudge", "", "Smudge", |d| matches!(d, Does::Flag(k) if k == "MergedPaint"), 1.0);
        assert!(p.flag("MergedPaint", false));
        let p = set_on_page(
            "tangentnormal",
            "",
            "Tangent tilt",
            |d| matches!(d, Does::Choose { key, .. } if key == "Tangent/swizzleRed"),
            3.0,
        );
        assert_eq!(p.get("Tangent/swizzleRed"), Some("3"));
        // sketch: its density, and its line width option's sensor
        let p = set_on_page(
            "sketchbrush",
            "",
            "Sketch",
            |d| matches!(d, Does::Number { key, .. } if key == "Sketch/probability"),
            80.0,
        );
        assert!((p.number("Sketch/probability", 0.0) - 0.8).abs() < 1e-5);
        let p = set_on_page("sketchbrush", "", "Lines", |d| matches!(d, Does::Sensor(id) if id == "Line width"), 1.0);
        assert!(p.get("Line widthSensor").is_some_and(|s| s.contains("speed")));
        // spray: its diameter, gaussian spread, a pixel's shape, and newer presets' dynamics
        let p = set_on_page(
            "spraybrush",
            "",
            "Spray area",
            |d| matches!(d, Does::Number { key, .. } if key == "Spray/diameter"),
            250.0,
        );
        assert_eq!(p.number("Spray/diameter", 0.0), 250.0);
        let p = set_on_page(
            "spraybrush",
            "",
            "Spray area",
            |d| matches!(d, Does::Choose { key, .. } if key == "Spray/radialDistributionType"),
            1.0,
        );
        assert_eq!(p.get("Spray/radialDistributionType"), Some("gaussian"));
        let p = set_on_page(
            "spraybrush",
            "",
            "Particles",
            |d| matches!(d, Does::Choose { key, .. } if key == "SprayShape/shape"),
            3.0,
        );
        assert_eq!(p.get("SprayShape/shape"), Some("3"));
        let newer = r#"<param name="ShapeDynamicsVersion" type="string"><![CDATA[2.3]]></param>"#;
        let p = set_on_page(
            "spraybrush",
            newer,
            "Shape dynamics",
            |d| matches!(d, Does::Flag(k) if k == "ShapeDynamics/randomSize"),
            1.0,
        );
        assert!(p.flag("ShapeDynamics/randomSize", false));
        // a bristle brush's spread, a hatching's style, a curve's points, a particle's count and a
        // grid's cells
        let p = set_on_page(
            "hairybrush",
            "",
            "Bristles",
            |d| matches!(d, Does::Number { key, .. } if key == "HairyBristle/scale"),
            500.0,
        );
        assert!((p.number("HairyBristle/scale", 0.0) - 5.0).abs() < 1e-4);
        let p = set_on_page("hairybrush", "", "Ink", |d| matches!(d, Does::PlainCurve(_)), 5.0);
        assert_eq!(p.get("HairyInk/inkDepletionCurve"), Some("0,1;1,0;"));
        let p = set_on_page("hatchingbrush", "", "Hatching", |d| *d == Does::HatchStyle, 4.0);
        assert!(p.flag("Hatching/bool_moirepattern", false) && !p.flag("Hatching/bool_nocrosshatching", true));
        let p = set_on_page(
            "hatchingbrush",
            "",
            "Separation and thickness",
            |d| matches!(d, Does::Flag(k) if k == "PressureThickness"),
            1.0,
        );
        assert!(p.flag("PressureThickness", false));
        let p = set_on_page(
            "curvebrush",
            "",
            "Curves",
            |d| matches!(d, Does::Number { key, .. } if key == "Curve/strokeHistorySize"),
            12.0,
        );
        assert_eq!(p.number("Curve/strokeHistorySize", 0.0), 12.0);
        let p = set_on_page(
            "particlebrush",
            "",
            "Particles",
            |d| matches!(d, Does::Number { key, .. } if key == "Particle/count"),
            80.0,
        );
        assert_eq!(p.number("Particle/count", 0.0), 80.0);
        let p = set_on_page(
            "gridbrush",
            "",
            "Grid",
            |d| matches!(d, Does::Choose { key, .. } if key == "GridShape/shape"),
            1.0,
        );
        assert_eq!(p.get("GridShape/shape"), Some("1"));
        // chalk's radius, dyna's mass
        let p = set_on_page(
            "chalkbrush",
            "",
            "Chalk",
            |d| matches!(d, Does::Number { key, .. } if key == "Chalk/radius"),
            8.0,
        );
        assert_eq!(p.number("Chalk/radius", 0.0), 8.0);
        let p =
            set_on_page("dynabrush", "", "Dyna", |d| matches!(d, Does::Number { key, .. } if key == "Dyna/mass"), 40.0);
        assert!((p.number("Dyna/mass", 0.0) - 0.4).abs() < 1e-5);
        // the quick brush: its diameter
        let p = set_on_page(
            "roundmarker",
            "",
            "Quick brush",
            |d| matches!(d, Does::Number { key, .. } if key == "diameter"),
            64.0,
        );
        assert_eq!(p.number("diameter", 0.0), 64.0);
    }
}
