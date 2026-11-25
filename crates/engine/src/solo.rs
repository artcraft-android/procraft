//! A track shown alone: what it shows at a frame as a layer of its own would, through the mattes
//! of the tracks below it, for exports that keep each track apart (a plugin's, through
//! `procraft-core`'s `plugin_exports.rs`).

use crate::document::BlendMode;
use crate::timeline::{MaskMode, Timeline, TrackId};

/// The tracks whose clips mask what track `track` shows at `frame` of `tl`, nearest first: the
/// clip on the track below it, if it masks with that, then whatever masks that clip, and so on.
pub fn matte_sources(tl: &Timeline, track: TrackId, frame: u32) -> Vec<TrackId> {
    let items = tl.plan(frame).items;
    let mut out = Vec::new();
    let mut at = items.iter().position(|i| i.track == track);
    while let Some((_, source)) = at.and_then(|i| items.get(i)).and_then(|i| i.mask) {
        let Some(item) = items.get(source) else { break };
        out.push(item.track);
        at = Some(source);
    }
    out
}

/// `tl` changed to show what track `track` shows at `frame` as a layer of its own: every other
/// track hidden but those that mask it there (see [`matte_sources`]), which only mask (a clipping
/// mask's source, drawn too on the timeline, is left to its own layer), and its clip there
/// blending normally (its mode is what its layer is given apart).
pub fn solo(tl: &mut Timeline, track: TrackId, frame: u32) {
    let sources = matte_sources(tl, track, frame);
    for t in &mut tl.tracks {
        let mine = t.id == track;
        t.hidden = !mine && !sources.contains(&t.id);
        if t.hidden {
            continue;
        }
        // (a track's own mask and blend mode are those of its clips that have none)
        let look = t.look;
        if mine {
            t.look.blend = BlendMode::Normal;
        }
        let Some(clip) = t.clips.iter_mut().find(|c| c.covers(frame)) else { continue };
        // the same matte, its source no longer drawn
        let mask = if clip.mask == MaskMode::None { look.mask } else { clip.mask };
        if mask == MaskMode::Clipping {
            clip.mask = MaskMode::Alpha;
        }
        if mine {
            clip.blend = BlendMode::Normal;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::LayerId;
    use crate::timeline::{Clip, ClipContent, ClipId, Drawing, Track};

    fn track(id: u64, clips: Vec<Clip>) -> Track {
        Track {
            id: TrackId(id),
            name: format!("T{id}"),
            hidden: false,
            locked: false,
            opacity: 1.0,
            look: Default::default(),
            clips,
        }
    }

    fn drawing(id: u64, start: u32, duration: u32, mask: MaskMode) -> Clip {
        Clip {
            id: ClipId(id),
            name: "d".into(),
            start,
            duration,
            opacity: 1.0,
            blend: BlendMode::Normal,
            mask,
            adjustment: false,
            disabled: false,
            label: None,
            keys: Default::default(),
            content: ClipContent::Drawing(Drawing { root: LayerId(id) }),
        }
    }

    /// Three tracks, bottom first: a matte, a clip masked by it on frames 0 to 3, and one on top.
    fn matted(mode: MaskMode) -> Timeline {
        let mut tl = Timeline::new(12, 8);
        tl.tracks = vec![
            track(1, vec![drawing(1, 0, 8, MaskMode::None)]),
            track(2, vec![drawing(2, 0, 4, mode), drawing(3, 4, 4, MaskMode::None)]),
            track(3, vec![drawing(4, 0, 8, MaskMode::None)]),
        ];
        tl.next_track = 4;
        tl.next_clip = 5;
        tl
    }

    #[test]
    fn a_track_alone_keeps_its_matte_as_a_matte() {
        let mut tl = matted(MaskMode::Luminance);
        assert_eq!(matte_sources(&tl, TrackId(2), 1), [TrackId(1)]);
        assert!(matte_sources(&tl, TrackId(2), 5).is_empty());
        assert!(matte_sources(&tl, TrackId(3), 1).is_empty());
        tl.tracks[1].clips[0].blend = BlendMode::Screen;
        solo(&mut tl, TrackId(2), 1);
        let hidden: Vec<bool> = tl.tracks.iter().map(|t| t.hidden).collect();
        assert_eq!(hidden, [false, false, true]);
        let plan = tl.plan(1);
        assert_eq!(plan.items.len(), 2);
        // the matte only masks; the clip draws through it, normally
        assert!(plan.items[0].matte_only);
        assert_eq!(plan.items[1].mask, Some((MaskMode::Luminance, 0)));
        assert_eq!(plan.items[1].blend, BlendMode::Normal);
        assert_eq!(plan.drawings(), [LayerId(2)]);

        // where nothing masks it, it is alone
        let mut tl = matted(MaskMode::Alpha);
        solo(&mut tl, TrackId(2), 5);
        let hidden: Vec<bool> = tl.tracks.iter().map(|t| t.hidden).collect();
        assert_eq!(hidden, [true, false, true]);
        assert_eq!(tl.plan(5).drawings(), [LayerId(3)]);
    }

    #[test]
    fn a_clipping_mask_alone_leaves_its_source_to_its_own_layer() {
        let mut tl = matted(MaskMode::Clipping);
        // on the timeline the source draws too
        assert_eq!(tl.plan(1).drawings(), [LayerId(1), LayerId(2), LayerId(4)]);
        solo(&mut tl, TrackId(2), 1);
        let plan = tl.plan(1);
        assert!(plan.items[0].matte_only);
        assert_eq!(plan.items[1].mask, Some((MaskMode::Alpha, 0)));
        assert_eq!(plan.drawings(), [LayerId(2)]);
    }

    #[test]
    fn a_matte_of_a_matte_comes_along_too() {
        let mut tl = matted(MaskMode::Alpha);
        tl.tracks[2].clips[0].mask = MaskMode::Clipping;
        assert_eq!(matte_sources(&tl, TrackId(3), 2), [TrackId(2), TrackId(1)]);
        solo(&mut tl, TrackId(3), 2);
        assert!(tl.tracks.iter().all(|t| !t.hidden));
        let plan = tl.plan(2);
        assert!(plan.items[0].matte_only && plan.items[1].matte_only);
        assert_eq!(plan.drawings(), [LayerId(4)]);
    }
}
