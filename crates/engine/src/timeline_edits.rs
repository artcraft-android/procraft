//! Group clips and media clips, edited further: where the groups a clip is in put it (its motion
//! path drawn where it shows), clips moved into and out of groups, ungrouping that keeps what the
//! group did (its keys folded into its clips, its cycle laid out), drawings added to as the layers
//! panel's + does, and sounds and videos split and duplicated.

use std::collections::HashMap;

use super::*;

/// The most of a group's frames [`Timeline::group_shots`] looks at.
pub const MOST_GROUP_SHOT_FRAMES: u32 = 4000;

/// Something a group shows on a frame: a drawing's layer, or a video at a time in its file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GroupShown {
    Drawing(LayerId),
    Video { media: MediaId, seconds: f64 },
}

impl GroupShown {
    /// The same drawing, or the same video whatever the time in it.
    fn same(&self, other: &GroupShown) -> bool {
        match (self, other) {
            (GroupShown::Drawing(a), GroupShown::Drawing(b)) => a == b,
            (GroupShown::Video { media: a, .. }, GroupShown::Video { media: b, .. }) => a == b,
            _ => false,
        }
    }
}

impl Timeline {
    /// How the group clips clip `id` is inside move, scale and turn it on its own frame `frame`
    /// (in their first cycles): their poses, the innermost's first. None if none of them is
    /// posed, or it is in none.
    pub fn groups_pose(&self, id: ClipId, frame: f32) -> Option<Affine> {
        let mut path = Vec::new();
        if !clip_path(&self.tracks, id, &mut path) {
            return None;
        }
        let clip = path.pop()?;
        // the frame on the level of the group it is directly in
        let mut at = clip.start as f32 + frame;
        let mut pose: Option<Affine> = None;
        for g in path.iter().rev() {
            let keyed = match &g.content {
                ClipContent::Group(Group { cycle: Some(c), .. }) if c.keys == CycleKeys::Repeat => {
                    at.rem_euclid(c.length.max(1) as f32)
                }
                _ => at,
            };
            if let Some(p) = g.keys.pose_at(keyed) {
                pose = Some(pose.map_or(p, |q| q.then(p)));
            }
            at += g.start as f32;
        }
        pose
    }

    /// The group clips holding track `id` grow to cover their clips, innermost first (a group
    /// that cycles stays as it is; one stops at the clip after it on its track).
    pub(crate) fn cover(&mut self, id: TrackId) {
        let mut track = id;
        while let Some(group) = self.groups_of_track(track).and_then(|gs| gs.last().map(|g| g.id)) {
            let needed = match self.clip(group).map(|g| &g.content) {
                Some(ClipContent::Group(g)) if g.cycle.is_none() => {
                    g.tracks.iter().flat_map(|t| t.clips.last()).map(Clip::end).max()
                }
                _ => None,
            };
            if let Some(end) = needed
                && self.clip(group).is_some_and(|g| g.duration < end)
            {
                self.set_duration(group, end, false);
            }
            let Some(next) = self.track_of_clip(group) else { return };
            track = next;
        }
    }

    /// Clip `id` taken out of the group clip it is in, onto the group's level at the frame it
    /// showed on: on the track just above the group's when there is room there, else on a new
    /// track just above it. The group's opacity and keys are folded into it, as ungrouping does.
    /// Returns its track.
    pub fn move_out_of_group(&mut self, id: ClipId) -> Option<TrackId> {
        let group = self.clip(self.group_of(id)?)?.clone();
        let clip = self.clip(id)?;
        let (start, duration, name) = (group.start + clip.start, clip.duration, clip.name.clone());
        let offset = i32::try_from(clip.start).ok()?;
        let home = self.track_of_clip(group.id)?;
        let level = level_of_track(&mut self.tracks, home)?;
        let at = level.iter().position(|t| t.id == home)?;
        let above =
            level.get(at + 1).filter(|t| t.clips.iter().all(|c| c.end() <= start || c.start >= start + duration));
        let onto = match above {
            Some(t) => t.id,
            None => {
                let track = Track {
                    id: TrackId(self.next_track),
                    name,
                    hidden: false,
                    locked: false,
                    opacity: 1.0,
                    look: Default::default(),
                    clips: Vec::new(),
                };
                self.next_track += 1;
                let id = track.id;
                level_of_track(&mut self.tracks, home)?.insert(at + 1, track);
                id
            }
        };
        if let Some(c) = self.clip_mut(id) {
            fold_into(c, &group, offset);
        }
        self.move_clip(id, onto, start).then_some(onto)
    }

