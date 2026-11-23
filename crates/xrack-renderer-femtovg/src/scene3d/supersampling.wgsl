@group(0) @binding(0) var scene_color: texture_2d<f32>;
@group(0) @binding(1) var<uniform> output_size: vec2<u32>;

@vertex
fn vertex(@builtin(vertex_index) id: u32) -> @builtin(position) vec4<f32> {
    let x = f32((id << 1u) & 2u);
    let y = f32(id & 2u);
    return vec4<f32>(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let output_pixel = vec2<u32>(position.xy);
    let input_size = textureDimensions(scene_color);
    if all(input_size == output_size * 2u) {
        let p = vec2<i32>(output_pixel * 2u);
        return 0.25 * (textureLoad(scene_color, p, 0)
            + textureLoad(scene_color, p + vec2<i32>(1, 0), 0)
            + textureLoad(scene_color, p + vec2<i32>(0, 1), 0)
            + textureLoad(scene_color, p + vec2<i32>(1, 1), 0));
    }

    // In these integer coordinates each source texel has extent output_size
    // while each destination pixel has extent input_size.
    let start = output_pixel * input_size;
    let end = start + input_size;
    let first = start / output_size;
    let last = (end - 1u) / output_size;
    var sum = vec4<f32>(0.0);
    for (var y = first.y; y <= last.y; y++) {
        let weight_y = min(end.y, (y + 1u) * output_size.y) - max(start.y, y * output_size.y);
        for (var x = first.x; x <= last.x; x++) {
            let weight_x = min(end.x, (x + 1u) * output_size.x) - max(start.x, x * output_size.x);
            sum += textureLoad(scene_color, vec2<i32>(i32(x), i32(y)), 0) * f32(weight_x * weight_y);
        }
    }
    return sum / (f32(input_size.x) * f32(input_size.y));
}
