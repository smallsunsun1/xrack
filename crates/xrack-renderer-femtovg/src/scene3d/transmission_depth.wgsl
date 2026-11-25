@group(0) @binding(0) var source_depth: texture_depth_2d;
@group(0) @binding(1) var multisampled_depth: texture_depth_multisampled_2d;
@group(0) @binding(2) var<storage, read_write> captured_depth: array<f32>;
override REVERSE_DEVICE_DEPTH: bool = false;

@compute @workgroup_size(8, 8)
fn single(@builtin(global_invocation_id) invocation: vec3<u32>) {
    let size = textureDimensions(source_depth);
    if all(invocation.xy < size) {
        captured_depth[invocation.y * size.x + invocation.x] =
            textureLoad(source_depth, vec2<i32>(invocation.xy), 0);
    }
}

@compute @workgroup_size(8, 8)
fn multisampled(@builtin(global_invocation_id) invocation: vec3<u32>) {
    let size = textureDimensions(multisampled_depth);
    if all(invocation.xy < size) {
        var closest = select(1.0, 0.0, REVERSE_DEVICE_DEPTH);
        for (var sample = 0u; sample < textureNumSamples(multisampled_depth); sample++) {
            let value = textureLoad(multisampled_depth, vec2<i32>(invocation.xy), sample);
            closest = select(min(closest, value), max(closest, value), REVERSE_DEVICE_DEPTH);
        }
        captured_depth[invocation.y * size.x + invocation.x] = closest;
    }
}
