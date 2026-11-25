//! Static storefront covers for plugins whose publisher artwork is unavailable.

use image::RgbaImage;

use super::super::{PREVIEW_HEIGHT, PREVIEW_WIDTH, seed_of};
use super::surface::{Bounds, CoverArt, oval, polygon, polyline, rectangle};

const PALETTES: [(u32, u32, u32, u32); 6] = [
    (0x132827, 0xdcf1e1, 0x78ddba, 0x2c625b),
    (0x241d3e, 0xf6e5ff, 0xe6a4f7, 0x6656a6),
    (0x16293e, 0xe6f2ff, 0x84cafd, 0x315b9e),
    (0x36231d, 0xffeee1, 0xf7ac78, 0x93553f),
    (0xf3ead9, 0x2a2636, 0xad3b58, 0xdec9db),
    (0xe5eddd, 0x173833, 0x2b7564, 0xb9d1a5),
];

#[derive(Clone, Copy, Debug)]
enum Layout {
    Circuit,
    Aperture,
    Isometric,
    Signal,
    Mosaic,
    Cutout,
    Orbit,
    Prism,
    Fold,
    Grid,
}

const LAYOUTS: [Layout; 10] = [
    Layout::Circuit,
    Layout::Aperture,
    Layout::Isometric,
    Layout::Signal,
    Layout::Mosaic,
    Layout::Cutout,
    Layout::Orbit,
    Layout::Prism,
    Layout::Fold,
    Layout::Grid,
];

pub(crate) fn plugin_cover(item: &crate::StoreItem) -> RgbaImage {
    render(item, PREVIEW_HEIGHT)
}

pub(crate) fn plugin_icon(item: &crate::StoreItem) -> RgbaImage {
    render(item, PREVIEW_WIDTH)
}

fn render(item: &crate::StoreItem, height: u32) -> RgbaImage {
    let seed = seed_of(&item.id);
    let layout = LAYOUTS.get((seed % LAYOUTS.len() as u64) as usize).copied().unwrap_or(Layout::Circuit);
    let palette = PALETTES
        .get(((seed >> 24) % PALETTES.len() as u64) as usize)
        .copied()
        .unwrap_or((0x132827, 0xdcf1e1, 0x78ddba, 0x2c625b));
    draw(item, layout, palette, height)
}

