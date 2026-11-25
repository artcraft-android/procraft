//! The app's own features, listed as plugins (see `docs/design/plugins-everything.md`). A built-in
//! plugin has a card on the Plugins page as any plugin has, with Turn on, Turn off and switches, but
//! no file: its code is the app's. Its id starts `procraft.`, which no plugin file may take, so a
//! file can never stand in for one. It can't be removed, only turned off.
//!
//! Off, its tools, panels, menu rows and Share entries are hidden; nothing an artwork already holds
//! changes, so an artwork that uses the feature opens, shows and plays as it was saved. Each part a
//! built-in hides is a [`Feature`], which the apps ask about through [`crate::Plugins::has`]: the
//! plugin itself, or one of its switches as well.
//!
//! Unlike the plugin files the app comes with, a built-in starts on: these are features the app
//! always had, so with none touched it is as it was. Its on or off and its switches are kept as any
//! plugin's are, under their keys, by the app.

use crate::Plugins;
use crate::plugins::{PluginControl, PluginInfo, PluginSource, PluginSwitch};

/// What a built-in plugin hides while it, or the switch the part comes under, is off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum Feature {
    // upstream's features, one built-in each (see `UPSTREAM`)
    /// Add text, and a text layer's Edit Text.
    Text,
    /// Models brought in to paint on, and a model's lighting.
    ModelPainting,
    /// Adjustments' Liquify.
    Liquify,
    /// A stroke held still snapped to a shape, and Gesture controls' QuickShape.
    QuickShape,
    /// Adjustments' Gradient Map and its gradient library.
    Gradients,
    /// Recolor.
    Recolor,
    /// Animation Assist and Page Assist, and the frames' bar.
    AnimationAssist,
    /// Page Assist alone.
    PageAssist,
    /// The drawing guides, symmetry among them, and a layer's Drawing Assist.
    DrawingGuides,
    /// Time-lapse recording, replay and export.
    TimeLapse,
    /// The gallery's stacks: Stack, and an artwork dropped on another.
    GalleryStacks,
    /// A layer's Clipping Mask.
    ClippingGroups,
    /// PSD files brought in and shared.
    Psd,
    /// PSD files brought in.
    PsdImport,
    /// PSD in Share.
    PsdExport,
    /// The colour panel's Palettes.
    Palettes,
    // Timeline
    /// The timeline: Actions › Canvas › Timeline and its settings, and the timeline itself.
    Timeline,
    /// Performing a move: the grid's Perform button.
    PerformedMoves,
    /// The curve editor's My curves.
    MyCurves,
    // Timeline media
    /// Sounds, videos and timing tracks on the timeline.
    TimelineMedia,
    /// Sounds brought onto the timeline.
    Sounds,
    /// Videos brought onto the timeline.
    Videos,
    /// The animatic detector, which finds a video's shots.
    AnimaticDetector,
    // HDR
    /// HDR canvases: the new-canvas choice and what edits light above white.
    Hdr,
    /// The Light tool.
    LightTool,
    /// Curves' Exposure and Limit modes.
    HdrCurves,
    /// The colour panel's HDR light row.
    HdrColour,
    /// HDR HEIC and OpenEXR in Share.
    HdrExports,
    // Effects
    /// Adjustment layers and layer styles.
    Effects,
    /// Adjustments made as a layer of their own.
    AdjustmentLayers,
    /// Layer Style in a layer's options.
    LayerStyles,
    // Vector layers
    /// Vector layers in the Layers panel's +.
    VectorLayers,
    /// Vector PDF in Share.
    VectorShare,
    /// Share's Texture Packer.
    TexturePacker,
    // Ribbon and pixel brushes
    /// The brush studio's Ribbon and Pixelated, and Pixelated in selection, transform and crop.
    BrushModes,
    RibbonBrushes,
    PixelArt,
    // the rest, one feature each
    /// The gallery's workspaces.
    Workspaces,
}

/// A built-in plugin as listed: its card and the features it holds, the first standing for the
/// plugin itself and each other for a switch on its card, in order.
struct Builtin {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    /// The feature that is the plugin itself.
    feature: Feature,
    /// Its switches: the key's last part, label and feature.
    switches: &'static [(&'static str, &'static str, Feature)],
}

