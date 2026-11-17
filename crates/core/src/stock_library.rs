//! The brushes the app comes with: the stock sets a new library starts with, made from the settings
//! `stock_brushes.rs` keeps (each brush its own variant, its shapes and grains drawn by our own
//! stand-ins), then ours from before, which a library holds in one set after them (see
//! [`crate::brush_library::LibrarySet::shipped`]).

use std::sync::{Arc, LazyLock};

use procraft_engine::brush::{Brush, FLOW_POWER, Notches, PressureCurve};
use procraft_engine::brushfile::{BrushFile, StandIn, Texture};
use procraft_engine::curve::Curve;
use procraft_engine::geom::Vec2;
use procraft_engine::library::{BrushSet, SHELF, library};

/// How many sets of ours from before ship first: kept libraries know their brushes by these places,
/// so the stock sets come after them.
pub(crate) const LEGACY_SETS: usize = SHELF.len();

/// The set a library holds ours from before in.
pub(crate) const LEGACY_SET: &str = "Legacy";

/// A stock set as `stock_brushes.rs` keeps it.
pub(crate) struct StockSet {
    pub(crate) name: &'static str,
    pub(crate) brushes: &'static [StockBrush],
}

/// A stock brush as `stock_brushes.rs` keeps it: what a brush file would hold of it (see
/// [`BrushFile`]), its controls as they differ from [`crate::stock_brushes::BASE`] (by their place
/// in [`crate::stock_brushes::KEYS`], in that order) and those it doesn't set, its curves as points,
/// and its shape, grain and materials as our stand-ins.
pub(crate) struct StockBrush {
    pub(crate) name: &'static str,
    pub(crate) settings: &'static [(u8, f32)],
    pub(crate) unset: &'static [u8],
    pub(crate) max_size: f32,
    pub(crate) min_size: Option<f32>,
    pub(crate) shape_angle: f32,
    pub(crate) pressure_curve: Option<&'static [[f32; 2]]>,
    pub(crate) size_curve: Option<&'static [[f32; 2]]>,
    pub(crate) opacity_curve: Option<&'static [[f32; 2]]>,
    pub(crate) curves: &'static [(PressureCurve, &'static [[f32; 2]])],
    pub(crate) shape: Option<StandIn>,
    pub(crate) grain: Option<StandIn>,
    pub(crate) metallic: Option<StandIn>,
    pub(crate) roughness: Option<StandIn>,
    pub(crate) sliders: [Option<[f32; 2]>; 3],
    pub(crate) second: Option<&'static StockBrush>,
}

impl StockBrush {
    /// The brush as a brush file holding it would give it.
    fn file(&self) -> BrushFile {
        let points = |p: &[[f32; 2]]| p.iter().map(|&[x, y]| Vec2::new(x, y)).collect::<Vec<_>>();
        let look = |s: Option<StandIn>| s.map_or(Texture::None, Texture::Drawn);
        // every control in the shared order, at the brush's own value or the shared one
        let settings = crate::stock_brushes::KEYS
            .iter()
            .zip(crate::stock_brushes::BASE)
            .enumerate()
            .filter(|&(k, _)| !self.unset.iter().any(|&u| usize::from(u) == k))
            .map(|(k, (key, base))| {
                let own = self.settings.iter().find(|&&(o, _)| usize::from(o) == k);
                ((*key).to_owned(), own.map_or(base, |&(_, v)| v))
            })
            .collect();
        BrushFile {
            name: self.name.to_owned(),
            author: String::new(),
            settings,
            max_size: self.max_size,
            min_size: self.min_size,
            shape_angle: self.shape_angle,
            pressure_curve: self.pressure_curve.map(points),
            size_curve: self.size_curve.map(points),
            opacity_curve: self.opacity_curve.map(points),
            pressure_curves: self.curves.iter().map(|&(kind, p)| (kind, points(p))).collect(),
            shape: look(self.shape),
            grain: look(self.grain),
            metallic: look(self.metallic),
            roughness: look(self.roughness),
            sliders: self.sliders,
            notches: Notches::default(),
            thumbnail: None,
            author_picture: None,
            signature: None,
            secondary: self.second.map(|s| Box::new(s.file())),
            engine: None,
        }
    }
}

