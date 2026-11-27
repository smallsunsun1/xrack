fn scene_material(input: XrMaterialInput, surface: XrMaterialSurface) -> XrMaterialSurface {
    var result = surface;
    let texel = material_texture(0u, vec2(0.25, 0.25));
    result.base_color = select(vec3(0.025), texel.rgb * texel.a, material_has_texture(0u));
    return result;
}
