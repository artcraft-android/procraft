//! HDR pixels: what an HDR canvas's layers hold (premultiplied RGBA, half floats, sRGB-encoded
//! and carried on past white), to and from 8-bit pixels, tone-mapped to a standard picture, and
//! written out as an OpenEXR file.

/// Bytes an HDR pixel takes: four half floats.
pub const HDR_PIXEL: usize = 8;

/// Where the tone map's shoulder begins, a share of white: below it a colour is left as it is.
pub const KNEE: f32 = 0.8;

/// A half float (IEEE 754 binary16) as a float.
pub fn half_to_f32(h: u16) -> f32 {
    let sign = u32::from(h >> 15) << 31;
    let exp = u32::from((h >> 10) & 0x1f);
    let man = u32::from(h & 0x3ff);
    let bits = match (exp, man) {
        (0, 0) => sign,
        // subnormal: normalised
        (0, m) => {
            let shift = m.leading_zeros() - 21;
            sign | ((113 - shift) << 23) | (((m << shift) & 0x3ff) << 13)
        }
        (0x1f, 0) => sign | 0x7f80_0000,
        (0x1f, m) => sign | 0x7fc0_0000 | (m << 13),
        (e, m) => sign | ((e + 112) << 23) | (m << 13),
    };
    f32::from_bits(bits)
}

/// A float as the nearest half float (ties to even); past the largest, infinity.
pub fn f32_to_half(f: f32) -> u16 {
    let bits = f.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let man = bits & 0x7f_ffff;
    if exp == 0xff {
        return sign | 0x7c00 | if man != 0 { 0x200 } else { 0 };
    }
    let e = exp - 127 + 15;
    if e >= 0x1f {
        return sign | 0x7c00;
    }
    if e <= 0 {
        // subnormal (or zero)
        if e < -10 {
            return sign;
        }
        let m = man | 0x80_0000;
        let shift = (14 - e) as u32;
        let half = m >> shift;
        let rest = m & ((1 << shift) - 1);
        let mid = 1 << (shift - 1);
        let round = u32::from(rest > mid || (rest == mid && half & 1 == 1));
        return sign | (half + round) as u16;
    }
    let half = ((e as u32) << 10) | (man >> 13);
    let rest = man & 0x1fff;
    let round = u32::from(rest > 0x1000 || (rest == 0x1000 && half & 1 == 1));
    // a carry out of the mantissa steps the exponent up, as it should
    sign | (half + round) as u16
}

/// HDR pixels' channels as floats.
pub fn floats(half: &[u8]) -> impl Iterator<Item = f32> + '_ {
    half.as_chunks::<2>().0.iter().map(|b| half_to_f32(u16::from_le_bytes(*b)))
}

/// Floats as HDR pixels' bytes (little-endian half floats).
pub fn halves(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    values.into_iter().flat_map(|v| f32_to_half(v).to_le_bytes()).collect()
}

/// 8-bit pixels as HDR ones, as they are.
pub fn widen(rgba: &[u8]) -> Vec<u8> {
    // every byte's half, once
    let table: Vec<[u8; 2]> = (0..=255u8).map(|v| f32_to_half(f32::from(v) / 255.0).to_le_bytes()).collect();
    rgba.iter().flat_map(|&v| table.get(usize::from(v)).copied().unwrap_or_default()).collect()
}

/// HDR pixels as 8-bit ones: light above white cut off at white (each colour kept no brighter
/// than its alpha, so it stays a premultiplied colour).
pub fn narrow(half: &[u8]) -> Vec<u8> {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut out = Vec::with_capacity(half.len() / 2);
    for px in half.as_chunks::<HDR_PIXEL>().0 {
        let mut c = [0.0f32; 4];
        for (to, v) in c.iter_mut().zip(floats(px)) {
            *to = if v.is_finite() { v } else { 0.0 };
        }
        let a = c[3].clamp(0.0, 1.0);
        out.extend_from_slice(&[byte(c[0].min(a)), byte(c[1].min(a)), byte(c[2].min(a)), byte(a)]);
    }
    out
}

