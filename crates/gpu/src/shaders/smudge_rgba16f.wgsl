// An HDR canvas's smudged layer in `work` and `grained`: a premultiplied pixel two u32s, red and
// green then blue and alpha as half floats. Colour keeps its light above white; half floats don't
// band, so nothing is dithered.

fn work_at(k: u32) -> vec4<f32> {
    return vec4<f32>(unpack2x16float(work[2u * k]), unpack2x16float(work[2u * k + 1u]));
}

fn grained_at(k: u32) -> vec4<f32> {
    return vec4<f32>(unpack2x16float(grained[2u * k]), unpack2x16float(grained[2u * k + 1u]));
}

fn kept(v: vec4<f32>) -> vec4<f32> {
    return clamp(v, vec4<f32>(0.0), vec4<f32>(CEILING, CEILING, CEILING, 1.0));
}

fn put_work(k: u32, v: vec4<f32>, h: f32) {
    let c = kept(v);
    work[2u * k] = pack2x16float(c.xy);
    work[2u * k + 1u] = pack2x16float(c.zw);
}

fn put_grained(k: u32, v: vec4<f32>, h: f32) {
    let c = kept(v);
    grained[2u * k] = pack2x16float(c.xy);
    grained[2u * k + 1u] = pack2x16float(c.zw);
}
