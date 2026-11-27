// Shared surface and particle light layout and punctual attenuation.
struct Light {
    position_kind: vec4<f32>,
    axis_inverse_range: vec4<f32>,
    // RGB radiance and the one-based index of the shadow light.
    radiance: vec4<f32>,
    // Cone scale, cone offset, and fully baked light flag.
    cone: vec3<f32>,
    scope: u32,
}

struct Lighting {
    ambient: vec4<f32>,
    // Active light count and explicit light list flag.
    settings: vec4<f32>,
    lights: array<Light, 16>,
    // Strength, signed radius (negative for world units), taps and bias.
    shadow_sampling: array<vec4<f32>, 4>,
}

struct IncidentLight {
    axis: vec3<f32>,
    radiance: vec3<f32>,
    attenuation: f32,
}

fn incident_light(light: Light, world: vec3<f32>) -> IncidentLight {
    let kind = light.position_kind.w;
    if kind == 0.0 {
        return IncidentLight(light.axis_inverse_range.xyz, light.radiance.xyz, 1.0);
    }
    let delta = light.position_kind.xyz - world;
    let distance_squared = dot(delta, delta);
    let none = IncidentLight(vec3<f32>(0.0), vec3<f32>(0.0), 0.0);
    if distance_squared <= 0.000001 {
        return none;
    }
    var fade = 1.0;
    if light.axis_inverse_range.w > 0.0 {
        let fraction = distance_squared * light.axis_inverse_range.w;
        if fraction >= 1.0 {
            return none;
        }
        fade = 1.0 - fraction * fraction;
    }
    let axis = delta * inverseSqrt(distance_squared);
    if kind == 2.0 {
        let cosine = dot(axis, light.axis_inverse_range.xyz);
        let cone = clamp(cosine * light.cone.x + light.cone.y, 0.0, 1.0);
        if cone == 0.0 {
            return none;
        }
        fade *= cone * cone;
    }
    let attenuation = fade / max(distance_squared, 0.0001);
    return IncidentLight(axis, min(light.radiance.xyz * attenuation, vec3<f32>(1000000.0)), attenuation);
}