    /// Clip `id` put into group clip `group` (on the same level) at the frame it is at, no earlier
    /// than the group's start: on the group's topmost track with room there, or a new track on
    /// top of its own. The group grows to cover it. Returns its track.
    pub fn move_into_group(&mut self, id: ClipId, group: ClipId) -> Option<TrackId> {
        if id == group || self.group_of(id) != self.group_of(group) {
            return None;
        }
        let g = self.clip(group)?;
        let clip = self.clip(id)?;
        let start = clip.start.saturating_sub(g.start);
        let (duration, name) = (clip.duration, clip.name.clone());
        let ClipContent::Group(inner) = &g.content else { return None };
        let room = inner
            .tracks
            .iter()
            .rev()
            .find(|t| t.clips.iter().all(|c| c.end() <= start || c.start >= start + duration))
            .map(|t| t.id);
        let onto = match room {
            Some(t) => t,
            None => self.add_group_track(group, &name)?,
        };
        self.move_clip(id, onto, start).then_some(onto)
    }

    /// Group clip `id` taken apart, its clips back on the level it is on (as
    /// [`Timeline::ungroup_copying`] does, with no copies made: a cycle's later repeats of drawings
    /// are left out).
    pub fn ungroup(&mut self, id: ClipId) -> bool {
        self.ungroup_copying(id, &mut |_| None)
    }

    /// Group clip `id` taken apart, keeping what it did as far as clips on their own can: its
    /// bottom track's clips go back onto its own track and its other tracks just above that.
    ///
    /// - Its opacity and keys are folded into each clip that comes out: exactly, onto a channel the
    ///   clip has no keys on; worked out on every frame either has a key on, where both have.
    /// - A cycle is laid out: each clip in its first cycle comes out again for every cycle the
    ///   group plays, cut at the cycle's end and the group's, a drawing as a copy of its layers
    ///   from `copy` (none: that repeat is left out). Clips past the first cycle, which never
    ///   showed, go on a hidden track of their own.
    ///
    /// False if it isn't a group.
    pub fn ungroup_copying(&mut self, id: ClipId, copy: &mut dyn FnMut(LayerId) -> Option<LayerId>) -> bool {
        let Some(group) = self.clip(id).cloned() else { return false };
        let ClipContent::Group(g) = &group.content else { return false };
        let mut next_track = self.next_track;
        let mut next_clip = self.next_clip;
        let mut tracks = Vec::with_capacity(g.tracks.len() + 1);
        let mut unshown = Vec::new();
        for track in &g.tracks {
            let mut clips = Vec::new();
            for clip in &track.clips {
                let Some(cycle) = g.cycle else {
                    let mut c = clip.clone();
                    fold_into(&mut c, &group, i32::try_from(clip.start).unwrap_or(i32::MAX));
                    clips.push(c);
                    continue;
                };
                let length = cycle.length.max(1);
                if clip.start >= length {
                    unshown.push(clip.clone());
                    continue;
                }
                for k in 0_u32.. {
                    let Some(start) = k.checked_mul(length).and_then(|s: u32| s.checked_add(clip.start)) else { break };
                    if start >= group.duration {
                        break;
                    }
                    let mut c = clip.clone();
                    c.start = start;
                    c.duration = clip.duration.min(length - clip.start).min(group.duration - start);
                    if k > 0 {
                        let mut roots = std::collections::HashMap::new();
                        let wanted = c.roots();
                        for root in &wanted {
                            if let Some(made) = copy(*root) {
                                roots.insert(*root, made);
                            }
                        }
                        if roots.len() < wanted.len() {
                            continue;
                        }
                        renumber(&mut c, &mut next_track, &mut next_clip, &roots);
                    }
                    let offset = match cycle.keys {
                        CycleKeys::Repeat => clip.start,
                        CycleKeys::ContinueAcross => start,
                    };
                    fold_into(&mut c, &group, i32::try_from(offset).unwrap_or(i32::MAX));
                    clips.push(c);
                }
            }
            clips.sort_by_key(|c| c.start);
            tracks.push(Track { clips, ..track.clone() });
        }
        if !unshown.is_empty() {
            unshown.sort_by_key(|c| c.start);
            let mut kept: Vec<Clip> = Vec::new();
            for c in unshown {
                // (from different tracks they may overlap: each goes after the one before)
                let start = kept.last().map_or(c.start, |l| c.start.max(l.end()));
                kept.push(Clip { start, ..c });
            }
            let name = format!("{} (not shown)", group.name);
            tracks.push(Track {
                id: TrackId(next_track),
                name,
                hidden: true,
                locked: false,
                opacity: 1.0,
                look: Default::default(),
                clips: kept,
            });
            next_track += 1;
        }
        let Some(clip) = self.clip_mut(id) else { return false };
        if let ClipContent::Group(g) = &mut clip.content {
            g.tracks = tracks;
            g.cycle = None;
        }
        self.next_track = next_track;
        self.next_clip = next_clip;
        self.take_apart(id)
    }

