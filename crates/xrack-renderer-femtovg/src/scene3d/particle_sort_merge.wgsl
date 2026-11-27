// Each item ranks itself in the adjacent run. Original indices break ties
// across runs and give every item a unique destination.
override RUN_WIDTH: u32 = 128u;
@group(0) @binding(0) var<storage, read> input_keys: array<vec2<u32>>;
@group(0) @binding(1) var<storage, read_write> output_keys: array<vec2<u32>>;

@compute @workgroup_size(64)
fn merge_runs(@builtin(global_invocation_id) gid: vec3<u32>) {
    let count = arrayLength(&input_keys);
    let index = gid.x;
    if index >= count {
        return;
    }
    let key = input_keys[index];
    let run = index / RUN_WIDTH;
    let run_start = run * RUN_WIDTH;
    let adjacent = min((run ^ 1u) * RUN_WIDTH, count);
    var low = adjacent;
    var high = min(adjacent + RUN_WIDTH, count);
    while low < high {
        let middle = low + (high - low) / 2u;
        let other = input_keys[middle];
        if other.x < key.x || (other.x == key.x && other.y < key.y) {
            low = middle + 1u;
        } else {
            high = middle;
        }
    }
    let pair_start = (run & ~1u) * RUN_WIDTH;
    let rank = index - run_start + low - adjacent;
    output_keys[pair_start + rank] = key;
}
