override WIDE_POINTS: bool = false;

fn xr_point_corner(clip: vec4<f32>, index: u32, pixels: f32, viewport: vec2<f32>) -> vec4<f32> {
    if !WIDE_POINTS || clip.w <= 0.0 || clip.z < 0.0 || clip.z > clip.w || any(abs(clip.xy) > vec2(clip.w)) {
        return clip;
    }
    let dimensions = max(viewport, vec2(1.0));
    let corner = vec2(f32(index & 1u), f32((index >> 1u) & 1u)) * 2.0 - 1.0;
    let extent = min(vec2(pixels) / dimensions, vec2(4.0));
    return vec4(clip.xy + corner * extent * clip.w, clip.zw);
}
