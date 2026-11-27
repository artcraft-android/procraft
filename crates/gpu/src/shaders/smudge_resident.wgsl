// Wet paint laid apart with its paint kept on the GPU between passes (see `smudge.rs`): the stroke's
// colour and coverage in storage textures over its region, the layer as it was read from the
// region's copy of it, and the paint put together written straight into the layer. The accessors
// smudge.wgsl reads through, and the passes such a stroke takes (smudge_compute.wgsl's, as they lay
// wet paint, writing through the textures). The window buffers' passes stay in smudge_compute.wgsl
// as they were: with them rewritten (the same sums), an Adreno's OpenGL ES laid two wet tests' paint
// a few levels past their bounds. (This path is never taken there: it needs read-write storage
// textures, which wgpu's OpenGL doesn't give.)
@group(0) @binding(1) var layer_out: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<storage, read_write> carry: array<vec4<f32>>;
@group(0) @binding(3) var grained_tex: texture_storage_2d<rgba8unorm, read_write>;
@group(0) @binding(4) var amount_tex: texture_storage_2d<rgba8unorm, read_write>;
@group(0) @binding(10) var shown_tex: texture_storage_2d<rgba8unorm, read_write>;

// the layer as it was before the stroke: what it is laid over
fn work_at(p: vec2<i32>) -> vec4<f32> {
    return before_at(p);
}

// canvas pixel `p` in the region's textures
fn wet_pixel(p: vec2<i32>) -> vec2<i32> {
    return p - vec2<i32>(st.origin_x, st.origin_y);
}

fn grained_at(p: vec2<i32>) -> vec4<f32> {
    return unpack4x8unorm(pack4x8unorm(textureLoad(grained_tex, wet_pixel(p))));
}

// how much each pixel has taken and how much of the paint shows: an f32 a pixel kept as its four
// bytes, as the tiles keep them, so marks and a tip laid in place copy either path's alike
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

fn carry_word(i: u32) -> vec4<f32> {
    if (i >= arrayLength(&carry)) {
        return vec4<f32>(0.0);
    }
    return carry[i];
}

fn carry_at(k: i32) -> vec4<f32> {
    return carry_word(u32(k));
}

fn plot_texel(plots: u32, k: u32) -> vec4<f32> {
    if (plots == CARRIED_PLOT) {
        return carry_word(st.index * CARRIED + k);
    }
    return carry_word(2u * CARRIED + k);
}

@compute @workgroup_size(8, 8)
fn cs_deposit(@builtin(global_invocation_id) id: vec3<u32>) {
    let p = st.bbox.xy + vec2<i32>(id.xy);
    let dab = own_dab();
    if (!in_bbox(dab, p)) {
        return;
    }
    let d = deposit(dab, p, found_at(p));
    if (!d.laid) {
        return;
    }
    if (d.amount_set) {
        put_amount(p, d.amount);
    }
    if (d.shown_set) {
        put_shown(p, d.shown);
    }
    if (d.into_grained) {
        put_grained(p, pack4x8unorm(d.out));
        return;
    }
    put_work(p, pack4x8unorm(d.out));
}

// A run of a pass's stamps (`st.run`, as `wet_pass` lists them) laid at once over all their bounds
// (`st.bbox`): each pixel goes through those over it in order, as its paint stands after each (in 8
// bits a channel, as the window keeps it), and is written once. It comes out as the stamps laid
// one by one do, each a dispatch of its own.
@compute @workgroup_size(8, 8)
fn cs_deposit_run(@builtin(global_invocation_id) id: vec3<u32>) {
    let p = st.bbox.xy + vec2<i32>(id.xy);
    if (any(p >= st.bbox.zw)) {
        return;
    }
    var found = found_at(p);
    var laid = false;
    var amount_set = false;
    var shown_set = false;
    var word = 0u;
    for (var k = st.run.x; k < st.run.y; k++) {
        let bbox = wet_pass.stamps[k].bbox;
        if (any(p < bbox.xy) || any(p >= bbox.zw)) {
            continue;
        }
        let d = deposit(pass_dab(k), p, found);
        if (!d.laid) {
            continue;
        }
        laid = true;
        if (d.amount_set) {
            found.amount = d.amount;
            amount_set = true;
        }
        if (d.shown_set) {
            found.shown = d.shown;
            shown_set = true;
        }
        // as the window keeps it, and the next stamp finds it
        word = pack4x8unorm(d.out);
        if (d.into_grained) {
            found.grained = unpack4x8unorm(word);
        } else {
            found.work = unpack4x8unorm(word);
        }
    }
    if (!laid) {
        return;
    }
    if (amount_set) {
        put_amount(p, found.amount);
    }
    if (shown_set) {
        put_shown(p, found.shown);
    }
    if (st.laid_apart == 2u) {
        put_grained(p, word);
    } else {
        put_work(p, word);
    }
}

@compute @workgroup_size(8, 8)
fn cs_synthesize(@builtin(global_invocation_id) id: vec3<u32>) {
    let p = st.bbox.xy + vec2<i32>(id.xy);
    if (!in_bbox(own_dab(), p) || left_out(p)) {
        return;
    }
    put_work(p, pack4x8unorm(synthesized(p)));
}

@compute @workgroup_size(8, 8)
fn cs_wet_plots(@builtin(global_invocation_id) id: vec3<u32>) {
    let n = st.plot;
    let i = part_at(id.xy);
    if (!in_part(i, n)) {
        return;
    }
    let t = plot_t(vec2<f32>(i), n);
    var walk = wet_walk_start(t);
    for (var k = 0u; k < st.count; k++) {
        walk = wet_walk_on(walk, k, t);
        carry[2u * CARRIED + k * n * n + i.y * n + i.x] = kept_paint(from_mix(walk.out), vec4<f32>(0.0));
    }
    carry[(1u - st.index) * CARRIED + i.y * n + i.x] = kept_paint(walk.out, vec4<f32>(0.0));
}