fn draw(
    item: &crate::StoreItem,
    layout: Layout,
    (back, ink, accent, mid): (u32, u32, u32, u32),
    height: u32,
) -> RgbaImage {
    let mut art = CoverArt::new(Some(&item.name), height);
    art.stock(back);
    let (title, author): (Bounds, Bounds) = match layout {
        Layout::Circuit => {
            for (offset, colour) in [(0.0, accent), (46.0, mid), (92.0, accent), (138.0, mid)] {
                let points = [
                    (456.0, 420.0 + offset),
                    (572.0, 420.0 + offset),
                    (658.0, 334.0 + offset),
                    (792.0, 334.0 + offset),
                ];
                art.line(&polyline(&points, false), colour, 6.0, 0.9);
                art.fill(&oval((792.0, 334.0 + offset), 12.0, 12.0), colour, 1.0);
            }
            ((67.0, 150.0, 700.0, 240.0), (67.0, 525.0, 360.0, 40.0))
        }
        Layout::Aperture => {
            for (radius, colour) in [(185.0, mid), (135.0, accent), (85.0, mid)] {
                art.line(&oval((635.0, 215.0), radius, radius), colour, 24.0, 0.8);
            }
            art.fill(&oval((635.0, 215.0), 34.0, 34.0), accent, 1.0);
            ((67.0, 264.0, 415.0, 227.0), (67.0, 537.0, 575.0, 45.0))
        }
        Layout::Isometric => {
            let top = [(597.0, 147.0), (730.0, 209.0), (597.0, 271.0), (464.0, 209.0)];
            art.fill(&polygon(&top), accent, 1.0);
            art.fill(&polygon(&[(464.0, 209.0), (597.0, 271.0), (597.0, 434.0), (464.0, 372.0)]), mid, 1.0);
            art.fill(&polygon(&[(597.0, 271.0), (730.0, 209.0), (730.0, 372.0), (597.0, 434.0)]), accent, 0.57);
            ((67.0, 180.0, 377.0, 295.0), (67.0, 532.0, 675.0, 44.0))
        }
        Layout::Signal => {
            for (at, height) in
                [44.0, 118.0, 176.0, 88.0, 133.0, 197.0, 104.0, 153.0, 80.0, 39.0].into_iter().enumerate()
            {
                let x = 67.0 + at as f32 * 71.0;
                art.fill(&rectangle((x, 570.0 - height, 40.0, height)), if at % 3 == 1 { accent } else { mid }, 1.0);
            }
            ((67.0, 154.0, 706.0, 184.0), (67.0, 338.0, 674.0, 44.0))
        }
        Layout::Mosaic => {
            for (at, (x, y)) in
                [(546.0, 104.0), (665.0, 104.0), (546.0, 223.0), (665.0, 223.0), (665.0, 342.0)].into_iter().enumerate()
            {
                if at % 2 == 0 {
                    art.fill(&rectangle((x, y, 109.0, 109.0)), mid, 1.0);
                } else {
                    art.fill(&oval((x + 54.0, y + 54.0), 54.0, 54.0), accent, 1.0);
                }
            }
            ((67.0, 180.0, 452.0, 277.0), (67.0, 530.0, 665.0, 44.0))
        }
        Layout::Cutout => {
            art.fill(&polygon(&[(422.0, 318.0), (783.0, 220.0), (783.0, 560.0), (576.0, 560.0)]), mid, 1.0);
            art.fill(&polygon(&[(557.0, 346.0), (732.0, 294.0), (637.0, 511.0)]), accent, 0.95);
            ((67.0, 126.0, 663.0, 200.0), (67.0, 534.0, 365.0, 44.0))
        }
        Layout::Orbit => {
            art.line(&oval((599.0, 354.0), 179.0, 94.0), mid, 18.0, 1.0);
            art.line(&oval((599.0, 354.0), 130.0, 65.0), accent, 5.0, 1.0);
            art.fill(&oval((599.0, 354.0), 35.0, 35.0), accent, 1.0);
            art.fill(&oval((748.0, 301.0), 19.0, 19.0), accent, 1.0);
            ((67.0, 172.0, 440.0, 258.0), (67.0, 536.0, 679.0, 44.0))
        }
        Layout::Prism => {
            art.fill(&polygon(&[(546.0, 89.0), (768.0, 369.0), (477.0, 369.0)]), mid, 1.0);
            art.fill(&polygon(&[(546.0, 89.0), (768.0, 369.0), (596.0, 345.0)]), accent, 0.86);
            art.line(&polyline(&[(472.0, 395.0), (754.0, 395.0)], false), accent, 6.0, 0.75);
            ((67.0, 262.0, 384.0, 222.0), (67.0, 535.0, 676.0, 44.0))
        }
        Layout::Fold => {
            art.fill(&polygon(&[(428.0, 423.0), (779.0, 253.0), (779.0, 560.0), (428.0, 560.0)]), mid, 1.0);
            art.fill(&polygon(&[(428.0, 423.0), (779.0, 253.0), (657.0, 508.0)]), accent, 1.0);
            ((67.0, 155.0, 698.0, 234.0), (67.0, 536.0, 336.0, 44.0))
        }
        Layout::Grid => {
            for row in 0..5 {
                for col in 0..5 {
                    let center = (505.0 + col as f32 * 57.0, 178.0 + row as f32 * 67.0);
                    let main = (row + col) % 3 == 0;
                    let radius = if main { 15.0 } else { 8.0 };
                    art.fill(&oval(center, radius, radius), if main { accent } else { mid }, 1.0);
                }
            }
            ((67.0, 165.0, 394.0, 312.0), (67.0, 535.0, 675.0, 44.0))
        }
    };
    art.label(&item.category, (67.0, 68.0, 385.0, 43.0), 28.0, accent);
    art.title(title, 82.0, ink, true);
    art.label(&item.publisher, author, 31.0, ink);
    RgbaImage::from_raw(PREVIEW_WIDTH, height, art.finish(None)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_distinct_plugin_cover_layouts_and_a_stable_selection() {
        let Some(item) = crate::store::bundled_plugin_for_cover_test() else { panic!("a bundled plugin is available") };
        let mut seen = std::collections::HashSet::new();
        let mut icons = std::collections::HashSet::new();
        let mut board = RgbaImage::new(1800, 540);
        for (index, layout) in LAYOUTS.into_iter().enumerate() {
            let picture = draw(&item, layout, PALETTES[index % PALETTES.len()], PREVIEW_HEIGHT);
            assert_eq!(picture.dimensions(), (1200, 900));
            assert!(seen.insert(seed_of(&crate::digest_of(picture.as_raw()))));
            let square = draw(&item, layout, PALETTES[index % PALETTES.len()], PREVIEW_WIDTH);
            assert_eq!(square.dimensions(), (1200, 1200));
            assert!(icons.insert(crate::digest_of(square.as_raw())));
            let small = image::imageops::resize(&picture, 360, 270, image::imageops::FilterType::Triangle);
            image::imageops::replace(&mut board, &small, (index % 5 * 360) as i64, (index / 5 * 270) as i64);
        }
        assert_eq!(plugin_cover(&item), plugin_cover(&item));
        assert_eq!(plugin_icon(&item), plugin_icon(&item));
        if let Some(path) = std::env::var_os("PROCRAFT_DUMP_PLUGIN_COVERS") {
            board.save(path).unwrap();
        }
    }
}
