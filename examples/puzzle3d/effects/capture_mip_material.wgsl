fn scene_material(input: XrMaterialInput, surface: XrMaterialSurface) -> XrMaterialSurface {
    var result = surface;
    if input.parameters.y > 2.5 {
        let horizontal = input.parameters.x < 0.0;
        let diagonal = sqrt(0.5);
        let dx = select(vec2(1.0 / 256.0, 0.0), vec2(diagonal, diagonal / 128.0), horizontal);
        let dy = select(vec2(0.0, 2.0), vec2(diagonal, -diagonal / 128.0), horizontal);
        result.base_color = material_texture_grad(0u, vec2(0.25, 0.5), dx, dy).rgb;
        return result;
    }
    let uv = vec2(select(0.25, 0.50048828125, input.parameters.y > 1.5), 0.5);
    if input.parameters.y > 0.5 {
        result.base_color = material_texture_grad(
            0u, uv, input.uv0_dx * input.parameters.z, input.uv0_dy * input.parameters.z).rgb;
    } else {
        result.base_color = material_texture_lod(0u, uv, input.parameters.x).rgb;
    }
    return result;
}
