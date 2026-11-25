fn scene_vertex(input: XrVertexInput) -> vec3<f32> {
    return vec3(input.instance_data.w, 0.0, 0.0);
}

fn scene_material(input: XrMaterialInput, surface: XrMaterialSurface) -> XrMaterialSurface {
    var output = surface;
    output.base_color = input.instance_data.xyz;
    return output;
}
