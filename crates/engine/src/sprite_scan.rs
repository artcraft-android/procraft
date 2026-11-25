//! The Texture Packer's finding of sprites: the separate
//! drawings on a picture with no background, each as the box round it, so each can be cut out as
//! a PNG of its own.

/// A box on the picture: its top left corner and size (pixels).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SpriteBox {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl SpriteBox {
    fn right(&self) -> u32 {
        self.x + self.width
    }

    fn bottom(&self) -> u32 {
        self.y + self.height
    }

    /// Whether fewer than `separation` clear pixels lie between it and `other` (at 1, only
    /// touching ones, a corner too).
    fn near(&self, other: &SpriteBox, separation: u32) -> bool {
        self.x < other.right() + separation
            && other.x < self.right() + separation
            && self.y < other.bottom() + separation
            && other.y < self.bottom() + separation
    }

    fn union(&self, other: &SpriteBox) -> SpriteBox {
        let (x, y) = (self.x.min(other.x), self.y.min(other.y));
        SpriteBox { x, y, width: self.right().max(other.right()) - x, height: self.bottom().max(other.bottom()) - y }
    }

    /// Whether the point (`x`, `y`) is on it.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x as f32 && y >= self.y as f32 && x < self.right() as f32 && y < self.bottom() as f32
    }
}

/// How opaque a pixel must be to count as drawn (out of 255): fainter ones are left out.
pub const ALPHA_THRESHOLD: u8 = 8;

/// The fewest pixels across (its longer side) a drawing must be not to be dropped as a stray.
pub const SMALLEST_SIDE: u32 = 4;

/// The boxes round each separate drawing on `rgba` (`width` × `height` pixels, RGBA, alpha
/// marking what is drawn): pixels at least [`ALPHA_THRESHOLD`] opaque, joined to their four
/// neighbours, each group's box kept unless both its sides are under [`SMALLEST_SIDE`].
pub fn components(rgba: &[u8], width: u32, height: u32) -> Vec<SpriteBox> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || rgba.len() < w * h * 4 {
        return Vec::new();
    }
    // whether each pixel is drawn and not yet taken into a group
    let mut open: Vec<bool> = rgba.as_chunks::<4>().0.iter().take(w * h).map(|p| p[3] > ALPHA_THRESHOLD).collect();
    let mut boxes = Vec::new();
    let mut stack = Vec::with_capacity(4096);
    // takes pixel `n` into the group if it is drawn and not yet taken
    let take = |open: &mut Vec<bool>, stack: &mut Vec<usize>, n: usize| {
        if let Some(o) = open.get_mut(n)
            && *o
        {
            *o = false;
            stack.push(n);
        }
    };
    for start in 0..w * h {
        if !open.get(start).copied().unwrap_or(false) {
            continue;
        }
        take(&mut open, &mut stack, start);
        let (mut x0, mut y0) = (start % w, start / w);
        let (mut x1, mut y1) = (x0, y0);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
            if x > 0 {
                take(&mut open, &mut stack, i - 1);
            }
            if x + 1 < w {
                take(&mut open, &mut stack, i + 1);
            }
            if y > 0 {
                take(&mut open, &mut stack, i - w);
            }
            if y + 1 < h {
                take(&mut open, &mut stack, i + w);
            }
        }
        let b = SpriteBox { x: x0 as u32, y: y0 as u32, width: (x1 - x0 + 1) as u32, height: (y1 - y0 + 1) as u32 };
        if b.width.max(b.height) >= SMALLEST_SIDE {
            boxes.push(b);
        }
    }
    boxes
}