    /// What group clip `id` shows on each of its frames, as runs of frames showing the same
    /// drawings and videos: each run's first frame (counted from the group's own first), how many
    /// frames it is, and what shows there, bottom first (its own tracks', and those of the groups
    /// inside it; hidden tracks' and disabled clips' left out; a cycle played round), a video at
    /// the time in its file the run's first frame shows. What a panel draws a folded group's
    /// frames from. At most [`MOST_GROUP_SHOT_FRAMES`] frames of it.
    pub fn group_shots(&self, id: ClipId) -> Vec<(u32, u32, Vec<GroupShown>)> {
        fn drawings_at(g: &Group, frame: i64, fps: u32, out: &mut Vec<GroupShown>) {
            let local = match g.cycle {
                Some(c) => frame.rem_euclid(i64::from(c.length.max(1))),
                None => frame,
            };
            for t in g.tracks.iter().filter(|t| !t.hidden) {
                for c in t.clips.iter().filter(|c| !c.disabled) {
                    if i64::from(c.start) <= local && local < i64::from(c.end()) {
                        let within = u32::try_from(local - i64::from(c.start)).unwrap_or(0);
                        match &c.content {
                            ClipContent::Drawing(d) => out.push(GroupShown::Drawing(d.root)),
                            ClipContent::Group(inner) => drawings_at(inner, i64::from(within), fps, out),
                            ClipContent::Video(v) => {
                                out.push(GroupShown::Video { media: v.media, seconds: v.picture_at(within, fps) });
                            }
                            ClipContent::Audio(_) => {}
                        }
                    }
                }
            }
        }
        let Some(clip) = self.clip(id) else { return Vec::new() };
        let ClipContent::Group(g) = &clip.content else { return Vec::new() };
        let mut out: Vec<(u32, u32, Vec<GroupShown>)> = Vec::new();
        for f in 0..clip.duration.min(MOST_GROUP_SHOT_FRAMES) {
            let mut shown = Vec::new();
            drawings_at(g, i64::from(f), self.fps, &mut shown);
            match out.last_mut() {
                Some(last) if last.2.len() == shown.len() && last.2.iter().zip(&shown).all(|(a, b)| a.same(b)) => {
                    last.1 += 1
                }
                _ => out.push((f, 1, shown)),
            }
        }
        out
    }

    /// Sound or video clip `id` cut at frame `at` (on its level, inside it): it ends there, and a
    /// clip of the same media plays on from there, its volume keys going on from where it was
    /// cut. Returns the new clip.
    pub fn split_media(&mut self, id: ClipId, at: u32) -> Option<ClipId> {
        let fps = f64::from(self.fps.max(1));
        let next = self.next_clip;
        let (level, t, c) = level_of_clip(&mut self.tracks, id)?;
        let track = level.get_mut(t)?;
        let clip = track.clips.get_mut(c)?;
        if at <= clip.start
            || at >= clip.end()
            || !matches!(clip.content, ClipContent::Audio(_) | ClipContent::Video(_))
        {
            return None;
        }
        let cut = at - clip.start;
        let mut rest = clip.clone();
        clip.duration = cut;
        rest.id = ClipId(next);
        rest.start = at;
        rest.duration -= cut;
        rest.keys = shifted_keys(&rest.keys, i64::from(cut));
        match &mut rest.content {
            ClipContent::Audio(a) => {
                // on from where the sound was at that frame: the offset is only the first piece's
                a.trim_start = (a.trim_start + f64::from(cut) / fps - a.offset).max(0.0);
                a.offset = 0.0;
                a.volume_keys = shifted(&a.volume_keys, i32::try_from(cut).unwrap_or(i32::MAX));
            }
            ClipContent::Video(v) => v.trim_start += f64::from(cut) / fps,
            ClipContent::Drawing(_) | ClipContent::Group(_) => return None,
        }
        track.clips.insert(c + 1, rest);
        self.next_clip += 1;
        Some(ClipId(next))
    }