/// The tone map's shoulder on an sRGB-encoded value: up to [`KNEE`] as it is, past it eased
/// towards white, which only light far above it reaches.
pub fn shoulder(v: f32) -> f32 {
    if v <= KNEE {
        return v;
    }
    let range = 1.0 - KNEE;
    KNEE + range * (1.0 - (-(v - KNEE) / range).exp())
}

/// HDR pixels tone-mapped to a standard picture (8-bit, premultiplied): each colour's strongest
/// channel through [`shoulder`], the others scaled with it so its hue stays. What is all below
/// the knee comes out as [`narrow`] has it.
pub fn tone_mapped(half: &[u8]) -> Vec<u8> {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut out = Vec::with_capacity(half.len() / 2);
    for px in half.as_chunks::<HDR_PIXEL>().0 {
        let mut c = [0.0f32; 4];
        for (to, v) in c.iter_mut().zip(floats(px)) {
            *to = if v.is_finite() { v.max(0.0) } else { 0.0 };
        }
        let a = c[3].min(1.0);
        if a <= 0.0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        let straight = [c[0] / a, c[1] / a, c[2] / a];
        let m = straight[0].max(straight[1]).max(straight[2]);
        let k = if m > KNEE { shoulder(m) / m } else { 1.0 };
        let [r, g, b] = straight.map(|v| (v * k * a).min(a));
        out.extend_from_slice(&[byte(r), byte(g), byte(b), byte(a)]);
    }
    out
}

/// The sRGB curve undone, carried on past white (and mirrored below black).
pub fn linear(v: f32) -> f32 {
    let m = v.abs();
    let l = if m <= 0.04045 { m / 12.92 } else { ((m + 0.055) / 1.055).powf(2.4) };
    l.copysign(v)
}

/// The sRGB curve, carried on past white (and mirrored below black): [`linear`] undone.
pub fn encoded(v: f32) -> f32 {
    let m = v.abs();
    let e = if m <= 0.003_130_8 { m * 12.92 } else { 1.055 * m.powf(1.0 / 2.4) - 0.055 };
    e.copysign(v)
}

/// The most stops above white a colour is taken to: past half floats' reach, a colour is
/// no longer one.
pub const MOST_STOPS: f32 = 15.0;

/// A colour (sRGB-encoded, carried on past white) as a colour within white and how many stops
/// above white it is lit: its brightest channel, in linear light, brought down to white. A colour
/// within white is itself, 0 stops.
pub fn above_white(rgb: [f32; 3]) -> ([f32; 3], f32) {
    let lit = rgb.map(|c| if c.is_finite() { linear(c.max(0.0)) } else { 0.0 });
    let brightest = lit.iter().copied().fold(0.0, f32::max);
    if brightest <= 1.0 {
        return (rgb.map(|c| if c.is_finite() { c.clamp(0.0, 1.0) } else { 0.0 }), 0.0);
    }
    let stops = brightest.log2().min(MOST_STOPS);
    (lit.map(|c| encoded(c / brightest).clamp(0.0, 1.0)), stops)
}

/// A colour within white (sRGB-encoded) lit `stops` above white: [`above_white`] undone.
pub fn lit_above_white(rgb: [f32; 3], stops: f32) -> [f32; 3] {
    let stops = if stops.is_finite() { stops.clamp(0.0, MOST_STOPS) } else { 0.0 };
    if stops == 0.0 {
        return rgb;
    }
    let k = stops.exp2();
    rgb.map(|c| encoded(linear(c) * k))
}

/// How bright the brightest colour of HDR pixels is, in linear light (1: white).
pub fn brightest(half: &[u8]) -> f32 {
    half.as_chunks::<HDR_PIXEL>()
        .0
        .iter()
        .map(|px| {
            let c: Vec<f32> = floats(px).collect();
            let a = c.get(3).copied().unwrap_or(0.0);
            if a <= 0.0 {
                return 0.0;
            }
            c.iter().take(3).map(|v| linear(v / a)).fold(0.0, f32::max)
        })
        .fold(0.0, f32::max)
}

