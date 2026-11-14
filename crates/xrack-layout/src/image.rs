use crate::{Rect, Size};
use xrack_runtime::{
    ImageFit, ImageHorizontalAlignment as Horizontal, ImageRendering, ImageTileMode,
    ImageVerticalAlignment as Vertical, NineSlice, Node, Value,
};

/// The visible part of an image in integer source pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl ImageRegion {
    pub fn full([width, height]: [u32; 2]) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }

    pub fn for_node(node: &Node, size: [u32; 2]) -> Self {
        fn axis(start: Option<i64>, extent: Option<i64>, limit: u32) -> (u32, u32) {
            let start = start.unwrap_or(0);
            let limit = i64::from(limit);
            let end = extent.map_or(limit, |extent| start.saturating_add(extent.max(0)));
            let start = start.clamp(0, limit);
            (start as u32, (end.clamp(start, limit) - start) as u32)
        }
        let (x, width) = axis(
            node.int("source-clip-x"),
            node.int("source-clip-width"),
            size[0],
        );
        let (y, height) = axis(
            node.int("source-clip-y"),
            node.int("source-clip-height"),
            size[1],
        );
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn size(self) -> [u32; 2] {
        [self.width, self.height]
    }

    pub fn logical_size(self) -> Size {
        Size::new(self.width as f32, self.height as f32)
    }

    /// Preserve edge bands that survive the clip. If an axis shows only one
    /// border band, that band becomes the flexible region on that axis.
    pub fn clipped_edges(self, [width, height]: [u32; 2], edges: NineSlice) -> NineSlice {
        fn axis(start: u32, extent: u32, limit: u32, before: u32, after: u32) -> [u32; 2] {
            let first = before.saturating_sub(start).min(extent);
            let last = start
                .saturating_add(extent)
                .saturating_sub(limit.saturating_sub(after))
                .min(extent);
            if first.saturating_add(last) >= extent {
                [0, 0]
            } else {
                [first, last]
            }
        }
        let [left, right] = axis(self.x, self.width, width, edges.left, edges.right);
        let [top, bottom] = axis(self.y, self.height, height, edges.top, edges.bottom);
        NineSlice {
            left,
            right,
            top,
            bottom,
        }
    }
}

/// Presentation options shared by atlas regions, animations and full images.
#[derive(Clone, Copy)]
pub struct ImagePresentation {
    pub region: ImageRegion,
    pub modes: [ImageTileMode; 2],
    pub rendering: ImageRendering,
    fit: ImageFit,
    alignment: [f32; 2],
}

impl ImagePresentation {
    pub fn whole(size: [u32; 2], fit: ImageFit) -> Self {
        Self {
            region: ImageRegion::full(size),
            modes: [ImageTileMode::Stretch; 2],
            rendering: ImageRendering::Smooth,
            fit,
            alignment: [0.5; 2],
        }
    }

    pub fn for_node(node: &Node, size: [u32; 2], in_layout: bool) -> Self {
        let x = match node.properties.get("horizontal-alignment") {
            Some(Value::ImageHorizontalAlignment(Horizontal::Left)) => 0.0,
            Some(Value::ImageHorizontalAlignment(Horizontal::Right)) => 1.0,
            _ => 0.5,
        };
        let y = match node.properties.get("vertical-alignment") {
            Some(Value::ImageVerticalAlignment(Vertical::Top)) => 0.0,
            Some(Value::ImageVerticalAlignment(Vertical::Bottom)) => 1.0,
            _ => 0.5,
        };
        let rendering = match node.properties.get("image-rendering") {
            Some(Value::ImageRendering(rendering)) => *rendering,
            _ => ImageRendering::Smooth,
        };
        Self {
            region: ImageRegion::for_node(node, size),
            modes: [
                node.image_tile_mode("horizontal-tile-mode"),
                node.image_tile_mode("vertical-tile-mode"),
            ],
            rendering,
            fit: node.image_fit("image-fit").unwrap_or(
                if in_layout || node.kind == "MediaPlayer" {
                    ImageFit::Contain
                } else {
                    ImageFit::Fill
                },
            ),
            alignment: [x, y],
        }
    }

    pub fn placement(self, bounds: Rect) -> ImagePlacement {
        let source = self.region.logical_size();
        let mut pattern = fitted_rect(bounds, source, self.fit, self.alignment);
        let repeat = self.modes.map(|mode| mode != ImageTileMode::Stretch);
        let mut visible = pattern.intersect(bounds);
        if repeat[0] {
            pattern.x = bounds.x;
            pattern.width = tile_extent(bounds.width, source.width, self.modes[0]);
            visible.x = bounds.x;
            visible.width = bounds.width;
        }
        if repeat[1] {
            pattern.y = bounds.y;
            pattern.height = tile_extent(bounds.height, source.height, self.modes[1]);
            visible.y = bounds.y;
            visible.height = bounds.height;
        }
        ImagePlacement {
            pattern,
            visible,
            repeat,
        }
    }
}

pub struct ImagePlacement {
    pub pattern: Rect,
    pub visible: Rect,
    pub repeat: [bool; 2],
}

pub fn tile_extent(available: f32, natural: f32, mode: ImageTileMode) -> f32 {
    match mode {
        ImageTileMode::Stretch => available,
        ImageTileMode::Repeat => natural,
        ImageTileMode::Round => available / (available / natural).ceil().max(1.0),
    }
}

pub fn fitted_rect(bounds: Rect, source: Size, fit: ImageFit, align: [f32; 2]) -> Rect {
    let (width, height) = match fit {
        ImageFit::Fill => return bounds,
        ImageFit::Preserve => (source.width, source.height),
        ImageFit::Contain | ImageFit::Cover => {
            let x = bounds.width / source.width;
            let y = bounds.height / source.height;
            let scale = if fit == ImageFit::Contain {
                x.min(y)
            } else {
                x.max(y)
            };
            (source.width * scale, source.height * scale)
        }
    };
    Rect::new(
        bounds.x + (bounds.width - width) * align[0],
        bounds.y + (bounds.height - height) * align[1],
        width,
        height,
    )
}
