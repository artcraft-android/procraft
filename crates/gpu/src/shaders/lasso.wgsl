// A lasso's polygon as antialiased coverage, by the non-zero rule (as a filled lasso takes it).
// Drawn once a sample position: the polygon as a fan of triangles from its first point (given in
// clip space, moved by the sample's offset), each adding its winding (+1 one way round, -1 the
// other) to what it covers; then each pixel whose winding isn't zero adds that sample's share of
// coverage.

@vertex
fn vs_winding(@location(0) p: vec2<f32>) -> @builtin(position) vec4<f32> {
    return vec4<f32>(p, 0.0, 1.0);
}

@fragment
fn fs_winding(@builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return vec4<f32>(select(-1.0, 1.0, front), 0.0, 0.0, 0.0);
}

@group(0) @binding(0) var winding: texture_2d<f32>;

// a triangle over the whole target
@vertex
fn vs_cover(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fs_cover(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let w = textureLoad(winding, vec2<i32>(pos.xy), 0).r;
    // one of the eight samples' share where it is inside
    return vec4<f32>(select(0.0, 0.125, abs(w) >= 0.5), 0.0, 0.0, 0.0);
}