/// `boxes` with those fewer than `separation` clear pixels apart joined into one (so the
/// loose parts of one drawing, an i's dot or a speech bubble's tail, stay with it), in reading
/// order: top to bottom, then left to right.
pub fn merged(boxes: &[SpriteBox], separation: u32) -> Vec<SpriteBox> {
    let mut out = boxes.to_vec();
    // each box taken in turn, any near it joined into it, until none are near another
    let mut i = 0;
    while i < out.len() {
        let Some(&mine) = out.get(i) else { break };
        match out.iter().enumerate().position(|(j, b)| j != i && mine.near(b, separation)) {
            Some(j) => {
                let other = out.swap_remove(j);
                let at = if i == out.len() { j } else { i };
                if let Some(slot) = out.get_mut(at) {
                    *slot = mine.union(&other);
                }
                // what it grew to may reach ones already passed
                i = 0;
            }
            None => i += 1,
        }
    }
    out.sort_by_key(|b| (b.y, b.x));
    out
}

/// Each box grown by `padding` pixels all round, kept on the picture (`width` × `height`).
pub fn padded(boxes: &[SpriteBox], padding: u32, width: u32, height: u32) -> Vec<SpriteBox> {
    boxes
        .iter()
        .map(|b| {
            let (x, y) = (b.x.saturating_sub(padding), b.y.saturating_sub(padding));
            let right = (b.right() + padding).min(width);
            let bottom = (b.bottom() + padding).min(height);
            SpriteBox { x, y, width: right.saturating_sub(x), height: bottom.saturating_sub(y) }
        })
        .collect()
}

/// The box (an index into `boxes`) a tap at (`x`, `y`) picks: the smallest holding it.
pub fn hit(boxes: &[SpriteBox], x: f32, y: f32) -> Option<usize> {
    boxes
        .iter()
        .enumerate()
        .filter(|(_, b)| b.contains(x, y))
        .min_by_key(|(_, b)| u64::from(b.width) * u64::from(b.height))
        .map(|(i, _)| i)
}

/// The name sprite `index` (from 0, in reading order) has until it is renamed.
pub fn default_name(index: usize) -> String {
    format!("Sprite_{:02}", index + 1)
}

/// File names (without `.png`) for sprites named `names` (in order, `indices` their places for
/// the default): what can't be in a file name made a dash, an empty one or one starting with a
/// dot its default, and a name used before given `_2`, `_3`… so none is written over.
pub fn file_names(names: &[(usize, String)]) -> Vec<String> {
    let mut used = std::collections::HashSet::new();
    names
        .iter()
        .map(|(index, name)| {
            let cleaned: String = name
                .trim()
                .chars()
                .map(|c| if matches!(c, '/' | '\\' | ':' | '\0') || c.is_control() { '-' } else { c })
                .collect();
            let base = if cleaned.is_empty() || cleaned.starts_with('.') { default_name(*index) } else { cleaned };
            let mut candidate = base.clone();
            let mut n = 2;
            while !used.insert(candidate.to_lowercase()) {
                candidate = format!("{base}_{n}");
                n += 1;
            }
            candidate
        })
        .collect()
}

