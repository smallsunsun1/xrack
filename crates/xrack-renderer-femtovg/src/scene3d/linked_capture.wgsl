// Weighted attachments are still written in this pass so overflow has a
// complete fallback, independent of which fragment obtained the last slot.
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

@group(0) @binding(XR_LINKED_REGION_SLOT) var<uniform> xr_linked_region: LinkedRegion;
@group(0) @binding(XR_LINKED_HEADS_SLOT) var<storage, read_write> xr_linked_heads: array<atomic<u32>>;
@group(0) @binding(XR_LINKED_FRAGMENTS_SLOT) var<storage, read_write> xr_linked_fragments: array<LinkedFragment>;
@group(0) @binding(XR_LINKED_DEPTH_SLOT) var xr_linked_depth: XR_LINKED_DEPTH_TYPE;

override XR_LINKED_REVERSE_DEPTH: bool = false;
override XR_LINKED_EQUAL_DEPTH: bool = false;

fn xr_record_fragment(color: vec4<f32>, point: vec2<f32>, depth: f32, sample: u32) {
    if color.a <= 0.0 {
        return;
    }
    // Storage side effects can run before hardware depth testing. Sample the
    // same read-only depth attachment and apply the same comparison here.
    let pixel = vec2<u32>(point);
    let local = pixel - xr_linked_region.origin;
    if any(local >= xr_linked_region.size) {
        return;
    }
    let opaque = textureLoad(xr_linked_depth, vec2<i32>(pixel), XR_LINKED_DEPTH_INDEX);
    let in_front = select((depth < opaque), (depth > opaque), XR_LINKED_REVERSE_DEPTH);
    if !(in_front || (XR_LINKED_EQUAL_DEPTH && depth == opaque)) {
        return;
    }

    // Slot zero is the allocation counter in the head buffer and the null
    // link in the fragment buffer. A counter above capacity triggers fallback.
    let capacity = arrayLength(&xr_linked_fragments) - 1u;
    if atomicLoad(&xr_linked_heads[0]) > capacity {
        return;
    }
    let index = atomicAdd(&xr_linked_heads[0], 1u) + 1u;
    if index > capacity {
        return;
    }
    let pixel_index = local.y * xr_linked_region.size.x + local.x;
    let head = 1u + pixel_index * XR_LINKED_SAMPLES + sample;
    let previous = atomicExchange(&xr_linked_heads[head], index);
    let packed = clamp(color, vec4<f32>(0.0), vec4<f32>(65000.0, 65000.0, 65000.0, 1.0));
    xr_linked_fragments[index] = LinkedFragment(
        select(depth, -depth, XR_LINKED_REVERSE_DEPTH),
        pack2x16float(packed.rg), pack2x16float(packed.ba), previous,
    );
}
