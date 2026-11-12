use crate::dimension;
use xrack_runtime::{ImageAsset, Node, button_text};

/// Shared intrinsic content for button measurement and painting.
pub struct ButtonContent<'a> {
    pub label: &'a str,
    pub icon: Option<(ImageAsset, f32)>,
    pub padding: f32,
}

impl<'a> ButtonContent<'a> {
    pub fn new(node: &'a Node) -> Self {
        let size = dimension(node, "icon-size").unwrap_or(18.0);
        let icon = node
            .image_asset("icon")
            .filter(|image| size > 0.0 && !image.size().contains(&0))
            .map(|image| (image, size));
        let label = if icon.is_some() && node.boolean("icon-only", false) {
            ""
        } else {
            button_text(node)
        };
        Self {
            label,
            icon,
            padding: dimension(node, "padding").unwrap_or(if node.kind == "ToolButton" {
                8.0
            } else {
                10.0
            }),
        }
    }

    pub fn gap(&self) -> f32 {
        if self.icon.is_some() && !self.label.is_empty() {
            6.0
        } else {
            0.0
        }
    }
}
