fn position_at(slot: u32, uv: vec2<f32>) -> XrCapturePosition {
    return material_texture_world_position(slot, uv);
}

fn scene_material(input: XrMaterialInput, surface: XrMaterialSurface) -> XrMaterialSurface {
    var result = surface;
    let point = position_at(u32(input.parameters.x), input.uv0);
    if position_at(2u, input.uv0).valid || position_at(99u, input.uv0).valid {
        result.base_color = vec3(1.0, 0.0, 0.0);
    } else if point.valid {
        result.base_color = point.position * vec3(0.13, 0.125, 0.16) + vec3(0.5, 0.5, 0.0);
    } else {
        result.base_color = vec3(0.035, 0.055, 0.07);
    }
    return result;
}
