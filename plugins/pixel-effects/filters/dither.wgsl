// Dither, as a filter layer: each channel (or the grey, with Black and white) brought to v.levels
// steps, the threshold between them broken up by a pattern of dots v.pattern pixels across: an
// ordered (Bayer) grid, or noise. v.amount of the way from the colour to that.

// The 8 × 8 ordered threshold at cell c, 0..1.
fn bayer(c: vec2<i32>) -> f32 {
    var b = 0;
    let x = c.x & 7;
    let y = c.y & 7;
    for (var bit = 0; bit < 3; bit++) {
        let xb = (x >> u32(2 - bit)) & 1;
        let yb = (y >> u32(2 - bit)) & 1;
        b = b * 4 + ((xb ^ yb) * 2 + yb);
    }
    return (f32(b) + 0.5) / 64.0;
}

// A threshold at cell c that looks random, 0..1.
fn noise_at(c: vec2<f32>) -> f32 {
    return fract(sin(dot(c, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn pixel(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> {
    if (colour.a <= 0.0 || v.amount <= 0.0) {
        return colour;
    }
    var rgb = colour.rgb;
    if (v.mono > 0.5) {
        rgb = vec3<f32>(dot(rgb, vec3<f32>(0.299, 0.587, 0.114)));
    }
    let cell = floor(at / max(v.pattern, 1.0));
    var threshold = bayer(vec2<i32>(cell));
    if (v.noise > 0.5) {
        threshold = noise_at(cell);
    }
    let steps = max(v.levels - 1.0, 1.0);
    let stepped = clamp(floor(rgb * steps + threshold), vec3<f32>(0.0), vec3<f32>(steps)) / steps;
    return vec4<f32>(mix(colour.rgb, stepped, clamp(v.amount, 0.0, 1.0)), colour.a);
}
