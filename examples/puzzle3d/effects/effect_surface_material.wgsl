fn scene_vertex(input: XrVertexInput) -> vec3<f32> {
    return vec3(0.0, 0.0, input.parameters.x);
}

fn scene_material(input: XrMaterialInput, surface: XrMaterialSurface) -> XrMaterialSurface {
    var result = surface;
    result.normal = vec3(0.0, 0.6, 0.8);
    result.roughness = 0.2;
    return result;
}
