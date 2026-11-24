fn scene_material(input: XrMaterialInput, surface: XrMaterialSurface) -> XrMaterialSurface {
    var result = surface;
    let slot = u32(input.parameters.x);
    if material_has_texture(slot) {
        let sample = material_texture(slot, vec2(0.3, 0.5));
        result.base_color = select(sample.rgb, vec3(sample.a), input.uv0.x >= 0.7);
    } else {
        result.base_color = vec3(0.025, 0.045, 0.07);
    }
    return result;
}
