fn scene_material_data(input: XrMaterialInput, surface: XrMaterialSurface) -> vec4<f32> {
    let green = select(input.uv0.y * 0.5, material_texture(0u, input.uv0).g, material_has_texture(0u));
    return vec4(input.parameters.x, green, surface.metallic, -0.25);
}
