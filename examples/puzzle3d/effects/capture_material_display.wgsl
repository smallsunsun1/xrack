fn scene_material(input: XrMaterialInput, surface: XrMaterialSurface) -> XrMaterialSurface {
    var result = surface;
    if material_has_texture(0u) {
        let feed = material_texture(0u, input.uv0);
        result.base_color = select(mix(vec3(0.035, 0.055, 0.07), feed.rgb, feed.a), feed.rgb, input.parameters.x > 0.0);
    } else {
        result.base_color = vec3(0.55, 0.025, 0.55);
    }
    return result;
}
