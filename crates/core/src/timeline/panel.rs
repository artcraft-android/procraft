//! What the timeline panel shows, read in one call: the timeline's settings, its rows of tracks
//! and clips, every clip's keys, the camera's and each sound's volume keys, the markers, the
//! clipboard, the looped frames, the onion skins, and a version of each drawing's picture. Each
//! reading is numbered: a reading the same as the last is given the same number, and the panel,
//! asking with the number it has, is told nothing has changed rather than sent it all again.

use make2d_engine::document::{Document, LayerId, LayerKind};

use super::{
    TimelineCamera, TimelineClipKind, TimelineInfo, TimelineKeyInfo,
    clipboard::TimelineClipboardInfo,
    groups_inline::TimelineRow,
    key_lanes::{TimelineKeyLane, TimelineLaneKey},
    menus::{TimelineLoop, TimelineOnion},
};
use crate::CanvasSession;

/// A clip's keys, in frame order.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TimelineClipKeys {
    pub id: u64,
    pub keys: Vec<TimelineKeyInfo>,
}

/// A sound's volume keys, in frame order.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TimelineVolumeKeys {
    pub id: u64,
    pub keys: Vec<TimelineLaneKey>,
}

/// A drawing's picture as it now is: `version` changes whenever what its picture shows may have
/// (its pixels, or for a group, which of its layers show and how strongly).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TimelinePicture {
    pub root: u64,
    pub version: u64,
}

/// Everything the panel reads after a change, as `timeline_info`, `timeline_rows`,
/// `timeline_keys`, `timeline_camera`, `timeline_camera_keys`, `timeline_markers`,
/// `timeline_clipboard`, `timeline_loop`, `timeline_onion` and `timeline_lane_keys` give it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TimelinePanel {
    /// The reading's number: the same while nothing in it changes.
    pub generation: u64,
    /// None: the canvas isn't on a timeline (everything else is then empty).
    pub info: Option<TimelineInfo>,
    pub rows: Vec<TimelineRow>,
    /// The keys of each clip in the rows that has some, in the rows' order.
    pub keys: Vec<TimelineClipKeys>,
    pub camera: Option<TimelineCamera>,
    pub camera_keys: Vec<u32>,
    pub markers: Vec<u32>,
    pub clipboard: TimelineClipboardInfo,
    pub loop_frames: Option<TimelineLoop>,
    pub onion: Option<TimelineOnion>,
    /// The camera's lane: its keys with their easings.
    pub camera_lane: Vec<TimelineLaneKey>,
    /// The volume keys of each sound in the rows that has some.
    pub volumes: Vec<TimelineVolumeKeys>,
    /// Each drawing in the rows (once each), its picture's version.
    pub pictures: Vec<TimelinePicture>,
}

/// The last reading given, kept to number the next.
#[derive(Default)]
pub(crate) struct PanelMemo {
    last: Option<TimelinePanel>,
}

#[uniffi::export]
impl CanvasSession {
    /// The panel's reading; none if it is the reading numbered `known` (nothing has changed since
    /// the panel read it).
    pub fn timeline_panel(&self, known: u64) -> Option<TimelinePanel> {
        let mut panel = self.timeline_panel_read();
        let mut guard = self.shared.lock();
        let memo = &mut guard.timeline_panel;
        let last = memo.last.as_ref().map_or(0, |l| l.generation);
        panel.generation = last;
        if memo.last.as_ref() != Some(&panel) {
            panel.generation = last + 1;
            memo.last = Some(panel.clone());
        }
        (panel.generation != known).then_some(panel)
    }
}

impl CanvasSession {
    /// The reading, unnumbered, from the calls the panel made one at a time before.
    fn timeline_panel_read(&self) -> TimelinePanel {
        let info = self.timeline_info();
        let mut panel = TimelinePanel {
            generation: 0,
            info: None,
            rows: Vec::new(),
            keys: Vec::new(),
            camera: None,
            camera_keys: Vec::new(),
            markers: Vec::new(),
            clipboard: self.timeline_clipboard(),
            loop_frames: None,
            onion: None,
            camera_lane: Vec::new(),
            volumes: Vec::new(),
            pictures: Vec::new(),
        };
        if info.is_none() {
            return panel;
        }
        panel.info = info;
        panel.rows = self.timeline_rows();
        let mut seen = std::collections::HashSet::new();
        let mut roots = Vec::new();
        for c in panel.rows.iter().flat_map(|r| &r.track.clips) {
            if !seen.insert(c.id) {
                continue;
            }
            let keys = self.timeline_keys(c.id);
            if !keys.is_empty() {
                panel.keys.push(TimelineClipKeys { id: c.id, keys });
            }
            if c.kind == TimelineClipKind::Audio {
                let keys = self.timeline_lane_keys(TimelineKeyLane::Volume { id: c.id });
                if !keys.is_empty() {
                    panel.volumes.push(TimelineVolumeKeys { id: c.id, keys });
                }
            }
            if c.root != 0 && !roots.contains(&c.root) {
                roots.push(c.root);
            }
        }
        panel.camera = self.timeline_camera();
        panel.camera_keys = self.timeline_camera_keys();
        panel.markers = self.timeline_markers();
        panel.loop_frames = self.timeline_loop();
        panel.onion = self.timeline_onion();
        panel.camera_lane = self.timeline_lane_keys(TimelineKeyLane::Camera);
        // strokes still being put down count in the versions
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.flush();
        panel.pictures = roots
            .into_iter()
            .map(|root| TimelinePicture {
                root,
                version: picture_version(
                    &st.doc,
                    |l| st.canvas.as_ref().map_or(0, |c| c.layer_version(l)),
                    LayerId(root),
                ),
            })
            .collect();
        panel
    }
}

/// A number that changes whenever the picture `frame_thumbnail` draws of layer `id` may: made from
/// the canvas's size, and the version, opacity and showing of the layer (for a group, of each of
/// its layers).
fn picture_version(doc: &Document, version: impl Fn(LayerId) -> u64, id: LayerId) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (doc.width(), doc.height()).hash(&mut h);
    let mut add = |l: LayerId| {
        let Some(layer) = doc.layer(l) else { return };
        (l.0, layer.kind == LayerKind::Group, layer.visible, layer.opacity.to_bits(), version(l)).hash(&mut h);
    };
    add(id);
    let mut open = vec![id];
    while let Some(g) = open.pop() {
        if doc.layer(g).is_some_and(|l| l.kind == LayerKind::Group) {
            for m in doc.members(Some(g)) {
                add(m);
                open.push(m);
            }
        }
    }
    h.finish()
}
