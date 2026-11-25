// Vignette, as a filter layer: the edges darkened (or, Towards Light, lightened) towards the
// corners by v.amount, starting v.size of the way from the middle to a corner and fading in over
// v.softness of the rest; round whatever the canvas's shape. Light above white stays where nothing darkens it.
fn pixel(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> {
    let half = canvas_size() * 0.5;
    let away = length(at - half) / max(length(half), 1.0);
    let start = clamp(v.size, 0.0, 1.0);
    let end = start + max(clamp(v.softness, 0.0, 1.0) * (1.5 - start), 0.01);
    let k = smoothstep(start, end, away) * clamp(v.amount, 0.0, 1.0);
    if (v.light > 0.5) {
        return vec4<f32>(mix(colour.rgb, vec3<f32>(1.0), k), colour.a);
    }
    return vec4<f32>(colour.rgb * (1.0 - k), colour.a);
}