const BUILTINS: &[Builtin] = &[
    Builtin {
        id: "procraft.timeline",
        name: "Timeline",
        description: "Animate on a timeline: clips and keys, motion paths, mattes, group clips and cycles, \
                      a move performed while it plays, and your own motion curves.",
        feature: Feature::Timeline,
        switches: &[
            ("perform", "Performed moves", Feature::PerformedMoves),
            ("curves", "My curves", Feature::MyCurves),
        ],
    },
    Builtin {
        id: "procraft.timeline-media",
        name: "Timeline media",
        description: "Sounds and videos on the timeline, played with it and exported with it, and the \
                      animatic detector, which cuts a video into its shots. Needs Timeline.",
        feature: Feature::TimelineMedia,
        switches: &[
            ("sounds", "Sounds", Feature::Sounds),
            ("videos", "Videos", Feature::Videos),
            ("animatic", "Animatic detector", Feature::AnimaticDetector),
        ],
    },
    Builtin {
        id: "procraft.hdr",
        name: "HDR",
        description: "Canvases that keep light brighter than white, shown on an HDR screen and exported \
                      as HDR, with the Light tool to brighten or tame it.",
        feature: Feature::Hdr,
        switches: &[
            ("light", "Light tool", Feature::LightTool),
            ("curves", "Exposure and Limit curves", Feature::HdrCurves),
            ("colour", "HDR colours", Feature::HdrColour),
            ("exports", "HDR exports", Feature::HdrExports),
        ],
    },
    Builtin {
        id: "procraft.effects",
        name: "Effects",
        description: "Adjustment layers that keep changing as the layers under them do, and layer styles: \
                      outlines, shadows and glows.",
        feature: Feature::Effects,
        switches: &[
            ("adjustment_layers", "Adjustment layers", Feature::AdjustmentLayers),
            ("styles", "Layer styles", Feature::LayerStyles),
        ],
    },
    Builtin {
        id: "procraft.vector-layers",
        name: "Vector layers",
        description: "Layers whose strokes stay sharp at any zoom and can be edited node by node, brought \
                      in from SVG files and shared as PDF.",
        feature: Feature::VectorLayers,
        switches: &[("share", "Vector PDF", Feature::VectorShare)],
    },
    Builtin {
        id: "procraft.texture-packer",
        name: "Texture Packer",
        description: "Share → Texture Packer: the canvas's separate drawings found as sprites, to pick, \
                      name and share each as a PNG of its own.",
        feature: Feature::TexturePacker,
        switches: &[],
    },
    Builtin {
        id: "procraft.brush-modes",
        name: "Ribbon and pixel brushes",
        description: "Brushes laid as one ribbon rather than stamps, and pixel-art brushes, selections and \
                      transforms that keep hard pixel edges.",
        feature: Feature::BrushModes,
        switches: &[("ribbon", "Ribbon brushes", Feature::RibbonBrushes), ("pixel", "Pixel art", Feature::PixelArt)],
    },
    Builtin {
        id: "procraft.workspaces",
        name: "Workspaces",
        description: "Galleries of their own in the gallery, each with its own artworks.",
        feature: Feature::Workspaces,
        switches: &[],
    },
];

