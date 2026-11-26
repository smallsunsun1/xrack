fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    if uv.x < 1.0 / 3.0 {
        let parameters = scene_effect_parameters();
        if parameters.y > 2.5 {
            let horizontal = parameters.x < 0.0;
            let diagonal = sqrt(0.5);
            let dx = select(vec2(1.0 / 256.0, 0.0), vec2(diagonal, diagonal / 128.0), horizontal);
            let dy = select(vec2(0.0, 2.0), vec2(diagonal, -diagonal / 128.0), horizontal);
            return scene_buffer_color_grad(0u, vec2(0.25, 0.5), dx, dy);
        }
        let source_uv = vec2(select(0.25, 0.50048828125, parameters.y > 1.5), 0.5);
        if parameters.y > 0.5 {
            return scene_buffer_color_grad(
                0u, source_uv, scene_uv_dx() * parameters.z, scene_uv_dy() * parameters.z);
        }
        return scene_buffer_color_lod(0u, source_uv, parameters.x);
    }
    if uv.x < 2.0 / 3.0 {
        return scene_buffer_color(1u, uv);
    }
    return scene_buffer_color(2u, uv);
}
