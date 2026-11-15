// A standard canvas's smudged layer in `work` and `grained`: a premultiplied RGBA8 pixel a u32.

fn work_at(k: u32) -> vec4<f32> {
    return unpack4x8unorm(work[k]);
}

fn grained_at(k: u32) -> vec4<f32> {
    return unpack4x8unorm(grained[k]);
}

// rounded to 8 bits up or down at random by `h` (0..1), in proportion to the remainder, so that
// faint stamps add up as they should instead of rounding away
fn rounded(v: vec4<f32>, h: f32) -> u32 {
    return pack4x8unorm(clamp(v + (h - 0.5) / 255.0, vec4<f32>(0.0), vec4<f32>(1.0)));
}

fn put_work(k: u32, v: vec4<f32>, h: f32) {
    work[k] = rounded(v, h);
}

fn put_grained(k: u32, v: vec4<f32>, h: f32) {
    grained[k] = rounded(v, h);
}