/// An OpenEXR file of HDR pixels `width` × `height` (rows top to bottom): half-float channels
/// in linear light, premultiplied, as compositing programs take them, with `p3`'s primaries (Display
/// P3's) named where the pixels are in them. Uncompressed scanlines.
pub fn exr(half: &[u8], width: u32, height: u32, p3: bool) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&20_000_630u32.to_le_bytes());
    out.extend_from_slice(&2u32.to_le_bytes());
    let attribute = |out: &mut Vec<u8>, name: &str, kind: &str, value: &[u8]| {
        out.extend_from_slice(name.as_bytes());
        out.push(0);
        out.extend_from_slice(kind.as_bytes());
        out.push(0);
        out.extend_from_slice(&(value.len() as u32).to_le_bytes());
        out.extend_from_slice(value);
    };
    // channels in alphabetical order, each: name, half (1), linear, reserved, sampling 1 × 1
    let mut channels = Vec::new();
    for name in ["A", "B", "G", "R"] {
        channels.extend_from_slice(name.as_bytes());
        channels.push(0);
        channels.extend_from_slice(&1u32.to_le_bytes());
        channels.extend_from_slice(&[0, 0, 0, 0]);
        channels.extend_from_slice(&1u32.to_le_bytes());
        channels.extend_from_slice(&1u32.to_le_bytes());
    }
    channels.push(0);
    attribute(&mut out, "channels", "chlist", &channels);
    attribute(&mut out, "compression", "compression", &[0]);
    let window: Vec<u8> =
        [0, 0, width.saturating_sub(1), height.saturating_sub(1)].iter().flat_map(|v| v.to_le_bytes()).collect();
    attribute(&mut out, "dataWindow", "box2i", &window);
    attribute(&mut out, "displayWindow", "box2i", &window);
    attribute(&mut out, "lineOrder", "lineOrder", &[0]);
    attribute(&mut out, "pixelAspectRatio", "float", &1.0f32.to_le_bytes());
    attribute(&mut out, "screenWindowCenter", "v2f", &[0u8; 8]);
    attribute(&mut out, "screenWindowWidth", "float", &1.0f32.to_le_bytes());
    if p3 {
        // red, green, blue and white's x and y
        let p3: [f32; 8] = [0.680, 0.320, 0.265, 0.690, 0.150, 0.060, 0.3127, 0.3290];
        let value: Vec<u8> = p3.iter().flat_map(|v| v.to_le_bytes()).collect();
        attribute(&mut out, "chromaticities", "chromaticities", &value);
    }
    out.push(0);
    // the offset table, a scanline a block
    let w = width as usize;
    let line = 8 + w * HDR_PIXEL;
    let table_at = out.len();
    let first = (table_at + height as usize * 8) as u64;
    for y in 0..height as u64 {
        out.extend_from_slice(&(first + y * line as u64).to_le_bytes());
    }
    for (y, row) in half.chunks_exact(w.max(1) * HDR_PIXEL).take(height as usize).enumerate() {
        out.extend_from_slice(&(y as u32).to_le_bytes());
        out.extend_from_slice(&((w * HDR_PIXEL) as u32).to_le_bytes());
        // linear light, premultiplied
        let px: Vec<[f32; 4]> = row
            .as_chunks::<HDR_PIXEL>()
            .0
            .iter()
            .map(|p| {
                let mut c = [0.0f32; 4];
                for (to, v) in c.iter_mut().zip(floats(p)) {
                    *to = if v.is_finite() { v } else { 0.0 };
                }
                let a = c[3].clamp(0.0, 1.0);
                let lin = |v: f32| if a > 0.0 { linear(v / a) * a } else { 0.0 };
                [lin(c[0]), lin(c[1]), lin(c[2]), a]
            })
            .collect();
        // each channel's row in turn: A, B, G, R
        for k in [3usize, 2, 1, 0] {
            for c in &px {
                out.extend_from_slice(&f32_to_half(c.get(k).copied().unwrap_or(0.0)).to_le_bytes());
            }
        }
    }
    out
}

