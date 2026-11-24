fn scene_material(input: XrMaterialInput, surface: XrMaterialSurface) -> XrMaterialSurface {
    var result = surface;
    let index = u32(input.parameters.x);
    let has_image = material_has_texture(index) && all(material_texture_size(index) > vec2(0u));
    let uv = input.uv0 * 2.0 - vec2(0.5);
    let texel = material_texture(index, uv);
    result.base_color = select(vec3(0.065), texel.rgb, has_image && texel.a > 0.0);
    return result;
}
