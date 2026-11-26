// Instance indices occupy 13 bits above the three rendering flags. The
// next byte carries the coarse coverage and a bit for its complement.
fn xr_instance_number(metadata: u32) -> u32 {
    return (metadata >> 3u) & 8191u;
}

fn xr_lod_covered(metadata: u32, position: vec2<f32>) -> bool {
    let coverage = (metadata >> 16u) & 255u;
    if coverage == 0u {
        return true;
    }
    let pixel = vec2<u32>(position);
    let order = vec4<u32>(0u, 2u, 3u, 1u);
    let low = order[(pixel.x & 1u) | ((pixel.y & 1u) << 1u)];
    let mid = order[((pixel.x >> 1u) & 1u) | (pixel.y & 2u)];
    let high = order[((pixel.x >> 2u) & 1u) | ((pixel.y >> 1u) & 2u)];
    let threshold = 16u * low + 4u * mid + high;
    return (threshold < (coverage & 127u)) != ((coverage & 128u) != 0u);
}
