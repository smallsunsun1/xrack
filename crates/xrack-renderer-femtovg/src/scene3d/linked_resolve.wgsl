struct LinkedFragment {
    depth: f32,
    red_green: u32,
    blue_alpha: u32,
    next: u32,
}

struct LinkedRegion {
    origin: vec2<u32>,
    size: vec2<u32>,
}

@group(0) @binding(5) var<storage, read> xr_linked_heads: array<u32>;
@group(0) @binding(6) var<storage, read> xr_linked_fragments: array<LinkedFragment>;
@group(0) @binding(7) var<uniform> xr_linked_region: LinkedRegion;

fn nearer(a: LinkedFragment, b: LinkedFragment) -> bool {
    // Coplanar fragments get a deterministic tie-breaker even if GPU
    // scheduling changes the order in which the list was built.
    return a.depth < b.depth || (a.depth == b.depth
        && (a.red_green < b.red_green || (a.red_green == b.red_green && a.blue_alpha < b.blue_alpha)));
}

fn unpack_layer(layer: LinkedFragment) -> vec4<f32> {
    return vec4<f32>(unpack2x16float(layer.red_green), unpack2x16float(layer.blue_alpha));
}

// Negative alpha signals a list that exceeded the local sorting budget.
fn resolve_list(head: u32) -> vec4<f32> {
    var index = xr_linked_heads[head];
    if index == 0u {
        return vec4<f32>(0.0);
    }
    let first = xr_linked_fragments[index];
    if first.next == 0u {
        return unpack_layer(first);
    }
    let second = xr_linked_fragments[first.next];
    if second.next == 0u {
        let first_color = unpack_layer(first);
        let second_color = unpack_layer(second);
        if nearer(first, second) {
            return first_color + second_color * (1.0 - first_color.a);
        }
        return second_color + first_color * (1.0 - second_color.a);
    }
    const MAX_FRAGMENTS = 32u;
    var sorted: array<LinkedFragment, MAX_FRAGMENTS>;
    var count = 0u;
    while index != 0u && count < MAX_FRAGMENTS {
        let fragment = xr_linked_fragments[index];
        var insert = count;
        while insert > 0u {
            if !nearer(fragment, sorted[insert - 1u]) {
                break;
            }
            sorted[insert] = sorted[insert - 1u];
            insert -= 1u;
        }
        sorted[insert] = fragment;
        count += 1u;
        index = fragment.next;
    }
    if index != 0u {
        return vec4<f32>(0.0, 0.0, 0.0, -1.0);
    }
    var color = vec4<f32>(0.0);
    for (var slot = 0u; slot < count; slot += 1u) {
        color += unpack_layer(sorted[slot]) * (1.0 - color.a);
    }
    return color;
}

@fragment
fn fragment_linked(@builtin(position) point: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<u32>(point.xy);
    let local = pixel - xr_linked_region.origin;
    if all(local < xr_linked_region.size) && xr_linked_heads[0] < arrayLength(&xr_linked_fragments) {
        let layer = resolve_list(1u + local.y * xr_linked_region.size.x + local.x);
        if layer.a >= 0.0 {
            return layer;
        }
    }
    return compose(textureLoad(accumulation, vec2<i32>(pixel), 0),
                   textureLoad(revealage, vec2<i32>(pixel), 0).r);
}

@fragment
fn fragment_linked_multisampled(@builtin(position) point: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(point.xy);
    let samples = textureNumSamples(samples_opaque);
    let local = vec2<u32>(pixel) - xr_linked_region.origin;
    let inside = all(local < xr_linked_region.size);
    let start = 1u + (local.y * xr_linked_region.size.x + local.x) * samples;
    let use_linked = inside && xr_linked_heads[0] < arrayLength(&xr_linked_fragments);
    var result = vec4<f32>(0.0);
    for (var sample = 0u; sample < samples; sample += 1u) {
        let opaque = textureLoad(samples_opaque, pixel, sample);
        if !inside {
            result += opaque;
            continue;
        }
        var layer = vec4<f32>(0.0, 0.0, 0.0, -1.0);
        if use_linked {
            layer = resolve_list(start + sample);
        }
        if layer.a < 0.0 {
            layer = compose(textureLoad(samples_accumulation, pixel, sample),
                            textureLoad(samples_revealage, pixel, sample).r);
        }
        result += layer + opaque * (1.0 - layer.a);
    }
    return result / f32(samples);
}
