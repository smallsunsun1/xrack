fn scene_material(input: XrMaterialInput, surface: XrMaterialSurface) -> XrMaterialSurface {
    let stripe = 0.62 + 0.38 * smoothstep(0.16, 0.4, abs(input.uv0.y - 0.5));
    var result = surface;
    result.base_color = vec3(0.88, 0.26, 0.035) * stripe;
    result.emissive = vec3(0.08, 0.02, 0.0);
    return result;
}
