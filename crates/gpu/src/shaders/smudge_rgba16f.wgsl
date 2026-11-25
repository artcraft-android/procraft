// An HDR canvas's pixels in a smudge's window (see smudge_compute.wgsl): a premultiplied pixel two
// u32s, red and green then blue and alpha as half floats. Colour keeps its light above white.

// u32 words a pixel
const PX_WORDS: u32 = 2u;
// half floats don't band: nothing is dithered
const DITHER: f32 = 0.0;

// the pixel its words hold
fn px_of(a: u32, b: u32) -> vec4<f32> {
    return vec4<f32>(unpack2x16float(a), unpack2x16float(b));
}

fn word0(v: vec4<f32>) -> u32 {
    return pack2x16float(v.xy);
}

fn word1(v: vec4<f32>) -> u32 {
    return pack2x16float(v.zw);
}