/// The stock sets, each brush made as its file would be read: its stand-ins shared with every other
/// brush drawing the same, and drawn only once a brush wants them (most never are).
fn stock_sets() -> Vec<BrushSet> {
    crate::stock_brushes::SETS
        .iter()
        .map(|set| BrushSet {
            name: set.name,
            brushes: set
                .brushes
                .iter()
                .map(|b| {
                    let mut brush = crate::brush_files::brush_from(&b.file());
                    compensate(&mut brush);
                    brush
                })
                .collect(),
        })
        .collect()
}

/// The stock brushes are tuned for the old pressure law; the shared one boosts past whole now.
/// Curves over the press turn it back into the old one, as the earlier brushes' retune does, and
/// the preview's swell reaches the pressure it used to. Their spacing and grain controls already
/// go through `set_setting` as the old model read them, so those stay as they are.
fn compensate(b: &mut Brush) {
    // the size the preview draws at: the least holds a share of it up, which the straight multiply
    // would drop, so the curve carries that share too (away from this size it drifts, as the
    // earlier brushes' held spacing does)
    let d = procraft_engine::preview::diameter(b);
    let r = (b.min_size.min(b.biggest()).min(d) / d.max(1e-3)).clamp(0.0, 1.0);
    if b.pressure_size != 0.0 {
        let keep = if b.pressure_size > 0.0 { r } else { 0.0 };
        // sampled through the curve's own read, so a bend it holds level stays held
        let pts: Vec<Vec2> = (0..=32)
            .map(|i| {
                let x = i as f32 / 32.0;
                Vec2::new(x, (keep + (1.0 - r) * b.size_curve.eval(x)) * 0.25)
            })
            .collect();
        b.size_curve = Curve::new(&pts);
    }
    // the opacity used to ease within the flow, through the opacity range; the new law lays it
    // straight outside the range. At the preview's flow the curve can carry the whole difference:
    // what the range let through at each pressure, over what it lets through without the pressure
    let a = b.pressure_opacity;
    if a != 0.0 {
        let c = b.flow.clamp(0.0, 1.0).powf(1.0 / FLOW_POWER);
        let ranged =
            |pa: f32| (b.min_opacity + (b.max_opacity - b.min_opacity) * c * pa).clamp(0.01, 1.0).powf(FLOW_POWER);
        let bare = (b.min_opacity + (b.max_opacity - b.min_opacity) * c).clamp(0.01, 1.0).powf(FLOW_POWER);
        let laid = |x: f32| {
            let pa =
                if a > 0.0 { 1.0 - a * (1.0 - b.opacity_curve.eval(x)) } else { 1.0 + a * b.opacity_curve.eval(x) };
            ranged(pa) / bare
        };
        // the curve spans what the range lays from its least to its most (a negative amount falls
        // the other way, so the least is at the far end of the press)
        let samples: Vec<f32> = (0..=32).map(|i| laid(i as f32 / 32.0)).collect();
        let lo = samples.iter().cloned().fold(f32::MAX, f32::min);
        let amount = 1.0 - lo;
        if bare > 1e-4 {
            if amount > 1e-4 {
                let pts: Vec<Vec2> = samples
                    .iter()
                    .enumerate()
                    .map(|(i, &l)| Vec2::new(i as f32 / 32.0, (l - lo) / (4.0 * amount)))
                    .collect();
                b.pressure_opacity = amount;
                b.opacity_curve = Curve::new(&pts);
            } else {
                // the range lays full whatever the pressure: the opacity response does nothing
                b.pressure_opacity = 0.0;
            }
        }
    }
    // the flow reads the pressure neutral at a quarter press now; these brushes drew it straight
    // off the curve. A curve a quarter as tall turns the neutral read back into the straight one,
    // as the opacity and size above keep their old reads
    if b.setting("pressure_flow") > 0.0 {
        let before = b.pressure_curves.get(PressureCurve::Flow).cloned();
        let pts: Vec<Vec2> = (0..=32)
            .map(|i| {
                let x = i as f32 / 32.0;
                Vec2::new(x, before.as_ref().map_or(x, |c| c.eval(x)) * 0.25)
            })
            .collect();
        b.pressure_curves.set(PressureCurve::Flow, Some(Curve::new(&pts)));
    }
    // the preview's swell reaches the pressure it used to: the shared gain is a quarter of what it
    // was, so the brush's own scale goes in fourfold (past the control's clamp, straight into extra)
    let scale = b.setting("preview_pressure_scale");
    b.extra.insert("preview_pressure_scale".to_owned(), scale * 4.0);
    if let Some(second) = b.secondary.as_deref_mut() {
        compensate(second);
    }
}