/// Upstream ProCraft's features, listed after the fork's own. Each is a seam kept through merges from
/// upstream: the apps ask about these at the edges of upstream's code, where its ways in are.
const UPSTREAM: &[Builtin] = &[
    Builtin {
        id: "procraft.text",
        name: "Text",
        description: "Text on a layer of its own, set in any font and edited as text until rasterised.",
        feature: Feature::Text,
        switches: &[],
    },
    Builtin {
        id: "procraft.3d-painting",
        name: "3D painting",
        description: "Models brought in from OBJ and USDZ files to paint on in 3D, lit as you choose, \
                      and shared as models again.",
        feature: Feature::ModelPainting,
        switches: &[],
    },
    Builtin {
        id: "procraft.liquify",
        name: "Liquify",
        description: "Adjustments › Liquify: push, twirl, pinch and expand the layer's pixels as if wet.",
        feature: Feature::Liquify,
        switches: &[],
    },
    Builtin {
        id: "procraft.quickshape",
        name: "QuickShape",
        description: "A stroke held still at its end snaps to a line, arc, ellipse or polygon, with \
                      handles to edit it.",
        feature: Feature::QuickShape,
        switches: &[],
    },
    Builtin {
        id: "procraft.gradients",
        name: "Gradients",
        description: "Adjustments › Gradient Map, which maps the layer's tones to a gradient, and the \
                      library of gradients it maps by.",
        feature: Feature::Gradients,
        switches: &[],
    },
    Builtin {
        id: "procraft.recolor",
        name: "Recolor",
        description: "Fill a colour and the colours near it with the paint colour, wherever the reticle \
                      is moved.",
        feature: Feature::Recolor,
        switches: &[],
    },
    Builtin {
        id: "procraft.animation-assist",
        name: "Animation Assist",
        description: "Frame by frame animation, each layer a frame, with onion skins, and Page Assist, \
                      each layer a page.",
        feature: Feature::AnimationAssist,
        switches: &[("pages", "Page Assist", Feature::PageAssist)],
    },
    Builtin {
        id: "procraft.drawing-guides",
        name: "Drawing guides and symmetry",
        description: "Grid, isometric, perspective and symmetry guides over the canvas, and drawing \
                      assist, which keeps a layer's strokes to them.",
        feature: Feature::DrawingGuides,
        switches: &[],
    },
    Builtin {
        id: "procraft.time-lapse",
        name: "Time-lapse",
        description: "Every artwork's making recorded as it is painted, to replay and to share as a video.",
        feature: Feature::TimeLapse,
        switches: &[],
    },
    Builtin {
        id: "procraft.gallery-stacks",
        name: "Gallery stacks",
        description: "Artworks put together in stacks in the gallery.",
        feature: Feature::GalleryStacks,
        switches: &[],
    },
    Builtin {
        id: "procraft.clipping-groups",
        name: "Clipping groups",
        description: "A layer clipped to the one under it, showing only where that one is painted.",
        feature: Feature::ClippingGroups,
        switches: &[],
    },
    Builtin {
        id: "procraft.psd",
        name: "PSD",
        description: "Photoshop files brought in with their layers, and artworks shared as PSD.",
        feature: Feature::Psd,
        switches: &[("import", "PSD import", Feature::PsdImport), ("export", "PSD export", Feature::PsdExport)],
    },
    Builtin {
        id: "procraft.palettes",
        name: "Palettes",
        description: "The colour panel's palettes: colours kept to paint with, made, imported and shared.",
        feature: Feature::Palettes,
        switches: &[],
    },
];

/// Every built-in plugin, in the order the page lists them: the fork's, then upstream's.
fn builtins() -> impl Iterator<Item = &'static Builtin> {
    BUILTINS.iter().chain(UPSTREAM)
}

/// The key a switch of built-in `id` is kept under.
fn switch_key(id: &str, last: &str) -> String {
    format!("{id}.{last}")
}

/// Every built-in plugin, on and with its switches on, as a card shows it before anything kept is
/// read back.
pub(crate) fn listed() -> Vec<PluginInfo> {
    builtins()
        .map(|b| PluginInfo {
            id: b.id.to_owned(),
            name: b.name.to_owned(),
            description: b.description.split_whitespace().collect::<Vec<_>>().join(" "),
            source: PluginSource::Builtin,
            holds: Vec::new(),
            version: String::new(),
            publisher: "ProCraft".to_owned(),
            experimental: false,
            on: true,
            on_key: format!("{}.on", b.id),
            problem: None,
            controls: b
                .switches
                .iter()
                .map(|(last, label, _)| PluginControl::Switch {
                    control: PluginSwitch {
                        key: switch_key(b.id, last),
                        label: (*label).to_owned(),
                        on: true,
                        starts: true,
                    },
                })
                .collect(),
            brush_controls: Vec::new(),
            actions: Vec::new(),
            kinds: Vec::new(),
            layer_kinds: Vec::new(),
            permissions: Vec::new(),
            features: Vec::new(),
            top: Vec::new(),
        })
        .collect()
}

