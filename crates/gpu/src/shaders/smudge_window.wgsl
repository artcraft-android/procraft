// Storage for the windowed compute path. Coverage uses its original float bits.
@group(0) @binding(1) var<storage, read_write> work: array<u32>;
@group(0) @binding(3) var<storage, read_write> grained: array<u32>;
@group(0) @binding(4) var<storage, read_write> amount: array<f32>;

fn window_index(p: vec2<i32>) -> u32 {
    return u32(p.y - st.origin_y) * st.stride + u32(p.x - st.origin_x);
}

fn work_at(p: vec2<i32>) -> vec4<f32> {
    let i = window_index(p);
    if (i >= arrayLength(&work)) {
        return vec4<f32>(0.0);
    }
    return unpack4x8unorm(work[i]);
}

fn grained_at(p: vec2<i32>) -> vec4<f32> {
    let i = window_index(p);
    if (i >= arrayLength(&grained)) {
        return vec4<f32>(0.0);
    }
    return unpack4x8unorm(grained[i]);
}

fn amount_word(i: u32) -> f32 {
    if (i >= arrayLength(&amount)) {
        return 0.0;
    }
    return amount[i];
}

fn amount_at(p: vec2<i32>) -> f32 {
    return amount_word(window_index(p));
}

fn shown_at(p: vec2<i32>) -> f32 {
    return amount_word(st.stride * st.rows + window_index(p));
}

fn put_work(p: vec2<i32>, word: u32) {
    work[window_index(p)] = word;
}

fn put_grained(p: vec2<i32>, word: u32) {
    grained[window_index(p)] = word;
}

fn put_amount(p: vec2<i32>, value: f32) {
    amount[window_index(p)] = value;
}

fn put_shown(p: vec2<i32>, value: f32) {
    amount[st.stride * st.rows + window_index(p)] = value;
}
