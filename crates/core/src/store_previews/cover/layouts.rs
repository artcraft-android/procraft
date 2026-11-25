pub(super) mod ink;
pub(super) mod paper;

use super::surface::{CoverArt, oval, rectangle};

pub(super) fn fallback(art: &mut CoverArt<'_>) {
    art.stock(0xe9dbc1);
    let (cx, cy, radius) = if art.titled() { (522.0, 344.0, 205.0) } else { (420.0, 315.0, 245.0) };
    art.fill(&oval((cx, cy), radius, radius), 0x9f5748, 1.0);
    art.line(&oval((cx, cy), radius - 20.0, radius - 20.0), 0xefdbc1, 2.0, 0.9);
    art.line(&rectangle((29.0, 29.0, 782.0, 572.0)), 0x5c453c, 2.0, 0.7);
    art.title((55.0, 65.0, 360.0, 220.0), 66.0, 0x332a28, true);
}
