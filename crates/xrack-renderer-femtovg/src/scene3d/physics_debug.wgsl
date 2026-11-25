struct Camera {
    clip_rows: array<vec4<f32>, 4>,
};

@group(0) @binding(0) var<uniform> camera: Camera;

struct Output {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vertex(@location(0) world: vec3<f32>, @location(1) srgb: vec4<f32>) -> Output {
    let point = vec4<f32>(world, 1.0);
    let clip = vec4<f32>(
        dot(camera.clip_rows[0], point),
        dot(camera.clip_rows[1], point),
        dot(camera.clip_rows[2], point),
        dot(camera.clip_rows[3], point),
    );
    let linear = select(
        srgb.rgb / 12.92,
        pow((srgb.rgb + 0.055) / 1.055, vec3<f32>(2.4)),
        srgb.rgb > vec3<f32>(0.04045),
    );
    return Output(clip, vec4<f32>(linear, srgb.a));
}

@fragment
fn fragment(input: Output) -> @location(0) vec4<f32> {
    return input.color;
}