/// Whether `id` is a built-in plugin's.
pub(crate) fn is_builtin(id: &str) -> bool {
    builtins().any(|b| b.id == id)
}

/// The built-in plugin `feature` is part of, and the key of the switch it comes under if it isn't
/// the plugin itself.
pub(crate) fn place(feature: Feature) -> (&'static str, Option<String>) {
    for b in builtins() {
        if b.feature == feature {
            return (b.id, None);
        }
        if let Some((last, _, _)) = b.switches.iter().find(|(_, _, f)| *f == feature) {
            return (b.id, Some(switch_key(b.id, last)));
        }
    }
    // (every feature is listed: every_feature_has_one_place says so)
    ("", None)
}

/// Whether `feature` is shown, as `plugins` stand: its plugin on and, for a switch, the switch on.
/// A part of Timeline media needs Timeline on too.
pub(crate) fn has(plugins: &[PluginInfo], feature: Feature) -> bool {
    let (id, switch) = place(feature);
    let Some(plugin) = plugins.iter().find(|p| p.id == id) else { return false };
    let switched = switch.is_none_or(|key| {
        plugin
            .controls
            .iter()
            .any(|c| matches!(c, PluginControl::Switch { control } if control.key == key && control.on))
    });
    let needs = id != "procraft.timeline-media" || has(plugins, Feature::Timeline);
    plugin.on && switched && needs
}

/// Every feature.
pub(crate) fn all() -> Vec<Feature> {
    builtins().flat_map(|b| std::iter::once(b.feature).chain(b.switches.iter().map(|(_, _, f)| *f))).collect()
}

/// The built-in feature a QuickMenu action is a way in to, if it is one's: with that feature hidden,
/// the picker doesn't list the action and a menu's button for it does nothing.
#[uniffi::export]
pub fn quick_action_feature(action: crate::QuickAction) -> Option<Feature> {
    use crate::QuickAction as Q;
    Some(match action {
        Q::AddText => Feature::Text,
        Q::Liquify => Feature::Liquify,
        Q::Quickshape | Q::QuickshapeOn => Feature::QuickShape,
        Q::GradientMap => Feature::Gradients,
        Q::Recolor => Feature::Recolor,
        Q::AnimationAssist | Q::NextFrame | Q::PreviousFrame => Feature::AnimationAssist,
        Q::PageAssist => Feature::PageAssist,
        Q::Guide2d | Q::GuideIsometric | Q::GuidePerspective | Q::GuideSymmetry | Q::DrawingAssist => {
            Feature::DrawingGuides
        }
        _ => return None,
    })
}

/// The built-in feature an adjustment in the Adjustments menu is, if it is one's.
#[uniffi::export]
pub fn adjustment_feature(adjustment: crate::Adjustment) -> Option<Feature> {
    match adjustment {
        crate::Adjustment::Liquify => Some(Feature::Liquify),
        crate::Adjustment::GradientMap => Some(Feature::Gradients),
        _ => None,
    }
}

/// The built-in feature a row of a layer's options is a way in to, if it is one's.
#[uniffi::export]
pub fn layer_option_feature(option: crate::LayerOption) -> Option<Feature> {
    match option {
        crate::LayerOption::EditText => Some(Feature::Text),
        crate::LayerOption::ClippingMask => Some(Feature::ClippingGroups),
        crate::LayerOption::DrawingAssist => Some(Feature::DrawingGuides),
        crate::LayerOption::LayerStyle => Some(Feature::LayerStyles),
        crate::LayerOption::AdjustmentSettings => Some(Feature::AdjustmentLayers),
        _ => None,
    }
}

/// The built-in feature a page of Gesture controls is, if it is one's.
#[uniffi::export]
pub fn gesture_purpose_feature(purpose: crate::GesturePurpose) -> Option<Feature> {
    match purpose {
        crate::GesturePurpose::Quickshape => Some(Feature::QuickShape),
        crate::GesturePurpose::Assisted => Some(Feature::DrawingGuides),
        _ => None,
    }
}

#[uniffi::export]
impl Plugins {
    /// Every one of the app's own features hidden now: the rest of [`Plugins::features`].
    pub fn hidden_features(&self) -> Vec<Feature> {
        let shown = self.features();
        all().into_iter().filter(|f| !shown.contains(f)).collect()
    }

