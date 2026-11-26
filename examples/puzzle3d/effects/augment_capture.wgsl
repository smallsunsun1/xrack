const gain: f32 = 1.0;

struct Payload { gain: f32 }

fn tint(value: f32) -> f32 {
    let payload = Payload(value);
    return gain * payload.gain;
}

fn scene_capture_augment(
    input: XrMaterialInput, surface: XrMaterialSurface, index: u32,
) -> vec4<f32> {
    let parameters = capture_augment_parameters();
    let intensity = tint(surface.base_color.g) * parameters.x;
    return vec4(intensity, surface.base_color.r + parameters.y, parameters.z, 1.0);
}
