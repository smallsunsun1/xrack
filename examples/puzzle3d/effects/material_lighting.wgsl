fn scene_light(input: XrLightInput, response: XrLightResponse) -> XrLightResponse {
    let parameters = input.shading.material.parameters;
    var tint = vec3(1.9, 0.19, 0.08);
    switch input.kind {
        case XR_POINT_LIGHT: { tint = vec3(0.12, 1.7, 0.26); }
        case XR_SPOT_LIGHT: { tint = vec3(0.16, 0.4, 1.95); }
        default: {}
    }
    let cosine = max(dot(input.shading.surface.normal, input.direction), 0.0);
    let highlight = vec3(0.05, 0.16, 1.0) * input.fresnel * (cosine * parameters.y * 6.0);
    return XrLightResponse(response.diffuse * mix(vec3(1.0), tint, parameters.x),
        response.specular + highlight);
}

fn scene_ambient(input: XrAmbientInput, diffuse: vec3<f32>) -> vec3<f32> {
    let amount = input.shading.material.parameters.x;
    return diffuse * mix(vec3(1.0), vec3(2.3, 1.15, 0.12), amount);
}
