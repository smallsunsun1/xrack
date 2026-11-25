fn scene_vertex(input: XrVertexInput) -> vec3<f32> {
    if input.parameters.z > 0.5 {
        let index_offset = select(0.0, 0.8, input.instance_index == 2u);
        let y = index_offset + input.instance_transform[3].x * 0.4 + input.instance_color.g * 0.7;
        return vec3(0.0, y, 0.0);
    }
    return vec3(0.0, input.instance_data.w, 0.0);
}

fn scene_material(input: XrMaterialInput, surface: XrMaterialSurface) -> XrMaterialSurface {
    var output = surface;
    if input.parameters.z > 0.5 {
        output.base_color = vec3(f32(input.instance_index) * 0.25, input.instance_color.r, input.instance_color.b);
    } else {
        output.base_color *= 1.0 - clamp(input.instance_data.x, 0.0, 1.0) * 0.5;
    }
    return output;
}
