// Smudge's compute passes share their math and use either window buffers or resident textures.
@group(0) @binding(2) var<storage, read_write> carry: array<vec4<f32>>;

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
fn cs_carry(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = part_at(id.xy);
    if (!in_part(i, window_side())) {
        return;
    }
    carry[cell_index(window_cell(i))] = kept_paint(carry_cell(i), vec4<f32>(0.0));
}

var<workgroup> dull_sums: array<vec4<f32>, 64>;
var<workgroup> dull_weights: array<f32, 64>;

@compute @workgroup_size(64)
fn cs_dull(@builtin(local_invocation_index) i: u32) {
    let part = dull_part(i);
    dull_sums[i] = part.sum;
    dull_weights[i] = part.weight;
    workgroupBarrier();
    if (i != 0u) {
        return;
    }
    var total = vec4<f32>(0.0);
    var weighed = 0.0;
    for (var k = 0u; k < DULL_PARTS; k = k + 1u) {
        total = total + dull_sums[k];
        weighed = weighed + dull_weights[k];
    }
    carry[dull_index()] = kept_paint(dull_colour(total, weighed), vec4<f32>(0.0));
}

@compute @workgroup_size(8, 8)
fn cs_blur_down(@builtin(global_invocation_id) id: vec3<u32>) {
    let n = window_side();
    if (id.x >= n || id.y >= n) {
        return;
    }
    let g = i32(st.grid);
    carry[g * g + cell_index(window_cell(id.xy))] = kept_paint(blur_down_cell(id.xy), vec4<f32>(0.0));
}

@compute @workgroup_size(8, 8)
fn cs_blur_across(@builtin(global_invocation_id) id: vec3<u32>) {
    let n = window_side();
    if (id.x >= n || id.y >= n) {
        return;
    }
    let g = i32(st.grid);
    carry[2 * g * g + cell_index(window_cell(id.xy))] = kept_paint(blur_across_cell(id.xy), vec4<f32>(0.0));
}

@compute @workgroup_size(8, 8)
fn cs_deposit(@builtin(global_invocation_id) id: vec3<u32>) {
    let p = st.bbox.xy + vec2<i32>(id.xy);
    let dab = own_dab();
    if (!in_bbox(dab, p)) {
        return;
    }
    let d = deposit(dab, p, found_at(p), selected_at(p));
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
    let selection = selected_at(p);
    if (selection <= 0.0) {
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
        let d = deposit(pass_dab(k), p, found, selection);
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
