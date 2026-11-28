// A stroke shown while it is still moving (see `canvas/provisional.rs`): laid on a canvas a k-th of
// this one's size over the layer as it was there, and shown in the layer as the layer as it was plus
// what the stroke changed on the small canvas, drawn k times as big. Where it changed nothing the
// layer stays exactly as it was.

// the layer as it was (this canvas's size); and the small canvas's layer as the stroke leaves it,
// and as it was before the stroke
@group(0) @binding(0) var before: texture_2d<f32>;
@group(0) @binding(1) var small_after: texture_2d<f32>;
@group(0) @binding(2) var small_before: texture_2d<f32>;
// how many of this canvas's pixels a small canvas's pixel covers each way
@group(0) @binding(3) var<uniform> k: vec4<u32>;

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32(vi & 1u) * 4.0 - 1.0;
    let y = f32(vi >> 1u) * 4.0 - 1.0;
    return vec4<f32>(x, y, 0.0, 1.0);
}

// a pixel of the small canvas: the mean of the k by k pixels of the layer it covers
@fragment
fn fs_down(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let n = max(i32(k.x), 1);
    let last = vec2<i32>(textureDimensions(before)) - 1;
    let q = vec2<i32>(pos.xy) * n;
    var sum = vec4<f32>(0.0);
    for (var y = 0; y < n; y++) {
        for (var x = 0; x < n; x++) {
            sum += textureLoad(before, clamp(q + vec2<i32>(x, y), vec2<i32>(0), last), 0);
        }
    }
    return sum / f32(n * n);
}

// `t` at small canvas point `s` (pixel middles at whole numbers), between its four nearest pixels
fn between(t: texture_2d<f32>, s: vec2<f32>) -> vec4<f32> {
    let last = vec2<i32>(textureDimensions(t)) - 1;
    let i = vec2<i32>(floor(s));
    let f = s - floor(s);
    let a = textureLoad(t, clamp(i, vec2<i32>(0), last), 0);
    let b = textureLoad(t, clamp(i + vec2<i32>(1, 0), vec2<i32>(0), last), 0);
    let c = textureLoad(t, clamp(i + vec2<i32>(0, 1), vec2<i32>(0), last), 0);
    let d = textureLoad(t, clamp(i + vec2<i32>(1, 1), vec2<i32>(0), last), 0);
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

// a pixel of the layer: as it was, and what the stroke changed on the small canvas there
@fragment
fn fs_show(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(pos.xy);
    let was = textureLoad(before, p, 0);
    let s = pos.xy / f32(max(k.x, 1u)) - 0.5;
    let changed = between(small_after, s) - between(small_before, s);
    let out = clamp(was + changed, vec4<f32>(0.0), vec4<f32>(1.0));
    // (premultiplied: no colour past its coverage)
    return vec4<f32>(min(out.rgb, vec3<f32>(out.a)), out.a);
}
