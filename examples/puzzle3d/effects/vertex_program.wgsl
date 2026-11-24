fn scene_vertex(input: XrVertexInput) -> vec3<f32> {
    let wave = sin(input.position.x * 4.0) * input.parameters.y;
    return vec3(input.parameters.x, wave, 0.0);
}
