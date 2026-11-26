struct XrSkyRadiance {
    sky_horizon: vec4<f32>,
    sky_top: vec4<f32>,
    ground_horizon: vec4<f32>,
    ground_bottom: vec4<f32>,
    // Solar axis and cosines of the outer and inner angles.
    sun: vec4<f32>,
    sun_radiance: vec4<f32>,
    sun_curve: vec4<f32>,
}

fn xr_sky_gradient(sky: XrSkyRadiance, direction: vec3<f32>) -> vec3<f32> {
    if direction.y < 0.0 {
        return mix(sky.ground_horizon.rgb, sky.ground_bottom.rgb,
            pow(clamp(-direction.y, 0.0, 1.0), sky.ground_horizon.a));
    }
    return mix(sky.sky_horizon.rgb, sky.sky_top.rgb,
        pow(clamp(direction.y, 0.0, 1.0), sky.sky_horizon.a));
}

fn xr_sky_sun_profile(sky: XrSkyRadiance, cosine: f32) -> f32 {
    let t = clamp((cosine - sky.sun.w) / max(sky.sun_radiance.w - sky.sun.w, 1e-7), 0.0, 1.0);
    return pow(t * t * (3.0 - 2.0 * t), sky.sun_curve.x);
}

fn xr_sky_radiance(sky: XrSkyRadiance, direction: vec3<f32>) -> vec3<f32> {
    return xr_sky_gradient(sky, direction)
        + xr_sky_sun_profile(sky, dot(direction, sky.sun.xyz)) * sky.sun_radiance.rgb;
}
