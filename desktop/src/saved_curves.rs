//! "My curves", as the iPad app's (TimelineCurves.swift, `SavedCurves`): motion curves kept by
//! name. One is an easing as the easing editor made it (its kind, that kind's numbers, a drawn
//! curve's points, how often it repeats, looping or backwards), kept by the app in its folder
//! (`timeline-curves`, a curve a line) and so there in every artwork, to put on other keys, rename,
//! duplicate, replace or delete.
//!
//! The later key rows list them in a key's Easing menu with [`easing_choices`].

// (opened from a key's rows, which come later: until then only its tests reach all of it)
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use procraft_core::{EasingKind, EasingSpec};
use procraft_engine::timeline::Easing;

/// The kinds by the names they are kept under (the iPad's).
const KINDS: [(EasingKind, &str); 11] = [
    (EasingKind::Linear, "linear"),
    (EasingKind::Hold, "hold"),
    (EasingKind::Smooth, "smooth"),
    (EasingKind::CubicBezier, "cubicBezier"),
    (EasingKind::Curve, "curve"),
    (EasingKind::Bounce, "bounce"),
    (EasingKind::Elastic, "elastic"),
    (EasingKind::Steps, "steps"),
    (EasingKind::ElasticSteps, "elasticSteps"),
    (EasingKind::Cyclic, "cyclic"),
    (EasingKind::Random, "random"),
];

/// A curve kept by name.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedCurve {
    /// Which it is while the app runs (not kept).
    pub id: u64,
    pub name: String,
    pub spec: EasingSpec,
}

impl SavedCurve {
    /// The engine's easing it is, to put on a key.
    pub fn easing(&self) -> Easing {
        self.spec.easing()
    }

    /// As a line of its file: kind, repeats, looping, backwards, its numbers, its name (tabs
    /// between, none in the name).
    fn line(&self) -> String {
        let kind = KINDS.iter().find(|k| k.0 == self.spec.kind).map_or("linear", |k| k.1);
        let numbers: Vec<String> = self.spec.params.iter().map(f32::to_string).collect();
        let name: String =
            self.name.chars().map(|c| if c == '\t' || c == '\n' || c == '\r' { ' ' } else { c }).collect();
        format!(
            "{kind}\t{}\t{}\t{}\t{}\t{name}",
            self.spec.repeats.max(1),
            u8::from(self.spec.looping),
            u8::from(self.spec.reversed),
            numbers.join(" ")
        )
    }

    /// A line of its file read back; none for one that isn't a curve.
    fn read(line: &str, id: u64) -> Option<SavedCurve> {
        let mut parts = line.splitn(6, '\t');
        let kind = parts.next()?;
        let kind = KINDS.iter().find(|k| k.1 == kind).map(|k| k.0)?;
        let repeats = parts.next()?.parse::<u32>().ok()?.max(1);
        let looping = parts.next()? == "1";
        let reversed = parts.next()? == "1";
        let params: Option<Vec<f32>> = parts.next()?.split_whitespace().map(|n| n.parse::<f32>().ok()).collect();
        let name = parts.next()?.trim().to_owned();
        let spec = EasingSpec { kind, params: params?, repeats, looping, reversed };
        (!name.is_empty()).then_some(SavedCurve { id, name, spec })
    }
}

/// The curves kept, in the order they were made (a copy just after what it is of).
#[derive(Debug, Default)]
pub struct SavedCurves {
    all: Vec<SavedCurve>,
    /// Where they are kept (none: only while the app runs).
    path: Option<PathBuf>,
    next_id: u64,
}

impl SavedCurves {
    /// The app's own, kept in its folder.
    pub fn open() -> SavedCurves {
        SavedCurves::open_at(&crate::files::app_dir().join("timeline-curves"))
    }

    /// Those kept at `path` (none yet if it isn't there), kept there as they change.
    pub fn open_at(path: &Path) -> SavedCurves {
        let mut curves = SavedCurves { all: Vec::new(), path: Some(path.to_owned()), next_id: 0 };
        let kept = std::fs::read_to_string(path).unwrap_or_default();
        for line in kept.lines() {
            if let Some(curve) = SavedCurve::read(line, curves.next_id) {
                curves.all.push(curve);
                curves.next_id += 1;
            }
        }
        curves
    }

    pub fn all(&self) -> &[SavedCurve] {
        &self.all
    }

    pub fn get(&self, id: u64) -> Option<&SavedCurve> {
        self.all.iter().find(|c| c.id == id)
    }

    fn keep(&self) {
        let Some(path) = &self.path else { return };
        let text: String = self.all.iter().map(|c| c.line() + "\n").collect();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // written whole beside it, then put in its place
        let beside = path.with_extension("new");
        if std::fs::write(&beside, text).is_ok() && std::fs::rename(&beside, path).is_err() {
            let _ = std::fs::remove_file(&beside);
        }
    }

    fn made(&mut self, name: String, spec: EasingSpec) -> SavedCurve {
        let id = self.next_id;
        self.next_id += 1;
        SavedCurve { id, name, spec }
    }

    /// `base`, or `base` with the first number after it that no curve has.
    fn free(&self, base: &str, numbered: bool) -> String {
        let taken = |name: &str| self.all.iter().any(|c| c.name == name);
        if !numbered && !taken(base) {
            return base.to_owned();
        }
        let mut n = if numbered { 1 } else { 2 };
        while taken(&format!("{base} {n}")) {
            n += 1;
        }
        format!("{base} {n}")
    }

    /// The name a new curve is offered: "Curve 1" and so on.
    pub fn next_name(&self) -> String {
        self.free("Curve", true)
    }

