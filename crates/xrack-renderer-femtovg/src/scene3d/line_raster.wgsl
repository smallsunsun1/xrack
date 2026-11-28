// The header holds the first expanded vertex and record stride, in words.
@group(3) @binding(4) var<storage, read> xr_line_vertices: array<u32>;

fn xr_line_peer_index(index: u32) -> u32 {
    let first = xr_line_vertices[0];
    return first + ((index - first) ^ 2u);
}

fn xr_line_peer_record(index: u32) -> u32 {
    let peer = xr_line_peer_index(index) - xr_line_vertices[0];
    return 4u + (peer / 2u) * xr_line_vertices[1];
}

fn xr_line_raw_peer(index: u32) -> vec3<f32> {
    let i = xr_line_peer_record(index);
    return bitcast<vec3<f32>>(vec3(xr_line_vertices[i], xr_line_vertices[i + 1u], xr_line_vertices[i + 2u]));
}

fn xr_line_words(i: u32) -> vec4<u32> {
    return vec4(xr_line_vertices[i], xr_line_vertices[i + 1u], xr_line_vertices[i + 2u], xr_line_vertices[i + 3u]);
}

fn xr_line_joints(a: u32, b: u32) -> vec4<u32> {
    return vec4(a & 65535u, a >> 16u, b & 65535u, b >> 16u);
}

fn xr_line_skin_weights(index: u32) -> SkinWeights {
    let info = xr_line_peer_record(index) + 4u;
    let joints = xr_line_words(info);
    return SkinWeights(xr_line_joints(joints.x, joints.y), xr_line_joints(joints.z, joints.w),
        bitcast<vec4<f32>>(xr_line_words(info + 4u)), bitcast<vec4<f32>>(xr_line_words(info + 8u)));
}

fn xr_line_local_peer(index: u32) -> vec3<f32> {
    var local = xr_line_raw_peer(index);
    // XRack line current morph
    // XRack line current skin
    return local;
}

fn xr_line_corner(clip: vec4<f32>, peer: vec4<f32>, index: u32, pixels: f32, viewport: vec2<f32>) -> vec4<f32> {
    // A common outside clip plane rejects the centerline, as for native lines.
    let left = clip.x < -clip.w && peer.x < -peer.w;
    let right = clip.x > clip.w && peer.x > peer.w;
    let below = clip.y < -clip.w && peer.y < -peer.w;
    let above = clip.y > clip.w && peer.y > peer.w;
    let near = clip.z < 0.0 && peer.z < 0.0;
    let far = clip.z > clip.w && peer.z > peer.w;
    if left || right || below || above || near || far {
        return clip;
    }
    let dimensions = max(viewport, vec2(1.0));
    let endpoint = (index - xr_line_vertices[0]) & 2u;
    let direction_sign = select(1.0, -1.0, endpoint != 0u);
    // This is proportional to the projected direction without dividing by W.
    // In particular it keeps the direction through a crossing of the near plane.
    let a = clip / max(max(max(abs(clip.x), abs(clip.y)), abs(clip.w)), 1e-20);
    let b = peer / max(max(max(abs(peer.x), abs(peer.y)), abs(peer.w)), 1e-20);
    let direction = direction_sign * (b.xy * a.w - a.xy * b.w) * dimensions;
    let length_squared = dot(direction, direction);
    if length_squared <= 1e-20 {
        return clip;
    }
    let across = vec2(-direction.y, direction.x) * inverseSqrt(length_squared);
    let side = f32(index & 1u) * 2.0 - 1.0;
    let extent = min(vec2(pixels) / dimensions, vec2(4.0));
    return vec4(clip.xy + across * side * extent * clip.w, clip.zw);
}
