struct Grid {
    inverse: array<vec4<f32>, 4>,
    depth_row: vec4<f32>,
    // Plane height, inverse spacing, forward depth limit (0 for none), axes.
    options: vec4<f32>,
    color: vec4<f32>,
    axis_x: vec4<f32>,
    axis_z: vec4<f32>,
}

@group(0) @binding(0) var<uniform> grid: Grid;

struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) clip_xy: vec2<f32>,
}

@vertex
fn grid_vertex(@builtin(vertex_index) index: u32) -> Vertex {
    let clip = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0))[index];
    return Vertex(vec4(clip, 0.0, 1.0), clip);
}

struct Fragment {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

@fragment
fn grid_fragment(input: Vertex) -> Fragment {
    let clip = vec3(input.clip_xy, 1.0);
    let rows = grid.inverse;
    let start = vec4(dot(rows[0].xyw, clip), dot(rows[1].xyw, clip),
                     dot(rows[2].xyw, clip), dot(rows[3].xyw, clip));
    let along_depth = vec4(rows[0].z, rows[1].z, rows[2].z, rows[3].z);
    // Intersect in homogeneous space so an infinite far plane needs no divide.
    let height = grid.options.x;
    let divisor = height * along_depth.w - along_depth.y;
    let crosses = abs(divisor) > 1e-12;
    let depth = (start.y - height * start.w) / select(1.0, divisor, crosses);
    let hit = start + clamp(depth, 0.0, 1.0) * along_depth;
    let world = hit.xyz / max(hit.w, 1e-10);

    let cell = world.xz * grid.options.y;
    let width = fwidth(cell);
    let safe_width = max(width, vec2(1e-7));
    let dist = abs(fract(cell + vec2(0.5)) - vec2(0.5)) / safe_width;
    let detail = vec2(1.0) - smoothstep(vec2(0.20), vec2(0.55), width);
    let lines = (vec2(1.0) - smoothstep(vec2(0.30), vec2(1.05), dist)) * detail;
    let axis_dist = abs(cell) / safe_width;
    let axes = grid.options.w *
        (vec2(1.0) - smoothstep(vec2(0.45), vec2(1.2), axis_dist)) * detail;
    let along_view = dot(grid.depth_row, vec4(world, 1.0));
    let far_limit = grid.options.z;
    let far_fade = 1.0 - smoothstep(0.80, 1.0, along_view / max(far_limit, 1e-6));
    let coverage = max(max(lines.x, lines.y), max(axes.x, axes.y));
    let alpha = coverage * grid.color.a * select(far_fade, 1.0, far_limit == 0.0);
    if !crosses || !(depth > 0.0 && depth < 1.0) || hit.w <= 0.0 || !(alpha > 0.0) {
        discard;
    }
    let color = mix(mix(grid.color.rgb, grid.axis_x.rgb, axes.y), grid.axis_z.rgb, axes.x);
    return Fragment(vec4(color * alpha, alpha), depth);
}
