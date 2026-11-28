struct XrSkyCamera {
    clip0: vec4<f32>,
    clip1: vec4<f32>,
    forward: vec4<f32>,
    lens: vec4<f32>,
    eye: vec4<f32>,
    custom_viewer: vec4<f32>,
    inverse: array<vec4<f32>, 4>,
}

struct XrSkyRay {
    origin: vec3<f32>,
    direction: vec3<f32>,
}

fn xr_sky_custom_forward(viewer: vec4<f32>, world: vec3<f32>) -> vec3<f32> {
    if viewer.w == 0.0 {
        return viewer.xyz;
    }
    return normalize((world - viewer.xyz) * viewer.w);
}

fn xr_sky_ray(camera: XrSkyCamera, ndc: vec2<f32>, custom: bool) -> XrSkyRay {
    if custom {
        let p = vec4<f32>(ndc, 0.0, 1.0);
        let inverse = camera.inverse;
        let origin = vec3(dot(inverse[0], p), dot(inverse[1], p), dot(inverse[2], p))
            / dot(inverse[3], p);
        return XrSkyRay(origin, xr_sky_custom_forward(camera.custom_viewer, origin));
    }
    let forward = camera.forward.xyz;
    let lens = camera.lens;
    let plane = (ndc - lens.zw) / lens.xy;
    let orthographic = camera.eye.w > 0.5;
    let shift = select(lens.zw, vec2(0.0), orthographic);
    let right = (camera.clip0.xyz - shift.x * forward) / lens.x;
    let up = (camera.clip1.xyz - shift.y * forward) / lens.y;
    let offset = plane.x * right + plane.y * up;
    if orthographic {
        return XrSkyRay(camera.eye.xyz + offset, forward);
    }
    return XrSkyRay(camera.eye.xyz, normalize(forward + offset));
}