/// Linear light in BT.2020's primaries from the document's (Display P3's with `p3`, else sRGB's),
/// rows of a matrix.
fn to_rec2020(p3: bool) -> [[f32; 3]; 3] {
    if p3 {
        [[0.753_833, 0.198_597, 0.047_570], [0.045_744, 0.941_777, 0.012_479], [-0.001_210, 0.017_601, 0.983_608]]
    } else {
        [[0.627_404, 0.329_283, 0.043_313], [0.069_097, 0.919_540, 0.011_362], [0.016_391, 0.088_013, 0.895_595]]
    }
}

/// HLG's scene light (1: its peak) for the document's white: what the BT.2408 reference signal
/// (0.75) stands for.
fn hlg_white() -> f32 {
    let (a, b, c) = (0.178_832_77f32, 0.284_668_92f32, 0.559_910_7_f32);
    (((0.75 - c) / a).exp() + b) / 12.0
}

/// ITU-R BT.2100 HLG's OETF on scene light 0..1.
fn hlg_signal(linear: f32) -> f32 {
    let e = linear.clamp(0.0, 1.0);
    if e <= 1.0 / 12.0 {
        return (3.0 * e).sqrt();
    }
    let (a, b, c) = (0.178_832_77f32, 0.284_668_92f32, 0.559_910_7_f32);
    a * (12.0 * e - b).ln() + c
}

