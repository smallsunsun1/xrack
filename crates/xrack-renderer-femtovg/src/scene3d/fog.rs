//! Per-viewport fog setup for the existing material shading pass.
use crate::Node;

use super::linear_rgb;

#[derive(Default)]
pub(super) struct Fog {
    pub color_density: [f32; 4],
    pub depth: [f32; 4],
    pub height: [f32; 4],
}

impl Fog {
    pub(super) fn from_node(node: &Node, background: [f64; 3]) -> Self {
        let number = |name, default| {
            node.float(name)
                .filter(|value| value.is_finite())
                .unwrap_or(default)
        };
        let density = number("fog-density", 0.0).clamp(0.0, 1.0);
        if density == 0.0 {
            return Self::default();
        }
        let curve = |name| {
            let value = number(name, 1.0);
            if value > 0.0 {
                value.clamp(0.01, 16.0)
            } else {
                1.0
            }
        };
        let near = number("fog-near", 0.0).max(0.0);
        let far = number("fog-far", 100.0);
        let depth_span = far - near;
        let depth_scale = if node.boolean("fog-depth-enabled", true)
            && depth_span.is_finite()
            && depth_span > 0.0
        {
            depth_span.max(0.000001).recip()
        } else {
            0.0
        };
        let clear_y = number("fog-height-start", 10.0);
        let dense_y = number("fog-height-end", 0.0);
        let height_span = dense_y - clear_y;
        let height_scale = if node.boolean("fog-height-enabled", false)
            && height_span.is_finite()
            && height_span != 0.0
        {
            height_span.signum() / height_span.abs().max(0.000001)
        } else {
            0.0
        };
        if depth_scale == 0.0 && height_scale == 0.0 {
            return Self::default();
        }
        let transmission = if depth_scale > 0.0 && node.boolean("fog-transmit-enabled", false) {
            curve("fog-transmit-curve")
        } else {
            0.0
        };
        let color = node.color("fog-color").map_or(background, linear_rgb);
        let [r, g, b] = color.map(|channel| channel as f32);
        Self {
            color_density: [r, g, b, density],
            depth: [near, depth_scale, curve("fog-depth-curve"), transmission],
            height: [clear_y, height_scale, curve("fog-height-curve"), 0.0],
        }
    }
}
