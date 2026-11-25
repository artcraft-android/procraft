use std::f32::consts::TAU;

use super::super::CoverTemplate::{self, *};
use super::super::ink::{BrushMarks, Role::*};
use super::super::surface::{CoverArt, H, W, curve, oval, polygon, polyline, rectangle, rotate};

pub(in super::super) fn draw(art: &mut CoverArt<'_>, marks: &BrushMarks, template: CoverTemplate) {
    let titled = art.titled();
    let tap = marks.primary(Stamp);
    let scatter = marks.support(Scatter, Stamp);
    let bold = marks.primary(Bold);
    let fine = marks.support(Fine, Bold);
    let wash = marks.support(Wash, Bold);
    let texture = marks.support(Grain, Bold);
    match template {
        BotanicalPrint => {
            art.stock(0xd6d0a7);
            let shift = if titled { 80.0 } else { 0.0 };
            let (dark, leaf) = (0x3e5946, 0x859355);
            art.mark(scatter, (485.0, 365.0 + shift, 700.0, 525.0), 74.0, dark, 0.27);
            let blooms = [
                ((254.0, 195.0), (277.0, 663.0), 272.0, 0xb7432d, 24.0),
                ((567.0, 154.0), (434.0, 665.0), 300.0, 0xd35b38, -23.0),
                ((178.0, 376.0), (358.0, 655.0), 147.0, 0xc45c39, 14.0),
                ((633.0, 357.0), (542.0, 668.0), 160.0, 0xac3e32, 32.0),
            ];
            for (i, ((x, y), root, size, ink, angle)) in blooms.into_iter().enumerate() {
                let y = y + shift * 0.76;
                let stem: Vec<_> = curve(root, (root.0 - 20.0, 472.0 + shift), (x + 45.0, y + 95.0), (x, y)).collect();
                art.line(&polyline(&stem, false), dark, 5.0 - i as f32 * 0.45, 1.0);
                for (j, a) in [15, 23].into_iter().filter_map(|at| stem.get(at).copied()).enumerate() {
                    let sign = if (i + j) % 2 == 0 { -1.0 } else { 1.0 };
                    let tip = (a.0 + sign * (105.0 - 14.0 * i as f32), a.1 - 81.0 + 9.0 * j as f32);
                    let body: Vec<_> =
                        curve(a, (a.0 + sign * 15.0, a.1 - 61.0), (tip.0 - sign * 19.0, tip.1 - 4.0), tip)
                            .chain(curve(tip, (tip.0 - sign * 11.0, a.1), (a.0 + sign * 21.0, a.1 + 13.0), a))
                            .collect();
                    art.fill(&polygon(&body), if sign > 0.0 { dark } else { leaf }, 0.96);
                    art.line(&polyline(&[a, (tip.0 * 0.91 + a.0 * 0.09, tip.1 + 9.0)], false), 0xaab477, 1.1, 0.65);
                }
                art.mark(tap, (x, y, size, size), angle, ink, 0.96);
                art.mark(scatter.or(tap), (x + 9.0, y + 15.0, size * 0.38, size * 0.36), -angle, 0x523134, 0.95);
            }
            if titled {
                art.fill(&rectangle((0.0, 0.0, W, 105.0)), 0xd6d0a7, 0.93);
                art.line(&polyline(&[(49.0, 105.0), (791.0, 105.0)], false), 0x74845c, 1.4, 0.88);
            }
            art.title((48.0, 32.0, 745.0, 68.0), 55.0, 0x333e30, false);
        }
        ImpactPoster => {
            art.stock(0xda5440);
            let (x, y, radius) = if titled { (576.0, 330.0, 276.0) } else { (420.0, 313.0, 318.0) };
            art.fill(&oval((x, y - 23.0), radius, radius), 0xf0c46c, 0.94);
            art.mark(tap, (x + 6.0, y, radius * 2.34, radius * 2.20), 24.0, 0x25202a, 1.0);
            art.mark(scatter, (x + 145.0, y + 175.0, 264.0, 255.0), -11.0, 0x25202a, 0.91);
            art.title((48.0, 52.0, 339.0, 265.0), 92.0, 0x291e28, true);
        }
        Constellation => {
            art.stock(0x151d34);
            let center = if titled { (464.0, 335.0) } else { (414.0, 311.0) };
            let orbit: Vec<_> = (0..=120)
                .map(|i| {
                    let angle = i as f32 * TAU / 120.0;
                    rotate((338.0 * angle.cos(), 162.0 * angle.sin()), center, -24.0)
                })
                .collect();
            art.line(&polyline(&orbit, true), 0x576b8e, 2.0, 0.78);
            let (x, y) = center;
            let span = if titled { 370.0 } else { 432.0 };
            art.mark(tap, (x, y, span, span), -21.0, 0x8fc6dc, 0.98);
            for (dx, dy, size, ink, angle) in [
                (211.0, -111.0, 188.0, 0xedd898, 43.0),
                (-224.0, 85.0, 226.0, 0x869fbe, 74.0),
                (-202.0, 111.0, 95.0, 0xf2c481, 4.0),
                (166.0, 150.0, 163.0, 0x709cc9, 18.0),
            ] {
                art.mark(scatter.or(tap), (x + dx, y + dy, size, size), angle, ink, 0.91);
            }
            art.title((48.0, 42.0, 332.0, 138.0), 49.0, 0xf1debb, false);
        }
        BlockPrint => {
            art.stock(0xeddbac);
            let colors = [0x2c6860, 0xaa4934, 0x212d42, 0xb98235];
            for row in 0..if titled { 3 } else { 4 } {
                for column in 0..7 {
                    let i = (column + row * 3) % 4;
                    let x = (column as f32 - 0.03) * 143.0 + if row % 2 == 0 { 45.0 } else { -14.0 };
                    let y = if titled { 238.0 + row as f32 * 148.0 } else { 70.0 + row as f32 * 163.0 };
                    let arc: Vec<_> = (0..=21)
                        .map(|step| {
                            let a = 1.5 * (step as f32 / 21.0 - 0.5) + i as f32 * 1.55;
                            (x + 54.0 * a.cos(), y + 57.0 * a.sin())
                        })
                        .collect();
                    let color = colors.get(i).copied().unwrap_or(0x2c6860);
                    art.line(&polyline(&arc, false), color, 1.8, 0.88);
                    art.mark(
                        if i % 2 == 0 { tap } else { scatter.or(tap) },
                        (x, y, 116.0, 108.0),
                        (row + column) as f32 * 27.0,
                        color,
                        0.94,
                    );
                }
            }
            if titled {
                art.line(&rectangle((38.0, 36.0, 764.0, 128.0)), 0x6c5644, 2.0, 0.87);
            }
            art.title((66.0, 59.0, 708.0, 85.0), 54.0, 0x254139, false);
        }
        Overprint => {
            art.stock(0xeddbd1);
            let x = if titled { 491.0 } else { 420.0 };
            let scale = if titled { 1.0 } else { 1.16 };
            for (i, (dx, dy, size, rgb, a)) in [
                (-111.0, 79.0, 474.0, 0x0b8f9c, 10.0),
                (106.0, 72.0, 525.0, 0xdd415b, -41.0),
                (134.0, -115.0, 386.0, 0xd28b22, 27.0),
                (21.0, 17.0, 316.0, 0x443250, 5.0),
            ]
            .into_iter()
            .enumerate()
            {
                art.mark(
                    if i == 1 || i == 2 { scatter.or(tap) } else { tap },
                    (x + dx, 314.0 + dy, size * scale, size * scale),
                    a,
                    rgb,
                    0.84,
                );
            }
            if titled {
                art.fill(&rectangle((43.0, 51.0, 300.0, 141.0)), 0x443250, 0.96);
            }
            art.title((59.0, 69.0, 269.0, 106.0), 44.0, 0xf7e6d8, true);
        }
        RisingPlume => {
            art.stock(0x172f32);
            let colors = [0x3e6964, 0x54817c, 0x76a195, 0xa2bda0, 0xd8d8ab];
            for (i, (x, y)) in [
                (633.0, 562.0),
                (547.0, 500.0),
                (474.0, 410.0),
                (423.0, 352.0),
                (427.0, 274.0),
                (341.0, 218.0),
                (277.0, 158.0),
                (224.0, 85.0),
            ]
            .into_iter()
            .enumerate()
            {
                let span = if titled { 444.0 } else { 505.0 } - i as f32 * 32.0;
                art.mark(
                    if i < 4 { tap } else { scatter.or(tap) },
                    (x + if titled { 62.0 } else { -11.0 }, y, span, span * 0.91),
                    47.0 + i as f32 * 22.0,
                    colors.get(i).copied().unwrap_or(0xd8d8ab),
                    0.64 + 0.045 * i as f32,
                );
            }
            art.title((50.0, 465.0, 347.0, 118.0), 61.0, 0xf3e6ba, false);
        }
        Eclipse => {
            art.stock(0x2b252b);
            let (x, y, r) = if titled { (544.0, 299.0, 173.0) } else { (419.0, 312.0, 224.0) };
            for (i, color) in [0xa65c4e, 0xd08255, 0xf3c278].into_iter().cycle().take(12).enumerate() {
                let a = i as f32 * TAU / 12.0;
                art.mark(
                    if i % 3 == 0 { tap } else { scatter.or(tap) },
                    (x + r * 0.94 * a.cos(), y + r * 0.94 * a.sin(), 232.0, 215.0),
                    i as f32 * 31.0,
                    color,
                    0.89,
                );
            }
            art.fill(&oval((x, y), r * 0.78, r * 0.78), 0x54363d, 0.98);
            art.fill(&oval((x + 11.0, y - 16.0), r * 0.74, r * 0.74), 0x2a242b, 0.97);
            art.title((47.0, 239.0, 276.0, 174.0), 48.0, 0xf5d59b, false);
        }
        SplitInk => {
            let (light, dark) = (0xf0d8ba, 0x272843);
            art.stock(light);
            let mut reverse = art.plate();
            reverse.stock(dark);
            let x = if titled { 484.0 } else { 410.0 };
            for (mark, area, angle, color) in [
                (tap, (x, 326.0, 711.0, 705.0), -8.0, 0xe8bd9e),
                (scatter, (x + 210.0, 458.0, 269.0, 259.0), 17.0, 0xf0d8ba),
                (scatter, (155.0, 404.0, 229.0, 221.0), 63.0, 0xb094ad),
            ] {
                art.mark(mark, area, angle, dark, 0.98);
                reverse.mark(mark, area, angle, color, 0.98);
            }
            let rise = if titled { 115.0 } else { 186.0 };
            art.layer(&reverse, &polygon(&[(0.0, 434.0), (W, rise), (W, H), (0.0, H)]), (0.0, 0.0));
            art.title((47.0, 39.0, 332.0, 127.0), 47.0, dark, true);
        }
        OffsetStrips => {
            art.stock(0xefb46c);
            let mut proof = art.plate();
            proof.stock(0x252637);
            proof.mark(tap, (466.0, 321.0, 758.0, 701.0), 31.0, 0xe6dfaf, 0.98);
            proof.mark(scatter, (660.0, 478.0, 235.0, 251.0), 16.0, 0xe8a36b, 0.88);
            let offsets = if titled { [317.0, 91.0, -36.0, 127.0] } else { [122.0, -61.0, 76.0, -98.0] };
            for ((top, height), dx) in
                [(0.0, 150.0), (162.0, 163.0), (337.0, 132.0), (481.0, 149.0)].into_iter().zip(offsets)
            {
                art.layer(&proof, &rectangle((0.0, top, W, height)), (dx, 0.0));
            }
            art.title((39.0, 27.0, 252.0, 108.0), 39.0, 0x3e2630, true);
        }
        InkWindow => {
            art.stock(0xd6dfc2);
            let mut field = art.plate();
            field.stock(0x584f96);
            let (x, span) = if titled { (535.0, 237.0) } else { (420.0, 308.0) };
            field.mark(tap, (x - 28.0, 295.0, 770.0, 680.0), -34.0, 0xf8df83, 1.0);
            field.mark(scatter, (x + 126.0, 419.0, 288.0, 267.0), 41.0, 0xf0a893, 0.94);
            let opening: Vec<_> =
                curve((x - span, 558.0), (x - span - 13.0, -30.0), (x + span + 13.0, -30.0), (x + span, 558.0))
                    .collect();
            let gate = polygon(&opening);
            art.shadow(&gate, (6.0, 12.0), 0.52);
            art.layer(&field, &gate, (0.0, 0.0));
            art.title((43.0, 406.0, 227.0, 151.0), 44.0, 0x373653, false);
        }
        CoastalPrint => {
            art.stock(0xefdabc);
            let dark = 0x263a3c;
            let (sun_x, sun_y) = if titled { (648.0, 203.0) } else { (582.0, 185.0) };
            art.fill(&oval((sun_x, sun_y), 122.0, 122.0), 0xc85b3e, 0.96);
            for i in 0..11 {
                let f = (i as f32 + 1.0) / 13.0;
                let half = 118.0 * (1.0 - f * f).sqrt();
                let y = sun_y + 122.0 * f;
                art.line(&polyline(&[(sun_x - half, y), (sun_x + half, y)], false), 0xe3a177, 1.5 + f * 2.0, 0.85);
            }
            let shore = [
                (-15.0, 359.0),
                (118.0, 361.0),
                (220.0, 341.0),
                (353.0, 390.0),
                (544.0, 421.0),
                (695.0, 453.0),
                (851.0, 473.0),
                (851.0, H),
                (-15.0, H),
            ];
            art.mark(wash.or(bold), (295.0, 330.0, 872.0, 266.0), 7.0, 0x828575, 0.48);
            art.fill(&polygon(&shore), dark, 0.99);
            art.print(bold, (266.0, 374.0, 756.0, 163.0), 8.0, dark, 0.98, true);
            art.mark(wash, (576.0, 470.0, 765.0, 174.0), 9.0, 0x9baaa0, 0.80);
            art.mark(fine.or(bold), (572.0, 496.0, 658.0, 64.0), 8.0, 0xf2d19e, 0.90);
            art.mark(texture, (257.0, 535.0, 635.0, 100.0), 1.0, 0x84958b, 0.88);
            art.title((53.0, 62.0, 424.0, 207.0), 79.0, 0x25383a, false);
        }
        GraphicPoster => {
            art.stock(0x253c88);
            let cut = if titled { 585.0 } else { 528.0 };
            art.fill(&polygon(&[(cut, -3.0), (W, -3.0), (W, H), (cut - 533.0, H)]), 0xd4d789, 0.96);
            let (x, y) = if titled { (575.0, 386.0) } else { (425.0, 344.0) };
            art.mark(texture, (x + 32.0, y - 128.0, 1160.0, 299.0), -54.0, 0x1b2a51, 0.83);
            art.mark(bold, (x, y, 1150.0, 225.0), -49.0, 0x131d2a, 0.99);
            art.print(fine, (x + 36.0, y + 36.0, 1046.0, 94.0), -49.0, 0xe9e3a9, 0.96, true);
            art.title((48.0, 60.0, 362.0, 282.0), 121.0, 0xf0e8c5, true);
        }
        CalligraphicGesture => {
            art.stock(0xeadac5);
            let x = if titled { 545.0 } else { 406.0 };
            let span = if titled { 663.0 } else { 778.0 };
            art.mark(wash, (x + 15.0, 329.0, span, 247.0), -35.0, 0xc59185, 0.7);
            art.print(bold, (x, 318.0, span, 202.0), -39.0, 0x2b2122, 0.98, true);
            art.mark(fine.or(bold), (x - 28.0, 407.0, span * 0.88, 81.0), -11.0, 0x3b2928, 0.94);
            art.mark(texture, (x + 50.0, 414.0, span * 0.72, 121.0), -51.0, 0x9b4436, 0.91);
            art.title((46.0, 70.0, 318.0, 175.0), 69.0, 0x2b2122, false);
        }
        WashFields => {
            art.stock(0xf1d9ca);
            let shift = if titled { 79.0 } else { 0.0 };
            let wash = marks.primary(Wash).or(bold);
            let wash_accent = marks.support(Wash, Wash).or(wash);
            for (i, (x, y, span, angle, rgb)) in [
                (245.0, 361.0, 744.0, 28.0, 0xe97763),
                (581.0, 424.0, 739.0, -22.0, 0x413477),
                (623.0, 379.0, 818.0, -42.0, 0x753557),
                (285.0, 261.0, 631.0, 43.0, 0xc46c76),
                (519.0, 259.0, 676.0, -53.0, 0x765a9a),
                (373.0, 194.0, 583.0, 18.0, 0xda8c76),
            ]
            .into_iter()
            .enumerate()
            {
                let accent = i == 1 || i == 3;
                let mark = if accent { wash_accent } else { wash };
                art.print(mark, (x + shift, y, span, 399.0), angle, rgb, 0.97, i % 2 != 0);
            }
            art.title((48.0, 39.0, 359.0, 140.0), 72.0, 0x552e4b, false);
        }
        InkCurrent => {
            art.stock(0x163237);
            let y = if titled { 275.0 } else { 310.0 };
            let span = if titled { 1030.0 } else { 1130.0 };
            art.mark(wash, (366.0, y + 38.0, 920.0, 245.0), -17.0, 0x507a77, 0.79);
            art.mark(texture, (455.0, y - 97.0, 850.0, 165.0), -15.0, 0x84a79d, 0.91);
            art.mark(bold, (421.0, y, span, 229.0), -17.0, 0xe4b98c, 0.99);
            art.print(fine, (414.0, y + 101.0, 888.0, 73.0), -14.0, 0x74b8b6, 0.99, true);
            art.title((52.0, 455.0, 726.0, 139.0), 76.0, 0xe6e5cc, false);
        }
        InkArchitecture => {
            art.stock(0xd7ae78);
            let base = if titled { 572.0 } else { 527.0 };
            let colors = [0x375858, 0x292f37, 0xa84f3d, 0xece0b4, 0x334339];
            for (i, color) in colors.into_iter().cycle().take(13).enumerate() {
                let x = (i as f32 - 0.5) * 67.0 + if titled { 75.0 } else { 30.0 };
                let height = 120.0 + ((i * 127) % 309) as f32;
                let width = 43.0 + ((i * 37) % 53) as f32;
                art.fill(&rectangle((x - width * 0.5, base - height, width, height)), color, 0.93);
                art.print(
                    if i % 3 == 0 { texture.or(bold) } else { bold },
                    (x, base - height * 0.5, height + 45.0, width + 24.0),
                    90.0,
                    if i % 5 == 1 { 0x9c9e85 } else { 0x282d2b },
                    0.78,
                    i % 2 == 1,
                );
            }
            art.line(&polyline(&[(0.0, base + 6.0), (W, base + 6.0)], false), 0x393a35, 2.0, 0.91);
            art.title((44.0, 34.0, 549.0, 99.0), 61.0, 0x2f3331, true);
        }
        HalftoneHorizon => {
            art.stock(0xf3cc82);
            let (x, r) = if titled { (564.0, 217.0) } else { (421.0, 258.0) };
            art.fill(&oval((x, 275.0), r, r), 0xbe5147, 0.98);
            let dotted = marks.primary(Grain).or(bold);
            for (i, color) in [0x415071, 0x2a3b53, 0x2f354b].into_iter().enumerate() {
                art.mark(dotted, (453.0 - 38.0 * i as f32, 357.0 + 52.0 * i as f32, 1080.0, 199.0), -6.0, color, 0.88);
            }
            art.mark(bold, (404.0, 568.0, 1143.0, 187.0), -5.0, 0x303141, 0.97);
            art.mark(fine, (485.0, 574.0, 877.0, 80.0), -6.0, 0xe4aa5c, 0.90);
            art.title((42.0, 44.0, 335.0, 143.0), 57.0, 0x333343, true);
        }
        InkWeave => {
            art.stock(0xdbddd4);
            let palette = [0x944f43, 0xce8454, 0x2d5b61, 0x718f7c, 0x2d3b47];
            let rows = if titled { 4 } else { 5 };
            for row in 0..rows {
                for column in 0..6 {
                    let x = -8.0 + column as f32 * 169.0 + (row % 2) as f32 * 81.0;
                    let y = if titled { 179.0 } else { 55.0 } + row as f32 * 128.0;
                    let odd = (row + column) % 2 == 0;
                    art.print(
                        if odd { bold } else { texture.or(fine).or(bold) },
                        (x, y, 193.0, 87.0),
                        if odd { 43.0 } else { -43.0 },
                        palette.get((row * 2 + column) % 5).copied().unwrap_or(0x2d3b47),
                        0.88,
                        !odd,
                    );
                }
            }
            if titled {
                let slip = polygon(&[(0.0, 0.0), (395.0, 0.0), (320.0, 155.0), (0.0, 155.0)]);
                art.shadow(&slip, (7.0, 9.0), 0.38);
                art.fill(&slip, 0xf0e5d4, 0.98);
            }
            art.title((40.0, 27.0, 271.0, 107.0), 58.0, 0x2d3b47, false);
        }
        SplitRegister => {
            art.stock(0xcbd0b2);
            let mut stripe = art.plate();
            stripe.stock(0x343b57);
            let x = if titled { 513.0 } else { 419.0 };
            let width = if titled { 323.0 } else { 263.0 };
            for (mark, area, a, outside, inside) in [
                (bold, (x, 342.0, 1220.0, 245.0), -23.0, 0x304a48, 0xe2bf83),
                (texture, (x - 37.0, 269.0, 959.0, 189.0), -18.0, 0x58695a, 0xba969b),
                (fine, (x + 37.0, 415.0, 1110.0, 113.0), -19.0, 0xa97043, 0xaac4c0),
            ] {
                art.mark(mark, area, a, outside, 0.97);
                stripe.mark(mark, area, a, inside, 0.96);
            }
            art.layer(&stripe, &rectangle((x - width * 0.5, 0.0, width, H)), (0.0, 0.0));
            art.title((46.0, 47.0, 268.0, 145.0), 56.0, 0x2e403f, false);
        }
        TornProofs => {
            art.stock(0x393345);
            let center = if titled { 482.0 } else { 420.0 };
            for (i, ((x, y, angle, rgb, stock), mark)) in [
                (-141.0, 139.0, -13.0, 0x3c2c32, 0xb99a92),
                (66.0, 250.0, 12.0, 0x25362e, 0xbac8a3),
                (-65.0, 385.0, -12.0, 0x4f251f, 0xe2bf97),
                (128.0, 489.0, 10.0, 0x302b43, 0xc5aec3),
            ]
            .into_iter()
            .zip([bold, texture.or(bold), fine.or(bold), wash.or(bold)])
            .enumerate()
            {
                let (x, y) = (x + center, y + if titled { 45.0 } else { 0.0 });
                let points: Vec<_> = [
                    (-215.0, -73.0),
                    (-179.0, -79.0),
                    (-111.0, -71.0),
                    (-31.0, -83.0),
                    (9.0, -73.0),
                    (94.0, -85.0),
                    (199.0, -72.0),
                    (218.0, 57.0),
                    (127.0, 73.0),
                    (64.0, 79.0),
                    (9.0, 70.0),
                    (-55.0, 76.0),
                    (-128.0, 70.0),
                    (-197.0, 77.0),
                    (-219.0, 28.0),
                ]
                .into_iter()
                .map(|point| rotate(point, (x, y), angle))
                .collect();
                let piece = polygon(&points);
                let mut proof = art.plate();
                proof.stock(stock);
                proof.print(mark, (x, y, 467.0, 145.0), angle, rgb, 0.96, i % 2 == 1);
                art.shadow(&piece, (6.0, 8.0), 0.59);
                art.layer(&proof, &piece, (0.0, 0.0));
            }
            art.title((43.0, 29.0, 495.0, 93.0), 52.0, 0xf4dfc8, false);
        }
        _ => super::fallback(art),
    }
}
