//! Portable triangle-edge coverage for devices without native polygon lines.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(super) enum RasterMode {
    #[default]
    Fill,
    NativeLine,
    ShaderLine,
}

impl RasterMode {
    pub fn select(device: &wgpu::Device, line: bool) -> Self {
        if !line {
            Self::Fill
        } else if device
            .features()
            .contains(wgpu::Features::POLYGON_MODE_LINE)
        {
            Self::NativeLine
        } else {
            Self::ShaderLine
        }
    }

    pub fn polygon_mode(self) -> wgpu::PolygonMode {
        match self {
            Self::NativeLine => wgpu::PolygonMode::Line,
            Self::Fill | Self::ShaderLine => wgpu::PolygonMode::Fill,
        }
    }
}

const EDGE_FUNCTIONS: &str = r#"
fn xr_wire_corner(vertex: u32) -> vec3<f32> {
    let corner = vertex % 3u;
    return vec3(f32(corner == 0u), f32(corner == 1u), f32(corner == 2u));
}

fn xr_wire_distance(weights: vec3<f32>) -> f32 {
    let dx = dpdx(weights);
    let dy = dpdy(weights);
    let distance = abs(weights) * inverseSqrt(max(dx * dx + dy * dy, vec3(1e-12)));
    return min(distance.x, min(distance.y, distance.z));
}

fn xr_wire_discard(distance: f32) {
    if distance > 0.75 {
        discard;
    }
}
"#;

pub(super) fn mesh(source: &str) -> String {
    let stage = source
        .replace(
            "// XRack wireframe varying",
            "@location(8) @interpolate(linear) wire: vec3<f32>,",
        )
        .replace("// XRack wireframe corner", "xr_wire_corner(vertex.index),")
        .replace(
            "// XRack wireframe distance",
            "let wire_distance = xr_wire_distance(vertex.wire);",
        )
        .replace(
            "// XRack wireframe discard",
            "xr_wire_discard(wire_distance);",
        );
    format!("{EDGE_FUNCTIONS}\n{stage}")
}

pub(super) fn particles(source: &str) -> String {
    // Every consecutive triple of a triangle strip has all three corner IDs.
    let stage = source
        .replace(
            "// XRack wireframe varying",
            "@location(5) @interpolate(linear) wire: vec3<f32>,",
        )
        .replace("// XRack wireframe corner", "xr_wire_corner(vertex),")
        .replace("// XRack wireframe ribbon", "sprite.wire,")
        .replace(
            "// XRack wireframe discard",
            "xr_wire_discard(xr_wire_distance(sprite.wire));",
        );
    format!("{EDGE_FUNCTIONS}\n{stage}")
}
