fn scene_capture_augment(
    input: XrMaterialInput, surface: XrMaterialSurface, index: u32,
) -> vec4<f32> {
    let center = vec2(0.5);
    let red = capture_augment_texture(0u, center).r;
    let green = capture_augment_texture(1u, center).g;
    let depth = capture_augment_texture_world_position(3u, center);
    let matching_depth = depth.valid && abs(depth.position.z - input.world_position.z) < 0.1;
    let blue = select(0.0, capture_augment_texture(2u, center).b,
        matching_depth && capture_augment_has_texture(2u) && capture_augment_texture_size(2u).x == 2u);
    return vec4(red, green, blue, 1.0);
}