    /// `spec` kept as a new curve named `name` (the next free name for one of only spaces); its
    /// id.
    pub fn add(&mut self, spec: EasingSpec, name: &str) -> u64 {
        let name = name.trim();
        let name = if name.is_empty() { self.next_name() } else { name.to_owned() };
        let curve = self.made(name, spec);
        let id = curve.id;
        self.all.push(curve);
        self.keep();
        id
    }

    /// Nothing for a name of only spaces.
    pub fn rename(&mut self, id: u64, name: &str) {
        let name = name.trim();
        let Some(curve) = self.all.iter_mut().find(|c| c.id == id) else { return };
        if name.is_empty() {
            return;
        }
        curve.name = name.to_owned();
        self.keep();
    }

    /// A copy just after it ("… copy"); its id.
    pub fn duplicate(&mut self, id: u64) -> Option<u64> {
        let i = self.all.iter().position(|c| c.id == id)?;
        let of = self.all.get(i)?.clone();
        let copy = self.made(self.free(&format!("{} copy", of.name), false), of.spec);
        let made = copy.id;
        self.all.insert(i + 1, copy);
        self.keep();
        Some(made)
    }

    /// Curve `id` becomes `spec`, its name kept.
    pub fn replace(&mut self, id: u64, spec: EasingSpec) {
        let Some(curve) = self.all.iter_mut().find(|c| c.id == id) else { return };
        curve.spec = spec;
        self.keep();
    }

    pub fn remove(&mut self, id: u64) {
        self.all.retain(|c| c.id != id);
        self.keep();
    }
}

/// A kept curve as a key's Easing menu offers it: its name, the easing it puts on the key, and
/// whether it is the one the key has.
#[derive(Clone, Debug, PartialEq)]
pub struct EasingChoice {
    pub name: String,
    pub easing: Easing,
    pub chosen: bool,
}

/// The kept curves for a key's Easing menu (under the named easings), the one `now` is marked.
pub fn easing_choices(curves: &SavedCurves, now: &Easing) -> Vec<EasingChoice> {
    let now = EasingSpec::of(now);
    curves
        .all()
        .iter()
        .map(|c| EasingChoice { name: c.name.clone(), easing: c.easing(), chosen: c.spec == now })
        .collect()
}

#[cfg(test)]
mod tests {
    use procraft_core::default_easing;

    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("procraft-curves-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// Kept curves are there again when read back (as the app is opened again): their names,
    /// order and easings, a drawn curve's points and a repeated, looping, backwards one's too;
    /// rename, duplicate, replace and delete are kept as well.
    #[test]
    fn my_curves_survive_being_saved_and_read_back() {
        let path = folder("kept").join("timeline-curves");
        let mut curves = SavedCurves::open_at(&path);
        assert!(curves.all().is_empty());
        let drawn =
            EasingSpec { params: vec![0.0, 0.0, 0.3, 1.25, 0.7, -0.1, 1.0, 1.0], ..default_easing(EasingKind::Curve) };
        let bounce = EasingSpec { repeats: 3, looping: true, reversed: true, ..default_easing(EasingKind::Bounce) };
        assert_eq!(curves.next_name(), "Curve 1");
        let a = curves.add(drawn.clone(), "  ");
        assert_eq!(curves.get(a).map(|c| c.name.as_str()), Some("Curve 1"), "a blank name is the next free one");
        let b = curves.add(bounce.clone(), "Bouncy\tball");
        assert_eq!(curves.next_name(), "Curve 2");
        let copy = curves.duplicate(a).unwrap();
        assert_eq!(curves.get(copy).map(|c| c.name.as_str()), Some("Curve 1 copy"));
        let again = curves.duplicate(a).unwrap();
        assert_eq!(curves.get(again).map(|c| c.name.as_str()), Some("Curve 1 copy 2"));
        curves.rename(again, "   ");
        assert_eq!(curves.get(again).map(|c| c.name.as_str()), Some("Curve 1 copy 2"), "no blank names");
        curves.rename(again, "Wobble");
        let ease = default_easing(EasingKind::CubicBezier);
        curves.replace(copy, ease.clone());
        curves.remove(b);

        let read = SavedCurves::open_at(&path);
        let shown: Vec<(&str, &EasingSpec)> = read.all().iter().map(|c| (c.name.as_str(), &c.spec)).collect();
        assert_eq!(
            shown,
            vec![("Curve 1", &drawn), ("Wobble", &drawn), ("Curve 1 copy", &ease)],
            "copies after theirs"
        );

        let mut read = read;
        read.add(bounce.clone(), "Bouncy\tball");
        let last = SavedCurves::open_at(&path);
        let kept = last.all().last().unwrap();
        assert_eq!(kept.name, "Bouncy ball", "a tab in a name is a space");
        assert_eq!(kept.spec, bounce, "repeats, looping and backwards kept");
        assert_eq!(kept.easing(), bounce.easing());
        // a key's Easing menu lists them, the key's own marked
        let choices = easing_choices(&last, &drawn.easing());
        assert_eq!(
            choices.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            ["Curve 1", "Wobble", "Curve 1 copy", "Bouncy ball"]
        );
        assert_eq!(choices.iter().map(|c| c.chosen).collect::<Vec<_>>(), [true, true, false, false]);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    /// A file with lines that aren't curves keeps the ones that are.
    #[test]
    fn lines_that_are_not_curves_are_passed_over() {
        let path = folder("odd").join("timeline-curves");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "nonsense\nbounce\t1\t0\t0\t0.3 0.8\tKept\nbounce\t1\t0\t0\t0.3 x\tBad\nlinear\t1\t0\t0\t\t\n",
        )
        .unwrap();
        let curves = SavedCurves::open_at(&path);
        assert_eq!(curves.all().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Kept"]);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