/// HDR pixels `width` × `height` as a frame of a 10-bit HLG video, `out_width` × `out_height`
/// (each output pixel the mean of those it covers): half floats (RGBA, alpha 1) in BT.2020's
/// primaries, HLG-encoded, see-through parts over black. Light past HLG's peak (about 3.8 times
/// the document's white) clips.
pub fn hlg_frame(half: &[u8], (width, height): (u32, u32), p3: bool, (out_width, out_height): (u32, u32)) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let (ow, oh) = (out_width.max(1) as usize, out_height.max(1) as usize);
    let m = to_rec2020(p3);
    let white = hlg_white();
    let px: Vec<f32> = floats(half).map(|v| if v.is_finite() { v } else { 0.0 }).collect();
    let mut out = Vec::with_capacity(ow * oh * HDR_PIXEL);
    for oy in 0..oh {
        let (y0, y1) = (oy * h / oh, ((oy + 1) * h / oh).max(oy * h / oh + 1).min(h));
        for ox in 0..ow {
            let (x0, x1) = (ox * w / ow, ((ox + 1) * w / ow).max(ox * w / ow + 1).min(w));
            // the mean of the covered pixels, in linear light (premultiplied: over black)
            let mut sum = [0.0f32; 3];
            let mut n = 0.0f32;
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = (y * w + x) * 4;
                    for (k, s) in sum.iter_mut().enumerate() {
                        let a = px.get(i + 3).copied().unwrap_or(0.0).clamp(0.0, 1.0);
                        let c = px.get(i + k).copied().unwrap_or(0.0);
                        *s += if a > 0.0 { linear(c / a) * a } else { 0.0 };
                    }
                    n += 1.0;
                }
            }
            let lin = sum.map(|s| s / n.max(1.0) * white);
            let row = |r: [f32; 3]| r[0] * lin[0] + r[1] * lin[1] + r[2] * lin[2];
            for r in m {
                out.extend_from_slice(&f32_to_half(hlg_signal(row(r))).to_le_bytes());
            }
            out.extend_from_slice(&f32_to_half(1.0).to_le_bytes());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_floats_go_there_and_back() {
        for v in [0.0f32, 1.0, -1.0, 0.5, 2.0, 4.5, 65504.0, 1e-5, 6.1e-5, 0.1, 1.0 / 255.0] {
            let back = half_to_f32(f32_to_half(v));
            assert!((back - v).abs() <= v.abs() * 1e-3 + 1e-7, "{v} came back {back}");
        }
        assert_eq!(f32_to_half(1.0), 0x3c00);
        assert_eq!(f32_to_half(2.0), 0x4000);
        assert_eq!(half_to_f32(0x3c00), 1.0);
        assert!(half_to_f32(f32_to_half(1e6)).is_infinite());
        // every half that is a number comes back as itself
        for h in 0..=u16::MAX {
            let f = half_to_f32(h);
            if f.is_finite() {
                assert_eq!(f32_to_half(f), h, "{h:#x}");
            }
        }
    }

    #[test]
    fn a_colour_above_white_splits_into_a_colour_and_stops_and_back() {
        for v in [0.0f32, 0.002, 0.04, 0.2, 0.5, 1.0, 1.7, 3.0] {
            assert!((linear(encoded(v)) - v).abs() < 1e-5, "{v}");
        }
        // within white: itself, no stops
        assert_eq!(above_white([0.2, 0.6, 1.0]), ([0.2, 0.6, 1.0], 0.0));
        assert_eq!(lit_above_white([0.2, 0.6, 1.0], 0.0), [0.2, 0.6, 1.0]);
        // two stops: four times white's light in its brightest channel, its hue kept
        let lit = lit_above_white([1.0, 0.5, 0.25], 2.0);
        assert!((linear(lit[0]) - 4.0).abs() < 1e-4);
        assert!((linear(lit[1]) / linear(lit[0]) - linear(0.5)).abs() < 1e-5);
        let (colour, stops) = above_white(lit);
        assert!((stops - 2.0).abs() < 1e-4, "{stops}");
        for (a, b) in colour.iter().zip([1.0, 0.5, 0.25]) {
            assert!((a - b).abs() < 1e-4, "{colour:?}");
        }
        // what isn't a colour is black
        assert_eq!(above_white([f32::NAN, f32::INFINITY, -1.0]), ([0.0; 3], 0.0));
    }

    #[test]
    fn eight_bit_pixels_widen_and_narrow_unchanged() {
        let rgba: Vec<u8> = (0..=255u8).flat_map(|v| [v, v / 2, v / 3, 255]).collect();
        assert_eq!(narrow(&widen(&rgba)), rgba);
    }

    #[test]
    fn light_above_white_is_cut_off_or_rolled_off() {
        let px = halves([3.0, 1.0, 0.5, 1.0]);
        assert_eq!(narrow(&px), vec![255, 255, 128, 255]);
        // tone-mapped, the red stays the strongest and the others are scaled down with it
        let toned = tone_mapped(&px);
        assert!(toned[0] > 240, "{toned:?}");
        assert!(toned[1] < 100 && toned[2] < 50, "{toned:?}");
        // what is below the knee is left as it is
        let dim = halves([0.5, 0.25, 0.1, 1.0]);
        assert_eq!(tone_mapped(&dim), narrow(&dim));
        assert!(brightest(&px) > 4.0);
    }

    #[test]
    fn an_hlg_frame_puts_white_at_the_reference_signal() {
        let white = halves([1.0, 1.0, 1.0, 1.0]);
        let frame = hlg_frame(&white, (1, 1), false, (1, 1));
        let v: Vec<f32> = floats(&frame).collect();
        assert!(v.iter().take(3).all(|c| (c - 0.75).abs() < 0.01), "{v:?}");
        // three times white's light (encoded) is near HLG's peak; far past it clips
        let bright = hlg_frame(&halves([1.6, 1.6, 1.6, 1.0]), (1, 1), false, (1, 1));
        let b: Vec<f32> = floats(&bright).collect();
        assert!(b[0] > 0.9 && b[0] <= 1.0, "{b:?}");
        // two pixels into one: their mean
        let two = halves([0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0]);
        let one: Vec<f32> = floats(&hlg_frame(&two, (2, 1), true, (1, 1))).collect();
        assert!(one[1] > 0.4 && one[1] < 0.75, "{one:?}");
    }

    #[test]
    fn an_exr_holds_its_pixels_in_linear_light() {
        let px = halves([2.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
        let file = exr(&px, 2, 1, false);
        assert_eq!(file.get(..4), Some(&20_000_630u32.to_le_bytes()[..]));
        // the last scanline's R channel (the last two halves): 2.0 encoded is linear 4.95
        let n = file.len();
        let r = half_to_f32(u16::from_le_bytes([file[n - 4], file[n - 3]]));
        assert!((r - linear(2.0)).abs() < 0.01, "{r}");
    }
}