static SHIPPED: LazyLock<Arc<Vec<BrushSet>>> = LazyLock::new(|| {
    let started = std::time::Instant::now();
    let mut sets = library();
    sets.extend(stock_sets());
    log::info!("the stock brushes made in {} ms", started.elapsed().as_millis());
    Arc::new(sets)
});

/// The brushes the app comes with, made once: ours from before (at the places kept libraries know
/// them by), then the stock sets.
pub fn shipped() -> Arc<Vec<BrushSet>> {
    Arc::clone(&SHIPPED)
}

/// Makes the brushes the app comes with ahead of the first artwork, off the main thread.
pub(crate) fn prepare() {
    LazyLock::force(&SHIPPED);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ours_from_before_ship_first_at_their_places_and_the_stock_sets_after() {
        let sets = shipped();
        let ours = library();
        assert_eq!(ours.len(), LEGACY_SETS);
        for (s, set) in ours.iter().enumerate() {
            assert_eq!(sets[s].name, set.name);
            assert_eq!(sets[s].brushes, set.brushes, "{}", set.name);
        }
        let stock = &sets[LEGACY_SETS..];
        assert_eq!(stock.len(), crate::stock_brushes::SETS.len());
        assert!(stock.iter().all(|set| !set.brushes.is_empty()));
        // no two sets of the same name, nor two brushes in a set
        let mut names: Vec<&str> = stock.iter().map(|s| s.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), stock.len());
        for set in stock {
            let mut brushes: Vec<&str> = set.brushes.iter().map(|b| b.name.as_str()).collect();
            brushes.sort_unstable();
            brushes.dedup();
            assert_eq!(brushes.len(), set.brushes.len(), "{}", set.name);
        }
    }

    #[test]
    fn the_stock_brushes_keep_the_old_size_response() {
        // the curves turn the boosting law back into the old one over the whole press, as the
        // brush's own file would have drawn it before the law changed
        let sets = shipped();
        for (s, set) in crate::stock_brushes::SETS.iter().enumerate() {
            for (i, sb) in set.brushes.iter().enumerate() {
                let raw = crate::brush_files::brush_from(&sb.file());
                let b = &sets[LEGACY_SETS + s].brushes[i];
                let d = procraft_engine::preview::diameter(&raw);
                let least = raw.min_size.min(raw.biggest()).min(d);
                for p in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
                    let pressed = raw.pressure_curve.eval(p);
                    let (rs, ro) = (raw.size_curve.eval(pressed), raw.opacity_curve.eval(pressed));
                    let (a, ao) = (raw.pressure_size, raw.pressure_opacity);
                    let want_s = if a >= 0.0 { 1.0 - a * (1.0 - rs) } else { 1.0 + a * rs };
                    let want_o = if ao >= 0.0 { 1.0 - ao * (1.0 - ro) } else { 1.0 + ao * ro };
                    let (got_s, got_o) = b.pressure_factors(p);
                    // the drawn size is what must hold: the least's share rides in the curve now
                    let want_px = least + (d - least) * want_s;
                    let got_px = d * got_s;
                    assert!(
                        (got_px - want_px).abs() < 0.002 * d.max(1.0),
                        "{} size at {p}: {got_px} vs {want_px}",
                        b.name
                    );
                    // the opacity's ease moved from the range into the factor: what the stamp lays
                    // at the preview's flow is what must hold
                    let c = raw.flow.clamp(0.0, 1.0).powf(1.0 / FLOW_POWER);
                    let (m, most) = (raw.min_opacity, raw.max_opacity);
                    let want_a = (m + (most - m) * c * want_o).clamp(0.01, 1.0).powf(FLOW_POWER);
                    let got_a = (m + (most - m) * c).clamp(0.01, 1.0).powf(FLOW_POWER) * got_o;
                    assert!((got_a - want_a).abs() < 2e-2, "{} alpha at {p}: {got_a} vs {want_a}", b.name);
                }
            }
        }
    }

    #[test]
    fn stock_brushes_carry_no_preview_maker_or_names_of_images() {
        for brush in shipped()[LEGACY_SETS..].iter().flat_map(|s| &s.brushes) {
            for b in std::iter::once(brush).chain(brush.secondary.as_deref()) {
                assert!(b.thumbnail.is_none() && b.author.is_none() && b.engine.is_none(), "{}", b.name);
                assert_eq!(b.stock, Default::default(), "{}", b.name);
            }
        }
    }

    #[test]
    fn brushes_standing_in_for_the_same_image_share_its_pixels() {
        let sets = shipped();
        let mut by_id: std::collections::HashMap<(u64, Option<u64>), &procraft_engine::brush::Pixels> =
            std::collections::HashMap::new();
        let mut shared = 0;
        for b in sets[LEGACY_SETS..].iter().flat_map(|s| &s.brushes) {
            for source in [&b.shape.source, &b.grain.source].into_iter().flatten() {
                match by_id.get(&(source.id, source.stand_in_for)) {
                    Some(first) => {
                        assert!(procraft_engine::brush::Pixels::same(first, &source.pixels), "{}", b.name);
                        shared += 1;
                    }
                    None => {
                        by_id.insert((source.id, source.stand_in_for), &source.pixels);
                    }
                }
            }
        }
        assert!(shared > 0);
    }

    /// The controls taken as they are, each a switch or a choice: every other control a brush file
    /// gives is a value the brushes' variants move.
    const CHOICES: &[&str] = &[
        "spacing_version",
        "relative_to_stroke",
        "grain_behavior",
        "cursor_outline",
        "hover_fill",
        "pen_taper_link",
        "pen_tip_animation",
        "touch_taper_link",
        "randomized",
        "flip_x",
        "flip_y",
        "grain_offset_jitter",
        "grain_follows_camera",
        "luminance_blending",
        "alpha_threshold",
        "classic_combine",
        "jitter_lateral_tilt",
        "jitter_linear_tilt",
        "count_tilt",
        "grain_depth_tilt",
        "stamp_hue_tilt",
        "stamp_saturation_tilt",
        "stamp_lightness_tilt",
        "stamp_darkness_tilt",
        "stamp_secondary_tilt",
        "attack_tilt",
        "attack_roll",
        "pressure_size_clamped",
        "orient_to_screen",
        "preview_stamp",
        "preview_wet_mix",
        "preview_tilt",
        "count",
        "input_style",
        "rendering_mode",
        "classic_taper",
        "blend_mode",
        "grain_blend",
        "burnt_blend",
        "grain_turn",
        "dual_blend",
        "shape_filtering",
        "grain_filtering",
    ];

    fn fnv(text: &str) -> u64 {
        text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
    }

    /// How much brush `name` moves its control `key`: a share of its value, 1% to 3% either way
    /// (2.95%, so that kept to four figures it stays within 3%), the same every time.
    fn moved(name: &str, key: &str) -> f32 {
        // splitmix64 of the two names' hashes
        let mut z = (fnv(name) ^ fnv(key).rotate_left(17)).wrapping_add(0x9e37_79b9_7f4a_7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        let share = 0.01 + 0.0195 * ((z >> 40) as f32 / (1u64 << 24) as f32);
        if z & 1 == 0 { share } else { -share }
    }

    /// `value` moved by `by` (a share of it) to four figures, kept inside `low..high` (moved the
    /// other way where it would leave them); none moves at none or at either end, where moving would
    /// change what it does.
    fn shifted(value: f32, by: f32, (low, high): (f32, f32)) -> f32 {
        let inside = |v: f32| v > low + 1e-6 && v < high - 1e-6;
        if value.abs() < 1e-6 || !inside(value) {
            return value;
        }
        let figures = |v: f32| {
            let scale = 10f32.powi(3 - v.abs().log10().floor() as i32);
            (v * scale).round() / scale
        };
        [value * (1.0 + by), value * (1.0 - by)].into_iter().map(figures).find(|&v| inside(v)).unwrap_or(value)
    }

    /// The top of a brush file's most size (its units, see `brushfile::set_sizes`).
    const FILE_MOST_SIZE: f32 = 4.0;

    /// The range of control `key`, as a brush keeps it.
    fn range(key: &str) -> (f32, f32) {
        let mut b = procraft_engine::brush::Brush::round_pen();
        b.set_setting(key, -1000.0);
        let low = b.setting(key);
        b.set_setting(key, 1000.0);
        (low, b.setting(key))
    }

    /// `file` as brush `name` of ours: each value moved (see [`moved`], as `seed` says), its
    /// switches, choices and curves as they are, and none of the file's own pictures, preview or
    /// maker; a dual brush's second by the same name, moved as its own.
    fn variant(mut file: BrushFile, name: &str, seed: &str) -> BrushFile {
        for (key, value) in &mut file.settings {
            if CHOICES.contains(&key.as_str()) {
                continue;
            }
            *value = shifted(*value, moved(seed, key), range(key));
        }
        // (the most size at the top of a brush file's own range stays there: a brush that big draws
        // its preview differently, see `preview::size_share`)
        file.max_size = shifted(file.max_size, moved(seed, "max_size"), (0.0, FILE_MOST_SIZE));
        file.min_size = file.min_size.map(|m| shifted(m, moved(seed, "min_size"), (0.0, f32::MAX)));
        file.shape_angle = shifted(file.shape_angle, moved(seed, "shape_angle"), (-1000.0, 1000.0));
        file.name = name.to_owned();
        file.author.clear();
        (file.thumbnail, file.author_picture, file.signature, file.engine) = (None, None, None, None);
        file.notches = Notches::default();
        let second = format!("{seed}\u{1}second");
        file.secondary = file.secondary.map(|s| Box::new(variant(*s, name, &second)));
        file
    }

    /// Our stand-ins for a file's images: the stock images it names, and any it carries by the name
    /// it carries it under (whose measurements the stock tables keep too).
    fn stand_ins(file: &mut BrushFile, carried: &[(String, Vec<u8>)]) -> [Option<StandIn>; 4] {
        let named = |t: &Texture| match t {
            Texture::None | Texture::Drawn(_) => None,
            Texture::Stock { name, inverted } => Some((name.clone(), *inverted)),
            Texture::Image { bytes, inverted } => {
                let (name, _) = carried.iter().find(|(_, b)| b == bytes).expect("a carried image's name");
                Some((name.clone(), *inverted))
            }
        };
        let preview = file.thumbnail.clone();
        let shape = named(&file.shape).map(|(n, inv)| crate::brush_files::shape_stand_in(&n, inv));
        let grain = named(&file.grain).map(|(n, inv)| crate::brush_files::grain_stand_in(&n, inv, preview.as_deref()));
        let material = |t: &Texture| {
            named(t).map(|(n, inv)| StandIn {
                pick: procraft_engine::brushfile::Pick::Ours(crate::brush_files::material_stand_in(&n)),
                key: procraft_engine::textures::stock_key(&n),
                inverted: inv,
            })
        };
        let looks = [shape, grain, material(&file.metallic), material(&file.roughness)];
        for t in [&mut file.shape, &mut file.grain, &mut file.metallic, &mut file.roughness] {
            *t = Texture::None;
        }
        looks
    }

    fn points(p: &[Vec2]) -> String {
        if p == [Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0)] {
            return "L".to_owned();
        }
        let pts: Vec<String> = p.iter().map(|v| format!("[{:?}, {:?}]", v.x, v.y)).collect();
        format!("&[{}]", pts.join(", "))
    }

    fn look(s: &Option<StandIn>) -> String {
        use procraft_engine::brushfile::Pick;
        let Some(s) = s else { return "None".to_owned() };
        let pick = match s.pick {
            Pick::Round(h) => format!("Round({h:?})"),
            Pick::Blank => "Blank".to_owned(),
            Pick::Ours(None) => "Ours(None)".to_owned(),
            Pick::Ours(Some(i)) => format!("Ours(Some({i}))"),
            Pick::Halftone([(a, b), (c, d)]) => format!("Halftone([({a:?}, {b:?}), ({c:?}, {d:?})])"),
        };
        format!("Some(S {{ pick: {pick}, key: {:#018x}, inverted: {} }})", s.key, s.inverted)
    }

    /// One brush as `stock_brushes.rs` writes it: its controls as they differ from `base` (each
    /// control's value in most brushes, in the order `keys` sets them), and those it doesn't set.
    fn written(
        file: &BrushFile,
        looks: &[Option<StandIn>; 4],
        second: Option<String>,
        keys: &[String],
        base: &[f32],
    ) -> String {
        // its controls in the shared order, leaving out those it doesn't set
        let set: Vec<&str> = file.settings.iter().map(|(k, _)| k.as_str()).collect();
        let unset: Vec<usize> = (0..keys.len()).filter(|&k| !set.contains(&keys[k].as_str())).collect();
        let order: Vec<&str> = (0..keys.len()).filter(|k| !unset.contains(k)).map(|k| keys[k].as_str()).collect();
        assert_eq!(order, set, "{}: its controls in the shared order", file.name);
        let mut settings = Vec::new();
        for (key, value) in &file.settings {
            let k = keys.iter().position(|x| x == key).unwrap();
            if value.to_bits() != base[k].to_bits() {
                settings.push(format!("({k}, {value:?})"));
            }
        }
        let unset: Vec<String> = unset.iter().map(usize::to_string).collect();
        let curve = |c: &Option<Vec<Vec2>>| c.as_deref().map_or("None".to_owned(), |p| format!("Some({})", points(p)));
        let curves: Vec<String> =
            file.pressure_curves.iter().map(|(k, p)| format!("(C::{k:?}, {})", points(p))).collect();
        let slider = |s: &Option<[f32; 2]>| s.map_or("None".to_owned(), |[a, b]| format!("Some([{a:?}, {b:?}])"));
        let [shape, grain, metallic, roughness] = looks;
        format!(
            "B {{ name: {:?},\n    settings: &[{}], unset: &[{}],\n    max_size: {:?}, min_size: {}, shape_angle: {:?},\n    pressure_curve: {}, size_curve: {}, opacity_curve: {},\n    curves: &[{}],\n    shape: {}, grain: {},\n    metallic: {}, roughness: {},\n    sliders: [{}], second: {} }}",
            file.name,
            settings.join(", "),
            unset.join(", "),
            file.max_size,
            file.min_size.map_or("None".to_owned(), |m| format!("Some({m:?})")),
            file.shape_angle,
            curve(&file.pressure_curve),
            curve(&file.size_curve),
            curve(&file.opacity_curve),
            curves.join(", "),
            look(shape),
            look(grain),
            look(metallic),
            look(roughness),
            file.sliders.iter().map(slider).collect::<Vec<_>>().join(", "),
            second.map_or("None".to_owned(), |s| format!("Some(&{s})")),
        )
    }

    /// Writes `stock_brushes.rs` from a folder of brush sets (`STOCK_SETS`: a folder for each
    /// library, one in it for each set, holding its brush files) and a table of what to call them
    /// (`STOCK_NAMES`, tab-separated: kind, library, set, the brush's name in its file and ours, a
    /// row for each library, set and brush in the order they go in). Each brush is its own variant
    /// (see [`variant`]), its images our stand-ins. `cargo test -p procraft-core --lib
    /// write_stock_brushes -- --ignored`.
    #[test]
    #[ignore]
    fn write_stock_brushes() {
        let (Ok(from), Ok(table)) = (std::env::var("STOCK_SETS"), std::env::var("STOCK_NAMES")) else { return };
        let from = std::path::PathBuf::from(from);
        let table = std::fs::read_to_string(table).unwrap();
        // each set's name and its brushes: the brush, its stand-ins, and its second's
        type Looks = [Option<StandIn>; 4];
        type Made = (BrushFile, Looks, Option<Looks>);
        let mut sets: Vec<(String, Vec<Made>)> = Vec::new();
        // how many values each variant moved, and how many it kept (switches and choices; none and
        // either end), and the most it moved one
        let (mut values_moved, mut kept, mut most) = (0usize, 0usize, 0f32);
        // each set's brushes, read once, by the name their files give them
        // a brush file read, and the images it carries by their names
        type Read = (BrushFile, Vec<(String, Vec<u8>)>);
        let mut folder: Option<(std::path::PathBuf, Vec<Read>)> = None;
        for row in table.lines().skip(1) {
            let cols: Vec<&str> = row.split('\t').collect();
            let [kind, library, set, original, ours] = cols[..] else { panic!("row {row:?}") };
            match kind {
                "library" => {}
                "set" => {
                    let dir = from.join(library).join(set);
                    let mut files = Vec::new();
                    for entry in std::fs::read_dir(&dir).unwrap() {
                        let path = entry.unwrap().path();
                        if path.extension().is_none_or(|e| e != "brush") {
                            continue;
                        }
                        let bytes = std::fs::read(&path).unwrap();
                        let zip = procraft_engine::zip::ZipReader::new(&bytes).unwrap();
                        let carried: Vec<(String, Vec<u8>)> = zip
                            .names()
                            .filter(|n| n.ends_with(".png") || n.ends_with(".jpg"))
                            .map(|n| (n.rsplit('/').next().unwrap_or(n).to_owned(), zip.read(n).unwrap()))
                            .collect();
                        files.push((procraft_engine::brushfile::read(&bytes).unwrap(), carried));
                    }
                    folder = Some((dir, files));
                    sets.push((ours.to_owned(), Vec::new()));
                }
                "brush" => {
                    let (dir, files) = folder.as_ref().unwrap();
                    let (file, carried) = files.iter().find(|(f, _)| f.name == original).unwrap_or_else(|| {
                        panic!("{original} in {}", dir.display());
                    });
                    // the stand-ins first, as its file has them (an unknown grain goes by the
                    // file's own preview), then its variant
                    let mut brush = file.clone();
                    let looks = stand_ins(&mut brush, carried);
                    let second_looks = brush.secondary.as_deref_mut().map(|second| stand_ins(second, carried));
                    let moved = variant(brush.clone(), ours, ours);
                    let pairs = std::iter::once((&brush, &moved))
                        .chain(brush.secondary.as_deref().zip(moved.secondary.as_deref()));
                    for (was, now) in pairs {
                        for ((_, a), (_, b)) in was.settings.iter().zip(&now.settings) {
                            if a == b {
                                kept += 1;
                            } else {
                                values_moved += 1;
                                most = most.max(((b - a) / a).abs());
                            }
                        }
                    }
                    sets.last_mut().unwrap().1.push((moved, looks, second_looks));
                }
                _ => panic!("kind {kind}"),
            }
        }
        // the controls in the order the brushes set them (the longest list: the others leave some
        // out), each with its value in most brushes
        let files = || {
            sets.iter()
                .flat_map(|(_, b)| b)
                .flat_map(|(file, _, _)| std::iter::once(file).chain(file.secondary.as_deref()))
        };
        let longest = files().max_by_key(|f| f.settings.len()).unwrap();
        let keys: Vec<String> = longest.settings.iter().map(|(k, _)| k.clone()).collect();
        let base: Vec<f32> = keys
            .iter()
            .map(|key| {
                let mut counts: Vec<(u32, u32)> = Vec::new();
                for (_, v) in files().flat_map(|f| f.settings.iter().filter(|(k, _)| k == key)) {
                    match counts.iter_mut().find(|(bits, _)| *bits == v.to_bits()) {
                        Some((_, n)) => *n += 1,
                        None => counts.push((v.to_bits(), 1)),
                    }
                }
                counts.iter().max_by_key(|(_, n)| *n).map_or(0.0, |&(bits, _)| f32::from_bits(bits))
            })
            .collect();
        let mut out = String::from(
            "//! The stock brushes, written by `write_stock_brushes` in `stock_library.rs` (don't edit by hand):\n\
             //! each set's brushes, their controls as the brush studio's, their curves, and the stand-ins of\n\
             //! ours their shapes, grains and materials are drawn with (see `brushfile::StandIn`).\n\n\
             #![cfg_attr(rustfmt, rustfmt_skip)]\n\
             // (measured values, some of which only look like constants)\n\
             #![allow(clippy::approx_constant)]\n\n\
             use procraft_engine::brush::PressureCurve as C;\n\
             use procraft_engine::brushfile::Pick::*;\n\
             use procraft_engine::brushfile::StandIn as S;\n\n\
             use crate::stock_library::{StockBrush as B, StockSet};\n\n\
             /// A straight curve.\nconst L: &[[f32; 2]] = &[[0.0, 0.0], [1.0, 1.0]];\n\n",
        );
        out += &format!(
            "/// The controls the brushes set, in the order they set them.\npub(crate) const KEYS: [&str; {}] = [{}];\n\n",
            keys.len(),
            keys.iter().map(|k| format!("{k:?}")).collect::<Vec<_>>().join(", ")
        );
        out += &format!(
            "/// Each control's value in most brushes (a brush keeps the others it has).\npub(crate) const BASE: [f32; {}] = [{}];\n\n",
            base.len(),
            base.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>().join(", ")
        );
        out += "/// The stock sets, in the order a new library shows them.\npub(crate) const SETS: &[StockSet] = &[\n";
        for (name, brushes) in &sets {
            out += &format!("StockSet {{ name: {name:?}, brushes: &[\n");
            for (file, looks, second_looks) in brushes {
                let second = file
                    .secondary
                    .as_deref()
                    .zip(second_looks.as_ref())
                    .map(|(second, looks)| written(second, looks, None, &keys, &base));
                out += &written(file, looks, second, &keys, &base);
                out += ",\n";
            }
            out += "] },\n";
        }
        out += "];\n";
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/stock_brushes.rs");
        std::fs::write(path, out).unwrap();
        let count: usize = sets.iter().map(|(_, b)| b.len()).sum();
        let out_len = std::fs::metadata(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/stock_brushes.rs"))
            .map_or(0, |m| m.len());
        eprintln!("{out_len} bytes");
        eprintln!("{} sets, {count} brushes, {} controls", sets.len(), keys.len());
        eprintln!("{values_moved} values moved (at most {:.2}%), {kept} kept", most * 100.0);
    }

    #[test]
    fn a_variant_moves_values_by_one_to_three_hundredths_and_keeps_ends_switches_and_none() {
        for (name, key) in [("Austen", "spacing"), ("Austen", "jitter_size"), ("Charming", "spacing")] {
            let by = moved(name, key);
            assert!((0.01..=0.03).contains(&by.abs()), "{name} {key} {by}");
            assert_eq!(by, moved(name, key), "the same every time");
        }
        assert_ne!(moved("Austen", "spacing"), moved("Charming", "spacing"));
        assert_eq!(shifted(0.0, 0.03, (0.0, 1.0)), 0.0);
        assert_eq!(shifted(1.0, -0.03, (0.0, 1.0)), 1.0);
        assert_eq!(shifted(-1.0, 0.03, (-1.0, 1.0)), -1.0);
        assert!((shifted(0.5, 0.02, (0.0, 1.0)) - 0.51).abs() < 1e-6);
        // past the end it moves the other way
        assert!((shifted(0.99, 0.02, (0.0, 1.0)) - 0.9702).abs() < 1e-6);
        assert!(CHOICES.iter().all(|k| !k.is_empty()));
    }
}