    /// `settings` as the built-in plugins leave them: no QuickShape from a stroke held still or a
    /// gesture while QuickShape is off, and no gesture's assisted drawing while the drawing guides
    /// are. The apps hand the canvas these, again each time the plugins change.
    pub fn touch_settings_shown(&self, settings: crate::TouchSettings) -> crate::TouchSettings {
        use crate::GestureAction as G;
        let hidden = |a: Option<G>| match a {
            Some(G::QuickShape) => !self.has(Feature::QuickShape),
            Some(G::Assisted) => !self.has(Feature::DrawingGuides),
            _ => false,
        };
        let shown = |a: Option<G>| if hidden(a) { None } else { a };
        crate::TouchSettings {
            draw_and_hold: settings.draw_and_hold && self.has(Feature::QuickShape),
            touch: shown(settings.touch),
            pen: shown(settings.pen),
            hold: shown(settings.hold),
            modify_touch: shown(settings.modify_touch),
            modify_pen: shown(settings.modify_pen),
            four_tap: shown(settings.four_tap),
            three_swipe: shown(settings.three_swipe),
            pen_button: shown(settings.pen_button),
            pen_second_button: shown(settings.pen_second_button),
            ..settings
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Plugins;
    use std::sync::Arc;

    fn app(name: &str) -> Arc<Plugins> {
        let dir = std::env::temp_dir().join(format!("procraft-builtins-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Plugins::with_bundled(dir.to_string_lossy().into_owned())
    }

    #[test]
    fn every_feature_shows_until_its_plugin_is_turned_off() {
        let plugins = app("start");
        assert_eq!(plugins.features(), all());
        assert!(plugins.set_on("procraft.hdr".into(), false));
        for feature in [Feature::Hdr, Feature::LightTool, Feature::HdrCurves, Feature::HdrColour, Feature::HdrExports] {
            assert!(!plugins.has(feature), "{feature:?}");
        }
        assert!(plugins.has(Feature::Timeline) && plugins.has(Feature::Workspaces));
    }

    #[test]
    fn a_switch_off_hides_its_part_alone() {
        let plugins = app("switch");
        assert!(plugins.set_switch("procraft.hdr.exports".into(), false));
        assert!(!plugins.has(Feature::HdrExports));
        assert!(plugins.has(Feature::Hdr) && plugins.has(Feature::LightTool) && plugins.has(Feature::HdrCurves));
    }

    #[test]
    fn timeline_media_needs_the_timeline() {
        let plugins = app("media");
        assert!(plugins.set_on("procraft.timeline".into(), false));
        assert!(!plugins.has(Feature::TimelineMedia) && !plugins.has(Feature::Sounds));
        assert!(plugins.plugins().iter().any(|p| p.id == "procraft.timeline-media" && p.on));
    }

    #[test]
    fn what_was_kept_is_read_back() {
        let plugins = app("kept");
        plugins.read_kept("procraft.workspaces.on".into(), "false".into());
        plugins.read_kept("procraft.timeline.perform".into(), "false".into());
        assert!(!plugins.has(Feature::Workspaces));
        assert!(plugins.has(Feature::Timeline) && !plugins.has(Feature::PerformedMoves));
    }

    #[test]
    fn built_ins_come_first_and_cannot_be_removed() {
        let plugins = app("page");
        let listed = plugins.plugins();
        let builtins = listed.iter().take_while(|p| p.source == crate::PluginSource::Builtin).count();
        assert_eq!(builtins, BUILTINS.len() + UPSTREAM.len());
        assert!(listed.len() > builtins, "the bundled files come after");
        assert!(plugins.remove("procraft.timeline".into()).is_none());
        assert!(plugins.plugins().iter().any(|p| p.id == "procraft.timeline"));
    }

    #[test]
    fn every_feature_has_one_place() {
        let features = all();
        let unique: std::collections::HashSet<_> = features.iter().collect();
        assert_eq!(unique.len(), features.len());
        for b in builtins() {
            assert!(b.id.starts_with("procraft."), "{}", b.id);
        }
    }

    #[test]
    fn upstreams_features_come_after_the_forks_and_start_on() {
        let plugins = app("upstream");
        let ids: Vec<String> = plugins.plugins().into_iter().map(|p| p.id).collect();
        let first = ids.iter().position(|id| id == "procraft.text").unwrap();
        assert_eq!(&ids[first..first + UPSTREAM.len()], UPSTREAM.iter().map(|b| b.id).collect::<Vec<_>>());
        assert!(ids[..first].iter().all(|id| BUILTINS.iter().any(|b| b.id == id)));
        for feature in [Feature::Text, Feature::Psd, Feature::PsdImport, Feature::Palettes, Feature::PageAssist] {
            assert!(plugins.has(feature), "{feature:?}");
        }
    }

    #[test]
    fn psd_import_and_export_switch_apart() {
        let plugins = app("psd");
        assert!(plugins.set_switch("procraft.psd.export".into(), false));
        assert!(plugins.has(Feature::PsdImport) && !plugins.has(Feature::PsdExport));
        assert!(plugins.set_on("procraft.psd".into(), false));
        assert!(!plugins.has(Feature::PsdImport) && !plugins.has(Feature::Psd));
        assert_eq!(plugins.hidden_features(), [Feature::Psd, Feature::PsdImport, Feature::PsdExport]);
        assert!(plugins.has(Feature::Text), "the others stay");
    }

    #[test]
    fn page_assist_goes_with_animation_assist() {
        let plugins = app("assist");
        assert!(plugins.set_switch("procraft.animation-assist.pages".into(), false));
        assert!(plugins.has(Feature::AnimationAssist) && !plugins.has(Feature::PageAssist));
        plugins.read_kept("procraft.animation-assist.pages".into(), "true".into());
        assert!(plugins.set_on("procraft.animation-assist".into(), false));
        assert!(!plugins.has(Feature::AnimationAssist) && !plugins.has(Feature::PageAssist));
    }

    #[test]
    fn ways_in_name_their_features() {
        use crate::{Adjustment, GesturePurpose, LayerOption, QuickAction};
        assert_eq!(quick_action_feature(QuickAction::AddText), Some(Feature::Text));
        assert_eq!(quick_action_feature(QuickAction::GuideSymmetry), Some(Feature::DrawingGuides));
        assert_eq!(quick_action_feature(QuickAction::QuickshapeOn), Some(Feature::QuickShape));
        assert_eq!(quick_action_feature(QuickAction::Undo), None);
        assert_eq!(adjustment_feature(Adjustment::GradientMap), Some(Feature::Gradients));
        assert_eq!(adjustment_feature(Adjustment::Curves), None);
        assert_eq!(layer_option_feature(LayerOption::ClippingMask), Some(Feature::ClippingGroups));
        assert_eq!(layer_option_feature(LayerOption::Rename), None);
        assert_eq!(gesture_purpose_feature(GesturePurpose::Quickshape), Some(Feature::QuickShape));
        // each adjustment a QuickMenu action opens is that action's feature too
        for action in crate::quick_actions_listed().into_iter().flatten().chain(crate::quick_adjustments_listed()) {
            if let Some(a) = crate::quick_action_adjustment(action) {
                assert_eq!(adjustment_feature(a), quick_action_feature(action), "{action:?}");
            }
        }
    }

    #[test]
    fn quickshape_off_takes_it_from_the_touches() {
        let plugins = app("touches");
        let settings = crate::TouchSettings {
            draw_and_hold: true,
            hold: Some(crate::GestureAction::QuickShape),
            pen: Some(crate::GestureAction::Assisted),
            touch: Some(crate::GestureAction::Eyedropper),
            ..crate::TouchSettings::default()
        };
        assert_eq!(plugins.touch_settings_shown(settings.clone()), settings);
        assert!(plugins.set_on("procraft.quickshape".into(), false));
        assert!(plugins.set_on("procraft.drawing-guides".into(), false));
        let shown = plugins.touch_settings_shown(settings);
        assert!(!shown.draw_and_hold && shown.hold.is_none() && shown.pen.is_none());
        assert_eq!(shown.touch, Some(crate::GestureAction::Eyedropper));
    }
}
