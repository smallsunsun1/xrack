const gain: f32 = 0.25;

fn tint(value: f32) -> f32 {
    return value * gain;
}

fn scene_material(input: XrMaterialInput, surface: XrMaterialSurface) -> XrMaterialSurface {
    var result = surface;
    result.base_color = vec3(tint(1.0), tint(2.0), 0.0);
    return result;
}

fn scene_material_data(input: XrMaterialInput, surface: XrMaterialSurface) -> vec4<f32> {
    return vec4(tint(1.0), 0.0, 0.0, 1.0);
}
