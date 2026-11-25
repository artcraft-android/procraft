use std::f32::consts::TAU;

use tiny_skia::Path;

use super::super::CoverTemplate::{self, *};
use super::super::surface::{CoverArt, Crop, H, Point, Scan, W, curve, polygon, polyline, rectangle, rotate};

fn piece(art: &mut CoverArt<'_>, scan: &Scan, shape: &Path, index: usize, shade: f32, shadow: Point) {
    art.shadow(shape, shadow, 0.71);
    art.cut(scan, shape, Crop::detail(index), 1.0, shade);
    art.line(shape, 0xfff1dc, 1.1, 0.28);
}

fn ground(art: &mut CoverArt<'_>, scan: &Scan, rgb: u32, opacity: f32) {
    let frame = rectangle((0.0, 0.0, W, H));
    art.fill(&frame, rgb, 1.0);
    art.cut(scan, &frame, Crop::WHOLE, opacity, 0.0);
}

pub(in super::super) fn draw(art: &mut CoverArt<'_>, scans: &[Scan], template: CoverTemplate) {
    let Some(first) = scans.first() else { return };
    let scan = |index| scans.get(index % scans.len()).unwrap_or(first);
    let titled = art.titled();
    match template {
        CutPaperRelief => {
            ground(art, scan(0), 0x766759, 0.56);
            let lift = if titled { 0.0 } else { -56.0 };
            for (i, (a, b, c, d, shade)) in [
                ((-20.0, 375.0), (294.0, 352.0), (290.0, 41.0), (862.0, 100.0), -0.44),
                ((-20.0, 468.0), (281.0, 445.0), (429.0, 101.0), (862.0, 178.0), -0.25),
                ((-20.0, 521.0), (388.0, 513.0), (485.0, 195.0), (862.0, 261.0), -0.12),
                ((-20.0, 569.0), (482.0, 584.0), (542.0, 272.0), (862.0, 338.0), 0.04),
                ((391.0, 655.0), (582.0, 546.0), (704.0, 490.0), (862.0, 536.0), -0.27),
            ]
            .into_iter()
            .enumerate()
            {
                let down = |(x, y): Point| (x, y + lift);
                let edge: Vec<_> =
                    curve(down(a), down(b), down(c), down(d)).chain([(862.0, 658.0), (a.0, 658.0)]).collect();
                piece(art, scan(i + 1), &polygon(&edge), i + 1, shade, (-2.0, -8.0));
            }
            art.scan_title((49.0, 64.0, 368.0, 182.0), 78.0, 0x332b27, false);
        }
        PaperPoster => {
            ground(art, scan(0), 0xbbada0, if titled { 0.82 } else { 0.42 });
            let shift = if titled { 176.0 } else { -11.0 };
            for (i, (points, shade)) in [
                ([(327.0, -19.0), (729.0, 32.0), (676.0, 551.0), (263.0, 501.0)], -0.16),
                ([(137.0, 62.0), (553.0, 10.0), (621.0, 555.0), (205.0, 601.0)], -0.06),
                ([(310.0, 126.0), (711.0, 201.0), (644.0, 656.0), (229.0, 587.0)], 0.01),
            ]
            .into_iter()
            .enumerate()
            {
                let points = points.map(|(x, y)| (x + shift, y));
                piece(art, scan(i + 1), &polygon(&points), i, shade, (7.0, 10.0));
            }
            art.scan_title((45.0, 154.0, 347.0, 323.0), 117.0, 0x373432, true);
        }
        FoldedFan => {
            ground(art, scan(0), 0x394840, 0.13);
            let center = if titled { (608.0, 593.0) } else { (446.0, 578.0) };
            for i in 0..7 {
                let a = if titled { 3.24 } else { 3.13 } + i as f32 * 0.414;
                let b = a + 0.446;
                let distance = 482.0 + (i % 3) as f32 * 31.0;
                let left = (center.0 + distance * a.cos(), center.1 + distance * a.sin());
                let right = (center.0 + (distance + 75.0) * b.cos(), center.1 + (distance + 75.0) * b.sin());
                let middle = (left.0 * 0.49 + right.0 * 0.51, left.1 * 0.49 + right.1 * 0.51);
                let material = scan(i);
                let face = polygon(&[center, left, right]);
                piece(art, material, &face, i, -0.07 - (i % 3) as f32 * 0.06, (5.0, 9.0));
                art.fill(&polygon(&[center, left, middle]), 0x221c18, 0.21);
                art.line(&polyline(&[center, middle], false), 0xf0dfc3, 1.3, 0.51);
            }
            art.scan_title((46.0, 50.0, 279.0, 182.0), 75.0, 0xf1e5d2, false);
        }
        DeckledImpressions => {
            ground(art, scan(0), 0x5a4545, 0.20);
            let x = if titled { 545.0 } else { 420.0 };
            for (i, (radius, dy, shade)) in [
                (308.0, -31.0, -0.29),
                (275.0, -19.0, -0.22),
                (244.0, -6.0, -0.14),
                (208.0, 9.0, -0.07),
                (173.0, 22.0, 0.03),
            ]
            .into_iter()
            .enumerate()
            {
                let points: Vec<_> = (0..=220)
                    .map(|j| {
                        let angle = j as f32 * TAU / 220.0;
                        let j = j as f32;
                        let r = radius + 3.6 * (j * 1.63 + i as f32 * 4.0).sin() + 6.4 * (j * 0.31).sin();
                        (x + r * angle.cos(), 314.0 + dy + r * angle.sin() * 0.91)
                    })
                    .collect();
                piece(art, scan(i), &polygon(&points), i, shade, (3.0, 8.0));
            }
            art.scan_title((44.0, 152.0, 192.0, 300.0), 60.0, 0xffefdc, false);
        }
        PaperLoom => {
            ground(art, scan(0), 0x304045, 0.10);
            let top = if titled { 146.0 } else { -12.0 };
            let columns = [23.0, 158.0, 293.0, 428.0, 563.0, 698.0];
            for (i, x) in columns.into_iter().enumerate() {
                let strip = polygon(&[(x, top), (x + 94.0, top - 2.0), (x + 114.0, H + 8.0), (x + 20.0, H + 8.0)]);
                piece(art, scan(i), &strip, i, -0.15 - (i % 3) as f32 * 0.06, (4.0, 6.0));
            }
            for i in 0..if titled { 5 } else { 7 } {
                let y = top + 46.0 + i as f32 * 87.0;
                let strip = polygon(&[(0.0, y), (W, y + 23.0), (W, y + 80.0), (0.0, y + 57.0)]);
                piece(art, scan(i + 1), &strip, i + 6, if i % 2 == 0 { -0.06 } else { 0.04 }, (3.0, 6.0));
                for (j, x) in columns.into_iter().enumerate().filter(|(j, _)| (i + j) % 2 == 0) {
                    let pull = 20.0 * ((y - top) / (H - top)).min(1.0);
                    let x = x + pull;
                    let y = y + x / W * 23.0;
                    let over = polygon(&[(x, y - 1.0), (x + 94.0, y + 1.0), (x + 96.0, y + 63.0), (x + 2.0, y + 60.0)]);
                    art.shadow(&over, (2.0, 2.0), 0.43);
                    art.cut(scan(j), &over, Crop::detail(j), 1.0, -0.15 - (j % 3) as f32 * 0.06);
                }
            }
            art.scan_title((47.0, 36.0, 748.0, 99.0), 67.0, 0xf2e6d1, false);
        }
        ConcentricCuts => {
            ground(art, scan(0), 0x382f35, 0.17);
            let (x, size) = if titled { (554.0, 477.0) } else { (413.0, 552.0) };
            for i in 0..8 {
                let side = (size - i as f32 * 53.0) * 0.5;
                let points = [(-side, -side), (side, -side), (side, side), (-side, side)]
                    .map(|point| rotate(point, (x, 321.0), 14.0 + i as f32 * 7.0));
                piece(art, scan(i), &polygon(&points), i, -0.28 + 0.042 * i as f32, (6.0, 8.0));
            }
            art.scan_title((42.0, 205.0, 220.0, 218.0), 65.0, 0xf5e3cb, false);
        }
        SheetEdges => {
            ground(art, scan(0), 0x534b41, 0.33);
            for i in 0..8 {
                let y = if titled { 151.0 + i as f32 * 56.0 } else { 34.0 + i as f32 * 69.0 };
                let edge: Vec<_> =
                    curve((-14.0, y + 139.0), (251.0, y + 147.0), (648.0, y - 87.0), (854.0, y - 65.0)).collect();
                let shape: Vec<_> = edge.iter().copied().chain([(854.0, H + 9.0), (-14.0, H + 9.0)]).collect();
                piece(art, scan(i), &polygon(&shape), i, -0.37 + i as f32 * 0.050, (-2.0, -6.0));
                art.line(&polyline(&edge, false), 0xffead0, 1.0, 0.56);
            }
            art.scan_title((46.0, 47.0, 484.0, 164.0), 73.0, 0xf2e7d7, false);
        }
        LightTable => {
            ground(art, scan(0), 0x26364e, 0.13);
            let shift = if titled { 91.0 } else { -4.0 };
            for (i, points) in [
                [(151.0, 135.0), (502.0, 68.0), (577.0, 449.0), (226.0, 516.0)],
                [(271.0, 214.0), (681.0, 194.0), (699.0, 558.0), (289.0, 581.0)],
                [(360.0, 124.0), (659.0, 49.0), (759.0, 434.0), (451.0, 513.0)],
                [(123.0, 397.0), (455.0, 304.0), (511.0, 479.0), (174.0, 568.0)],
            ]
            .into_iter()
            .enumerate()
            {
                let shape = polygon(&points.map(|(x, y)| (x + shift, y)));
                art.shadow(&shape, (4.0, 6.0), 0.24);
                art.cut(scan(i), &shape, Crop::detail(i), 0.39 + i as f32 * 0.045, 0.0);
                art.line(&shape, 0xfaefd9, 1.65, 0.60);
            }
            art.scan_title((42.0, 48.0, 245.0, 178.0), 62.0, 0xf6e7d0, false);
        }
        PaperMosaic => {
            ground(art, scan(0), 0x393736, 0.08);
            let top = if titled { 143.0 } else { -14.0 };
            for row in 0..if titled { 4 } else { 5 } {
                for col in 0..6 {
                    let x = col as f32 * 173.0 - 54.0 + (row % 2) as f32 * 79.0;
                    let y = top + row as f32 * 156.0;
                    let shift = if (col + row) % 2 == 0 { 18.0 } else { -17.0 };
                    let shape = polygon(&[
                        (x + 8.0, y + 8.0),
                        (x + 166.0, y + 13.0 + shift),
                        (x + 161.0, y + 154.0),
                        (x + 14.0, y + 151.0 + shift),
                    ]);
                    let i = row * 6 + col;
                    piece(art, scan(i), &shape, i, -0.05 * ((i * 3) % 5) as f32, (3.0, 5.0));
                }
            }
            art.scan_title((49.0, 38.0, 737.0, 104.0), 69.0, 0xf6e7d0, false);
        }
        ArchiveTabs => {
            ground(art, scan(0), 0x918373, 0.45);
            for i in 0..5 {
                let x = if titled { -12.0 + i as f32 * 122.0 } else { 26.0 + i as f32 * 119.0 };
                let y = if titled { 158.0 + i as f32 * 60.0 } else { 52.0 + i as f32 * 65.0 };
                let shape = polygon(&[
                    (x, y + 56.0),
                    (x + 52.0, y + 53.0),
                    (x + 83.0, y + 10.0),
                    (x + 211.0, y + 10.0),
                    (x + 239.0, y + 53.0),
                    (x + 377.0, y + 51.0),
                    (x + 377.0, H + 6.0),
                    (x, H + 6.0),
                ]);
                piece(art, scan(i), &shape, i, -0.25 + i as f32 * 0.065, (5.0, -4.0));
            }
            art.scan_title((49.0, 47.0, 738.0, 111.0), 74.0, 0x392e28, false);
        }
        _ => ground(art, first, 0x807364, 1.0),
    }
}