    /// The drawing clips a cut of group clip `id` at frame `at` of its level would go through
    /// (those of its own tracks, and of the groups inside it, that the frame falls inside past
    /// their first), each with its drawing's layer: [`Timeline::split_group`] wants a copy of each.
    /// Empty where the group can't be cut there.
    pub fn group_cut_drawings(&self, id: ClipId, at: u32) -> Vec<(ClipId, LayerId)> {
        fn walk(g: &Group, cut: u32, out: &mut Vec<(ClipId, LayerId)>) {
            for c in g.tracks.iter().flat_map(|t| &t.clips).filter(|c| c.start < cut && cut < c.end()) {
                match &c.content {
                    ClipContent::Drawing(d) => out.push((c.id, d.root)),
                    ClipContent::Group(inner) => walk(inner, cut - c.start, out),
                    ClipContent::Audio(_) | ClipContent::Video(_) => {}
                }
            }
        }
        let mut out = Vec::new();
        if let Some(clip) = self.clip(id)
            && let ClipContent::Group(g) = &clip.content
            && at > clip.start
            && at < clip.end()
        {
            walk(g, at - clip.start, &mut out);
        }
        out
    }

    /// Group clip `id` cut at frame `at` of its level (inside it): it ends there, and a new group
    /// after it holds the rest, on tracks of its own like the first's. A clip of theirs the cut
    /// goes through is cut too: a sound or video plays on from there, a group inside is cut the
    /// same way, and a drawing's second part shows the layer `copies` gives for it (a copy of its
    /// drawing; see [`Timeline::group_cut_drawings`]). Each second part's keys go on from the
    /// cut. Returns the new group; none (nothing changed) if it isn't a group, it or a group
    /// inside it the cut goes through cycles, `at` isn't inside it, or a copy is missing.
    pub fn split_group(&mut self, id: ClipId, at: u32, copies: &HashMap<ClipId, LayerId>) -> Option<ClipId> {
        /// The clips of `tracks` from frame `cut` on taken out onto new tracks like them (counted
        /// from the cut), the clips the cut goes through cut in two.
        fn cut_tracks(
            tracks: &mut [Track],
            cut: u32,
            fps: f64,
            copies: &HashMap<ClipId, LayerId>,
            next: &mut (u64, u64),
        ) -> Option<Vec<Track>> {
            let mut rest = Vec::new();
            for t in tracks.iter_mut() {
                let (mut stay, mut go) = (Vec::new(), Vec::new());
                for mut c in std::mem::take(&mut t.clips) {
                    if c.end() <= cut {
                        stay.push(c);
                        continue;
                    }
                    if c.start >= cut {
                        c.start -= cut;
                        go.push(c);
                        continue;
                    }
                    let before = cut - c.start;
                    let mut second = c.clone();
                    second.id = ClipId(next.1);
                    next.1 += 1;
                    second.start = 0;
                    second.duration = c.duration - before;
                    second.keys = shifted_keys(&second.keys, i64::from(before));
                    match (&mut c.content, &mut second.content) {
                        (_, ClipContent::Drawing(d)) => d.root = *copies.get(&c.id)?,
                        (_, ClipContent::Audio(a)) => {
                            a.trim_start = (a.trim_start + f64::from(before) / fps - a.offset).max(0.0);
                            a.offset = 0.0;
                            a.volume_keys = shifted(&a.volume_keys, i32::try_from(before).unwrap_or(i32::MAX));
                        }
                        (_, ClipContent::Video(v)) => v.trim_start += f64::from(before) / fps,
                        (ClipContent::Group(first), ClipContent::Group(g)) => {
                            if g.cycle.is_some() {
                                return None;
                            }
                            g.tracks = cut_tracks(&mut first.tracks, before, fps, copies, next)?;
                        }
                        _ => return None,
                    }
                    c.duration = before;
                    stay.push(c);
                    go.push(second);
                }
                t.clips = stay;
                rest.push(Track { id: TrackId(next.0), clips: go, ..t.clone() });
                next.0 += 1;
            }
            Some(rest)
        }

        // (worked on a copy: a cut that can't be finished leaves nothing half done)
        let mut work = self.clone();
        let fps = f64::from(work.fps.max(1));
        let mut next = (work.next_track, work.next_clip);
        let (level, t, c) = level_of_clip(&mut work.tracks, id)?;
        let track = level.get_mut(t)?;
        let clip = track.clips.get_mut(c)?;
        if at <= clip.start || at >= clip.end() {
            return None;
        }
        let before = at - clip.start;
        let mut rest = clip.clone();
        let ClipContent::Group(g) = &mut clip.content else { return None };
        if g.cycle.is_some() {
            return None;
        }
        let tracks = cut_tracks(&mut g.tracks, before, fps, copies, &mut next)?;
        clip.duration = before;
        rest.id = ClipId(next.1);
        next.1 += 1;
        rest.start = at;
        rest.duration -= before;
        rest.keys = shifted_keys(&rest.keys, i64::from(before));
        if let ClipContent::Group(g) = &mut rest.content {
            g.tracks = tracks;
        }
        let made = rest.id;
        track.clips.insert(c + 1, rest);
        (work.next_track, work.next_clip) = next;
        *self = work;
        Some(made)
    }

