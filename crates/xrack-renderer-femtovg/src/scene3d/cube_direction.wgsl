fn xr_cube_direction(face: u32, u: f32, v: f32) -> vec3<f32> {
    switch face {
        case 0u: { return vec3<f32>(1.0, -v, -u); }
        case 1u: { return vec3<f32>(-1.0, -v, u); }
        case 2u: { return vec3<f32>(u, 1.0, v); }
        case 3u: { return vec3<f32>(u, -1.0, -v); }
        case 4u: { return vec3<f32>(u, -v, 1.0); }
        default: { return vec3<f32>(-u, -v, -1.0); }
    }
}
