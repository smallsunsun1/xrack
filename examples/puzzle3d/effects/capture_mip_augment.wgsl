fn scene_capture_augment(
    input: XrMaterialInput, surface: XrMaterialSurface, index: u32,
) -> vec4<f32> {
    let parameters = capture_augment_parameters();
    if parameters.y > 2.5 {
        let horizontal = parameters.x < 0.0;
        let diagonal = sqrt(0.5);
        let dx = select(vec2(1.0 / 256.0, 0.0), vec2(diagonal, diagonal / 128.0), horizontal);
        let dy = select(vec2(0.0, 2.0), vec2(diagonal, -diagonal / 128.0), horizontal);
        return capture_augment_texture_grad(0u, vec2(0.25, 0.5), dx, dy);
    }
    let uv = vec2(select(0.25, 0.50048828125, parameters.y > 1.5), 0.5);
    if parameters.y > 0.5 {
        return capture_augment_texture_grad(
            0u, uv, input.uv0_dx * parameters.z, input.uv0_dy * parameters.z);
    }
    return capture_augment_texture_lod(0u, uv, parameters.x);
}
