struct Tile {
    start_word: u32,
    size: u32,
    stride: u32,
}
@group(0) @binding(0) var source: texture_depth_2d;
@group(0) @binding(1) var<storage, read_write> depth: array<u32>;
@group(0) @binding(2) var<uniform> tile: Tile;

fn depth16(pixel: vec2<u32>) -> u32 {
    return u32(round(clamp(textureLoad(source, vec2<i32>(pixel), 0), 0.0, 1.0) * 65535.0));
}

@compute @workgroup_size(8, 8)
fn pack_depth(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= tile.stride || id.y >= tile.size {
        return;
    }
    let x = id.x * 2u;
    let a = depth16(vec2(x, id.y));
    let b = depth16(vec2(min(x + 1u, tile.size - 1u), id.y));
    depth[tile.start_word + id.y * tile.stride + id.x] = a | (b << 16u);
}
