fn scene_material_data_at(
    input: XrMaterialInput, surface: XrMaterialSurface, index: u32,
) -> vec4<f32> {
    switch index {
        case 0u: { return vec4(0.25, 0.0, 0.0, 1.0); }
        case 1u: { return vec4(0.50, 0.0, 0.0, 1.0); }
        default: { return vec4(131072.0, 0.0, 0.0, 1.0); }
    }
}