    /// A copy of sound or video clip `id` just after it, as long; the clips after it move along.
    /// Returns the copy.
    pub fn duplicate_media(&mut self, id: ClipId) -> Option<ClipId> {
        let next = self.next_clip;
        let (level, t, c) = level_of_clip(&mut self.tracks, id)?;
        let track = level.get_mut(t)?;
        let mut copy = track.clips.get(c)?.clone();
        if !matches!(copy.content, ClipContent::Audio(_) | ClipContent::Video(_)) {
            return None;
        }
        copy.id = ClipId(next);
        copy.start = copy.end();
        make_room(track, copy.start, copy.end());
        track.clips.insert(c + 1, copy);
        self.next_clip += 1;
        self.fit();
        Some(ClipId(next))
    }
}

impl Document {
    /// A new empty layer in the drawing `root` is (the layers panel's + on a timeline): above the
    /// current layer when that is in it, else on top of it. A drawing of one layer is put in a
    /// group first, which its clip shows from then on. It becomes current; none if `root` isn't a
    /// drawing's or there is no room.
    pub fn add_layer_to_drawing(&mut self, root: LayerId) -> Option<LayerId> {
        if !self.has_room(1) {
            return None;
        }
        let group = self.drawing_group(root)?;
        self.add_layer_into(group)
    }

    /// The group the drawing `root` is: `root` itself, or, for a drawing of one layer, a group it
    /// is put in now, which its clip shows from then on (the layer keeping its name in it as
    /// "Layer 1"). None if `root` isn't a drawing's.
    pub fn drawing_group(&mut self, root: LayerId) -> Option<LayerId> {
        let (_, clip) = self.timeline.as_ref()?.drawing_of(root)?;
        match self.layer(root)?.kind {
            crate::document::LayerKind::Group => Some(root),
            crate::document::LayerKind::Paint => {
                let group = self.wrap_in_group(root, "Layer 1")?;
                if let Some(ClipContent::Drawing(d)) =
                    self.timeline.as_mut().and_then(|tl| tl.clip_mut(clip)).map(|c| &mut c.content)
                {
                    d.root = group;
                }
                Some(group)
            }
            crate::document::LayerKind::Mask => None,
        }
    }
}

/// `channel` with every key `by` frames earlier.
fn shifted<T: Clone>(channel: &Channel<T>, by: i32) -> Channel<T> {
    Channel { keys: channel.keys.iter().map(|k| Key { frame: k.frame.saturating_sub(by), ..k.clone() }).collect() }
}

/// The frames any of `channels` has keys on, `by` frames earlier, in order, once each.
fn frames_of(channels: &[(&[i32], i32)]) -> Vec<i32> {
    let mut frames: Vec<i32> =
        channels.iter().flat_map(|(fs, by)| fs.iter().map(move |f| f.saturating_sub(*by))).collect();
    frames.sort_unstable();
    frames.dedup();
    frames
}

fn key_frames<T>(c: &Channel<T>) -> Vec<i32> {
    c.keys.iter().map(|k| k.frame).collect()
}

/// The easing of the key on frame `frame` of the first of `channels` that has one there.
fn easing_on(channels: &[(&[(i32, Easing)], i32)], frame: i32) -> Easing {
    channels
        .iter()
        .find_map(|(keys, by)| keys.iter().find(|(f, _)| f.saturating_sub(*by) == frame).map(|(_, e)| e.clone()))
        .unwrap_or(Easing::Linear)
}

