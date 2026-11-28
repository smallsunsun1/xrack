struct XrSkyScreenSettings {
    // Width, height, and custom projection flag.
    size: vec4<u32>,
    parameters: vec4<f32>,
    basis: array<vec4<f32>, 3>,
    camera: XrSkyCamera,
}

@group(0) @binding(0) var<uniform> xr_sky_screen: XrSkyScreenSettings;
@group(0) @binding(1) var xr_sky_screen_output: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn xr_generate_screen_sky(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= xr_sky_screen.size.xy) {
        return;
    }
    let uv = (vec2<f32>(id.xy) + 0.5) / vec2<f32>(xr_sky_screen.size.xy);
    let ndc = vec2(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let ray = xr_sky_ray(xr_sky_screen.camera, ndc, xr_sky_screen.size.z != 0u);
    let basis = xr_sky_screen.basis;
    let direction = normalize(vec3(dot(ray.direction, basis[0].xyz),
        dot(ray.direction, basis[1].xyz), dot(ray.direction, basis[2].xyz)));
    let color = scene_sky(direction, xr_sky_screen.parameters.xyz);
    // Exposure and fog are applied when the viewport samples this image.
    let radiance = select(vec3(0.0), clamp(color, vec3(0.0), vec3(65504.0)), color >= vec3(0.0));
    textureStore(xr_sky_screen_output, vec2<i32>(id.xy), vec4(radiance, 1.0));
}
