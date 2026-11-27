// Sort 128 keys in workgroup memory. Padding sorts after all live indices.
@group(0) @binding(11) var<storage, read_write> sort_keys: array<vec2<u32>>;
var<workgroup> tile: array<vec2<u32>, 128>;

fn sort_key(index: u32) -> vec2<u32> {
    if index >= arrayLength(&sort_keys) {
        return vec2<u32>(0xffffffffu);
    }
    let value = particle_order_key(index);
    if !(abs(value) <= 3.402823e38) {
        return vec2<u32>(0xffffffffu, index);
    }
    let bits = bitcast<u32>(select(value, 0.0, value == 0.0));
    let ordered = select(bits ^ 0x80000000u, ~bits, (bits & 0x80000000u) != 0u);
    return vec2<u32>(ordered, index);
}

fn key_before(left: vec2<u32>, right: vec2<u32>) -> bool {
    return left.x < right.x || (left.x == right.x && left.y < right.y);
}

@compute @workgroup_size(64)
fn sort_tile(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) lane: u32) {
    let base = group.x * 128u;
    tile[lane] = sort_key(base + lane);
    tile[lane + 64u] = sort_key(base + lane + 64u);
    workgroupBarrier();

    for (var width = 2u; width <= 128u; width *= 2u) {
        for (var step = width / 2u; step > 0u; step /= 2u) {
            let low = 2u * lane - lane % step;
            let high = low + step;
            let a = tile[low];
            let b = tile[high];
            let ascending = (low & width) == 0u;
            if key_before(b, a) == ascending {
                tile[low] = b;
                tile[high] = a;
            }
            workgroupBarrier();
        }
    }

    let count = arrayLength(&sort_keys);
    if base + lane < count {
        sort_keys[base + lane] = tile[lane];
    }
    if base + lane + 64u < count {
        sort_keys[base + lane + 64u] = tile[lane + 64u];
    }
}
