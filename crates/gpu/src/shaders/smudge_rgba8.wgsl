// A standard canvas's pixels in a smudge's window (see smudge_compute.wgsl): a premultiplied RGBA8
// pixel a u32.

// u32 words a pixel
const PX_WORDS: u32 = 1u;
// what is laid is rounded to 8 bits up or down at random, in proportion to the remainder, so that
// faint stamps add up as they should instead of rounding away
const DITHER: f32 = 1.0;

// the pixel its words hold (the second unused)
fn px_of(a: u32, b: u32) -> vec4<f32> {
    return unpack4x8unorm(a);
}

fn word0(v: vec4<f32>) -> u32 {
    return pack4x8unorm(v);
}

fn word1(v: vec4<f32>) -> u32 {
    return 0u;
}
