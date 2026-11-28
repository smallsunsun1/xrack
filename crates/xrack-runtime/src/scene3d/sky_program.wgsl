struct XrSkyProgramSettings {
    size: vec4<u32>,
    parameters: vec4<f32>,
}

@group(0) @binding(0) var<uniform> xr_sky_program: XrSkyProgramSettings;
@group(0) @binding(1) var xr_sky_output: texture_storage_2d_array<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn xr_generate_sky(@builtin(global_invocation_id) id: vec3<u32>) {
    let side = xr_sky_program.size.x;
    if any(id.xy >= vec2(side)) || id.z >= 6u {
        return;
    }
    let uv = (vec2<f32>(id.xy) + 0.5) * (2.0 / f32(side)) - 1.0;
    let ray = normalize(xr_cube_direction(id.z, uv.x, uv.y));
    let color = scene_sky(ray, xr_sky_program.parameters.xyz);
    // Reject NaN before it can spread through the filtered probe.
    let rgb = select(vec3(0.0), clamp(color, vec3(0.0), vec3(65504.0)),
        color >= vec3(0.0));
    textureStore(xr_sky_output, vec2<i32>(id.xy), i32(id.z), vec4(rgb, 1.0));
}
