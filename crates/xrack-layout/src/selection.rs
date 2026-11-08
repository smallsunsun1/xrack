use crate::{Rect, font_size};
use xrack_runtime::{Node, Orientation};

pub(crate) fn radio_title_height(node: &Node) -> f32 {
    if node.kind == "RadioGroup" && node.text("title").is_some_and(|title| !title.is_empty()) {
        font_size(node) * 1.4 + 6.0
    } else {
        0.0
    }
}

pub(crate) struct TabGeometry {
    bar: Rect,
    pub(crate) content: Rect,
    orientation: Orientation,
}

impl TabGeometry {
    pub(crate) fn new(bounds: Rect, orientation: Orientation) -> Self {
        let (bar, content) = match orientation {
            Orientation::Horizontal => {
                let height = bounds.height.min(34.0);
                (
                    Rect::new(bounds.x, bounds.y, bounds.width, height),
                    Rect::new(
                        bounds.x,
                        bounds.y + height,
                        bounds.width,
                        bounds.height - height,
                    ),
                )
            }
            Orientation::Vertical => {
                let width = (bounds.width * 0.4).min(124.0);
                (
                    Rect::new(bounds.x, bounds.y, width, bounds.height),
                    Rect::new(
                        bounds.x + width,
                        bounds.y,
                        bounds.width - width,
                        bounds.height,
                    ),
                )
            }
        };
        Self {
            bar,
            content,
            orientation,
        }
    }

    pub(crate) fn header(&self, index: usize, count: usize) -> Rect {
        let count = count.max(1) as f32;
        match self.orientation {
            Orientation::Horizontal => {
                let width = self.bar.width / count;
                Rect::new(
                    self.bar.x + width * index as f32,
                    self.bar.y,
                    width,
                    self.bar.height,
                )
            }
            Orientation::Vertical => {
                let height = (self.bar.height / count).min(36.0);
                Rect::new(
                    self.bar.x,
                    self.bar.y + height * index as f32,
                    self.bar.width,
                    height,
                )
            }
        }
    }
}