fn easings<T>(c: &Channel<T>) -> Vec<(i32, Easing)> {
    c.keys.iter().map(|k| (k.frame, k.easing.clone())).collect()
}

/// Clip `clip`, inside group clip `group`, taking on the group's opacity and its keys as they
/// play over it, `offset` being the group's keyed frame on the clip's first frame. Onto a channel
/// the clip has no keys on, the group's go as they are (exactly as they played); where both
/// have keys, the two are worked out together on every frame either has a key on, eased as the
/// clip's key there (or else the group's) eases. A pose is worked out about the clip's pivot,
/// turns added and scales multiplied (exact unless one scales unevenly while the other turns).
fn fold_into(clip: &mut Clip, group: &Clip, offset: i32) {
    let (gk, ck) = (&group.keys, &mut clip.keys);
    // opacity
    if gk.opacity.keys.is_empty() {
        if ck.opacity.keys.is_empty() {
            clip.opacity *= group.opacity;
        } else {
            ck.opacity.keys.iter_mut().for_each(|k| k.value *= group.opacity);
        }
    } else if ck.opacity.keys.is_empty() {
        let mut keys = shifted(&gk.opacity, offset);
        keys.keys.iter_mut().for_each(|k| k.value *= clip.opacity);
        ck.opacity = keys;
    } else {
        let frames = frames_of(&[(&key_frames(&ck.opacity), 0), (&key_frames(&gk.opacity), offset)]);
        let (ce, ge) = (easings(&ck.opacity), easings(&gk.opacity));
        let keys = frames
            .into_iter()
            .map(|f| Key {
                frame: f,
                value: ck.opacity.value_at(f as f32).unwrap_or(1.0)
                    * gk.opacity.value_at(f.saturating_add(offset) as f32).unwrap_or(1.0),
                easing: easing_on(&[(&ce, 0), (&ge, offset)], f),
                path: None,
            })
            .collect();
        ck.opacity = Channel { keys };
    }
    // pose
    if !gk.posed() {
        return;
    }
    if !ck.posed() {
        ck.position = shifted(&gk.position, offset);
        ck.scale = shifted(&gk.scale, offset);
        ck.rotation = shifted(&gk.rotation, offset);
        ck.pivot = gk.pivot;
        return;
    }
    let frames = frames_of(&[
        (&key_frames(&ck.position), 0),
        (&key_frames(&ck.scale), 0),
        (&key_frames(&ck.rotation), 0),
        (&key_frames(&gk.position), offset),
        (&key_frames(&gk.scale), offset),
        (&key_frames(&gk.rotation), offset),
    ]);
    let (cp, cs, cr) = (easings(&ck.position), easings(&ck.scale), easings(&ck.rotation));
    let (gp, gs, gr) = (easings(&gk.position), easings(&gk.scale), easings(&gk.rotation));
    let order = [(&cp[..], 0), (&cs[..], 0), (&cr[..], 0), (&gp[..], offset), (&gs[..], offset), (&gr[..], offset)];
    let pivot = ck.pivot.unwrap_or([0.0; 2]);
    let (mut position, mut scale, mut rotation) = (Vec::new(), Vec::new(), Vec::new());
    for f in frames {
        let at = f.saturating_add(offset) as f32;
        let (Some(c), Some(g)) = (ck.pose_at(f as f32), gk.pose_at(at)) else { continue };
        let moved = c.then(g).apply(Vec2::new(pivot[0], pivot[1]));
        let [csx, csy] = ck.scale.value_at(f as f32).unwrap_or([1.0; 2]);
        let [gsx, gsy] = gk.scale.value_at(at).unwrap_or([1.0; 2]);
        let turn = ck.rotation.value_at(f as f32).unwrap_or(0.0) + gk.rotation.value_at(at).unwrap_or(0.0);
        let easing = easing_on(&order, f);
        position.push(Key {
            frame: f,
            value: [moved.x - pivot[0], moved.y - pivot[1]],
            easing: easing.clone(),
            path: None,
        });
        scale.push(Key { frame: f, value: [csx * gsx, csy * gsy], easing: easing.clone(), path: None });
        rotation.push(Key { frame: f, value: turn, easing, path: None });
    }
    ck.position = Channel { keys: position };
    ck.scale = Channel { keys: scale };
    ck.rotation = Channel { keys: rotation };
    ck.pivot = Some(pivot);
}