/// The pixels of `b` cut out of `rgba` (`width` pixels across, four bytes a pixel).
pub fn crop(rgba: &[u8], width: u32, b: &SpriteBox) -> Vec<u8> {
    let (w, x, bw) = (width as usize, b.x as usize, b.width as usize);
    let mut out = Vec::with_capacity(bw * b.height as usize * 4);
    for y in b.y as usize..b.bottom() as usize {
        let start = (y * w + x) * 4;
        if let Some(row) = rgba.get(start..start + bw * 4) {
            out.extend_from_slice(row);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `w` × `h` picture with the given pixels drawn (opaque) and the rest clear.
    fn picture(w: u32, h: u32, drawn: &[(u32, u32)]) -> Vec<u8> {
        let mut px = vec![0u8; (w * h * 4) as usize];
        for &(x, y) in drawn {
            let i = ((y * w + x) * 4) as usize;
            px[i..i + 4].copy_from_slice(&[255, 0, 0, 255]);
        }
        px
    }

    fn square(x: u32, y: u32, side: u32) -> Vec<(u32, u32)> {
        (y..y + side).flat_map(|r| (x..x + side).map(move |c| (c, r))).collect()
    }

    fn b(x: u32, y: u32, width: u32, height: u32) -> SpriteBox {
        SpriteBox { x, y, width, height }
    }

    #[test]
    fn separate_drawings_are_found_as_boxes_and_strays_left_out() {
        let mut drawn = square(2, 2, 5);
        drawn.extend(square(20, 3, 6));
        // a stray pixel, and a faint one that isn't drawn
        drawn.push((30, 30));
        let mut px = picture(40, 40, &drawn);
        let faint = ((35 * 40 + 35) * 4) as usize;
        px[faint + 3] = ALPHA_THRESHOLD;
        let found = merged(&components(&px, 40, 40), 1);
        // a diagonal step is two of the four-way groups, joined again at 1
        let steps = picture(8, 8, &[(0, 0), (1, 0), (2, 0), (3, 0), (4, 1), (5, 1), (6, 1), (7, 1)]);
        assert_eq!(components(&steps, 8, 8).len(), 2);
        assert_eq!(merged(&components(&steps, 8, 8), 1), vec![b(0, 0, 8, 2)]);
        assert_eq!(found, vec![b(2, 2, 5, 5), b(20, 3, 6, 6)]);
    }

    #[test]
    fn near_parts_join_by_the_separation_and_come_in_reading_order() {
        // an i: its dot two clear pixels over its stem; and two drawings on a lower row
        let parts = [b(10, 0, 2, 2), b(10, 4, 2, 8), b(30, 20, 4, 4), b(0, 20, 4, 4)];
        assert_eq!(merged(&parts, 2).len(), 4);
        let joined = merged(&parts, 3);
        assert_eq!(joined, vec![b(10, 0, 2, 12), b(0, 20, 4, 4), b(30, 20, 4, 4)]);
        // far enough, all of them one
        assert_eq!(merged(&parts, 40), vec![b(0, 0, 34, 24)]);
        // a chain: joining two brings a third within reach
        let chain = [b(0, 0, 2, 2), b(20, 0, 2, 2), b(3, 0, 17, 2)];
        assert_eq!(merged(&chain, 2), vec![b(0, 0, 22, 2)]);
        // at 1 only touching ones join, a corner too
        assert_eq!(merged(&[b(0, 0, 2, 2), b(2, 2, 2, 2), b(5, 0, 1, 1)], 1), vec![b(0, 0, 4, 4), b(5, 0, 1, 1)]);
    }

    #[test]
    fn padding_grows_boxes_within_the_picture() {
        let boxes = [b(1, 1, 4, 4), b(10, 10, 5, 5)];
        assert_eq!(padded(&boxes, 3, 16, 16), vec![b(0, 0, 8, 8), b(7, 7, 9, 9)]);
    }

    #[test]
    fn a_tap_picks_the_smallest_box_under_it() {
        let boxes = [b(0, 0, 20, 20), b(5, 5, 4, 4)];
        assert_eq!(hit(&boxes, 6.0, 6.0), Some(1));
        assert_eq!(hit(&boxes, 15.0, 15.0), Some(0));
        assert_eq!(hit(&boxes, 25.0, 1.0), None);
    }

    #[test]
    fn file_names_are_safe_and_never_the_same() {
        let names = [
            (0, "hero/run".to_owned()),
            (1, "  ".to_owned()),
            (2, ".hidden".to_owned()),
            (3, "Coin".to_owned()),
            (4, "coin".to_owned()),
            (5, "Coin".to_owned()),
        ];
        assert_eq!(file_names(&names), ["hero-run", "Sprite_02", "Sprite_03", "Coin", "coin_2", "Coin_3"]);
    }

    #[test]
    fn a_crop_is_the_box_s_pixels() {
        let px = picture(8, 8, &square(2, 3, 2));
        let cut = crop(&px, 8, &b(2, 3, 3, 2));
        assert_eq!(cut.len(), 3 * 2 * 4);
        assert_eq!(&cut[0..4], &[255, 0, 0, 255]);
        assert_eq!(&cut[8..12], &[0, 0, 0, 0]);
    }
}
