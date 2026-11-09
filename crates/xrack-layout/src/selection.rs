use crate::{Rect, ScrollLayout, Size, TextMeasurer, linear::Axis};
use xrack_runtime::{Node, Orientation, group_index};

pub const TAB_LABEL_SIZE: f32 = 14.0;

pub(crate) fn tab_bar(
    node: &Node,
    bounds: Rect,
    text: &mut impl TextMeasurer,
    requested: f32,
    reveal: bool,
) -> (Vec<Rect>, ScrollLayout) {
    let (axis, size_property) = match node.orientation() {
        Orientation::Horizontal => (Axis::Horizontal, "width"),
        Orientation::Vertical => (Axis::Vertical, "height"),
    };
    let size = Size::new(bounds.width, bounds.height);
    let extent = axis.extent(size);
    let cross_extent = axis.other().extent(size);
    let spacing = node.length("spacing").unwrap_or(0.0).max(0.0);
    let sizes: Vec<_> = node
        .children
        .iter()
        .map(|tab| {
            if !tab.is_visible() {
                return None;
            }
            let explicit = tab.relative_length(size_property, extent);
            let size = explicit.unwrap_or_else(|| match axis {
                Axis::Horizontal => {
                    let label = tab.text("text").unwrap_or("");
                    let font_size = text
                        .default_font_size()
                        .unwrap_or(TAB_LABEL_SIZE)
                        .clamp(1.0, (cross_extent / 1.4).max(1.0));
                    (text.measure(label, font_size, f32::INFINITY).width + 24.0).max(48.0)
                }
                Axis::Vertical => 36.0,
            });
            (size > 0.0).then_some((size, explicit.is_none()))
        })
        .collect();
    let gap_count = sizes.iter().flatten().count().saturating_sub(1);
    let minimum =
        sizes.iter().flatten().map(|(size, _)| *size).sum::<f32>() + gap_count as f32 * spacing;
    let flexible = sizes
        .iter()
        .flatten()
        .filter(|(_, flexible)| *flexible)
        .count();
    let extra = (extent - minimum).max(0.0) / flexible.max(1) as f32;
    let mut start = 0.0;
    let rects: Vec<_> = sizes
        .into_iter()
        .map(|size| {
            let Some((size, flexible)) = size else {
                return Rect::new(bounds.x, bounds.y, 0.0, 0.0);
            };
            let size = size + if flexible { extra } else { 0.0 };
            let rect = axis.rect(bounds, start, 0.0, size, cross_extent);
            start += size + spacing;
            rect
        })
        .collect();
    let max_offset = (minimum - extent).max(0.0);
    let translate = |mut tab: Rect, offset| {
        match axis {
            Axis::Horizontal => tab.x -= offset,
            Axis::Vertical => tab.y -= offset,
        }
        tab
    };
    let mut scroll = ScrollLayout::new(requested, max_offset);
    if reveal && let Some(tab) = group_index(node).and_then(|index| rects.get(index)) {
        let offset = tab_reveal_offset(
            bounds,
            translate(*tab, scroll.offset),
            node.orientation(),
            scroll.offset,
        );
        scroll = ScrollLayout::new(offset, max_offset);
    }
    let rects = rects
        .into_iter()
        .map(|tab| translate(tab, scroll.offset))
        .collect();
    (rects, scroll)
}

pub(crate) fn tab_reveal_offset(
    viewport: Rect,
    tab: Rect,
    orientation: Orientation,
    offset: f32,
) -> f32 {
    let (start, extent, available) = match orientation {
        Orientation::Horizontal => (tab.x - viewport.x, tab.width, viewport.width),
        Orientation::Vertical => (tab.y - viewport.y, tab.height, viewport.height),
    };
    let start = start + offset;
    offset
        .max(start + extent.min(available) - available)
        .min(start)
}

pub(crate) fn radio_title_height(node: &Node, size: f32) -> f32 {
    if node.kind == "RadioGroup" && node.text("title").is_some_and(|title| !title.is_empty()) {
        size * 1.4 + 6.0
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
