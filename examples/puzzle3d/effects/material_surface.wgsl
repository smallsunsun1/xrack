fn scene_material(input: XrMaterialInput, surface: XrMaterialSurface) -> XrMaterialSurface {
    let stripe = smoothstep(0.2, 0.5, fract(input.uv0.x * 8.0 + input.uv1.y * 0.6));
    let color = mix(vec3(0.025, 0.28, 0.72), vec3(0.06, 0.82, 0.92), stripe);
    let amount = clamp(input.parameters.x, 0.0, 1.0);
    var result = surface;
    result.base_color = mix(surface.base_color, color, amount);
    result.emissive += color * amount * 0.55;
    result.roughness = mix(surface.roughness, stripe * 0.55 + 0.16, amount);
    result.metallic = mix(surface.metallic, 0.42, amount);
    result.normal = surface.normal + vec3((stripe - 0.5) * 0.16 * amount, 0.0, 0.0);
    return result;
}
