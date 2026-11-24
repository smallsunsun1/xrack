fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    let color = scene_color(uv);
    if scene_depth(uv) >= 1.0 {
        return color;
    }
    let pixel = 2.0 / scene_size();
    let neighbor = max(
        max(scene_depth(uv + vec2(pixel.x, 0.0)), scene_depth(uv - vec2(pixel.x, 0.0))),
        max(scene_depth(uv + vec2(0.0, pixel.y)), scene_depth(uv - vec2(0.0, pixel.y)))
    );
    let stripe = 0.72 + 0.28 * cos(uv.y * scene_size().y * 1.2);
    let tint = vec3(0.06, 0.81, 0.95);
    let lit = mix(color.rgb, tint, 0.66) * stripe;
    return vec4(select(lit, tint, neighbor >= 1.0), color.a);
}
