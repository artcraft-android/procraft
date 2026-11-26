// Paint laid apart reads the saved layer and its own paint, and synthesizes directly
// into the layer. The mixed color still rounds through the same packed RGBA8 words.
@group(0) @binding(1) var layer_out: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(3) var grained_tex: texture_storage_2d<rgba8unorm, read_write>;
@group(0) @binding(4) var amount_tex: texture_storage_2d<rgba8unorm, read_write>;
@group(0) @binding(10) var shown_tex: texture_storage_2d<rgba8unorm, read_write>;

fn work_at(p: vec2<i32>) -> vec4<f32> {
    return before_at(p);
}

fn wet_pixel(p: vec2<i32>) -> vec2<i32> {
    return p - vec2<i32>(st.origin_x, st.origin_y);
}

fn grained_at(p: vec2<i32>) -> vec4<f32> {
    return unpack4x8unorm(pack4x8unorm(textureLoad(grained_tex, wet_pixel(p))));
}

// The legacy tile snapshots store each f32 as four raw bytes in RGBA8. Use the same
// encoding so marks and the replaceable tip can copy both paths the same way.
fn amount_at(p: vec2<i32>) -> f32 {
    return bitcast<f32>(pack4x8unorm(textureLoad(amount_tex, wet_pixel(p))));
}

fn shown_at(p: vec2<i32>) -> f32 {
    return bitcast<f32>(pack4x8unorm(textureLoad(shown_tex, wet_pixel(p))));
}

fn put_work(p: vec2<i32>, word: u32) {
    textureStore(layer_out, p, unpack4x8unorm(word));
}

fn put_grained(p: vec2<i32>, word: u32) {
    textureStore(grained_tex, wet_pixel(p), unpack4x8unorm(word));
}

fn put_amount(p: vec2<i32>, value: f32) {
    textureStore(amount_tex, wet_pixel(p), unpack4x8unorm(bitcast<u32>(value)));
}

fn put_shown(p: vec2<i32>, value: f32) {
    textureStore(shown_tex, wet_pixel(p), unpack4x8unorm(bitcast<u32>(value)));
}
