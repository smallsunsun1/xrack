fn scene_material(input: XrMaterialInput, surface: XrMaterialSurface) -> XrMaterialSurface {
    var result = surface;
    if material_has_volume(0u) && material_volume_size(0u).z > 0u {
        let sampled = material_volume(0u, input.parameters);
        result.base_color = sampled.rgb * sampled.a;
    } else {
        result.base_color = material_texture(1u, vec2(0.35, 0.5)).rgb;
    }
    return result;
}
