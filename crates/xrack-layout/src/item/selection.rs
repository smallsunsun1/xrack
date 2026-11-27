use super::{Item, ItemPaint, MeasureContext};
use crate::{
    ButtonContent, Rect, ScrollLayout, Size, TextMeasurer,
    linear::{Axis, measure_replacement_content},
    place_in_direction,
};
use xrack_runtime::{LayoutDirection, Node, Orientation, group_index};

pub const TAB_LABEL_SIZE: f32 = 14.0;

#[doc(hidden)]
pub fn tab_font_size(tab: &Node, text: &(impl TextMeasurer + ?Sized), height: f32) -> f32 {
    crate::popup::PopupTypography::from_text(text)
        .size(tab, TAB_LABEL_SIZE)
        .min((height / 1.4).max(1.0))
}

fn close_target(enlarged: bool) -> f32 {
    if enlarged { 44.0 } else { 24.0 }
}

pub(crate) fn tab_close_rect(
    header: Rect,
    direction: LayoutDirection,
    enlarged: bool,
) -> Option<Rect> {
    let size = (header.height.min(header.width) - 6.0).min(close_target(enlarged));
    if size < 1.0 {
        return None;
    }
    let rect = Rect::new(
        header.x + header.width - size - 3.0,
        header.y + (header.height - size) * 0.5,
        size,
        size,
    );
    Some(place_in_direction(rect, header, direction))
}

fn scaled_tab_height(group: &Node, tab: &Node, text: &(impl TextMeasurer + ?Sized)) -> f32 {
    let minimum = if xrack_runtime::tab_is_closable(group, tab) {
        50.0
    } else {
        44.0
    };
    (tab_font_size(tab, text, f32::INFINITY) * 1.4 + 8.0).max(minimum)
}

fn tab_strip_height(node: &Node, text: &(impl TextMeasurer + ?Sized), baseline: f32) -> f32 {
    if text.text_scale() <= 1.0 {
        return baseline;
    }
    node.children
        .iter()
        .filter(|tab| tab.is_visible())
        .map(|tab| scaled_tab_height(node, tab, text))
        .fold(baseline, f32::max)
}

pub(crate) fn preferred_tab_view_size(node: &Node, text: &(impl TextMeasurer + ?Sized)) -> Size {
    match (node.kind, node.orientation()) {
        ("TabBar", Orientation::Horizontal) => Size::new(240.0, tab_strip_height(node, text, 38.0)),
        ("TabBar", Orientation::Vertical) => Size::new(132.0, 144.0),
        ("TabWidget", Orientation::Horizontal) => {
            Size::new(240.0, 102.0 + tab_strip_height(node, text, 34.0))
        }
        _ => Size::new(240.0, 136.0),
    }
}

/// Reserve the logical trailing strip for a close button.
pub fn tab_content_rect(header: Rect, close: Rect) -> Rect {
    if close.x + close.width / 2.0 < header.x + header.width / 2.0 {
        let start = close.x + close.width + 2.0;
        Rect::new(
            start,
            header.y,
            (header.x + header.width - start).max(0.0),
            header.height,
        )
    } else {
        Rect::new(
            header.x,
            header.y,
            (close.x - header.x - 2.0).max(0.0),
            header.height,
        )
    }
}

pub(crate) fn tab_bar(
    node: &Node,
    bounds: Rect,
    text: &mut impl TextMeasurer,
    direction: LayoutDirection,
    requested: Option<f32>,
    reveal: bool,
) -> (Vec<Rect>, ScrollLayout) {
    let (axis, size_property) = match node.orientation() {
        Orientation::Horizontal => (Axis::Horizontal, "width"),
        Orientation::Vertical => (Axis::Vertical, "height"),
    };
    let direction = if matches!(axis, Axis::Horizontal) {
        direction
    } else {
        LayoutDirection::LeftToRight
    };
    let size = Size::new(bounds.width, bounds.height);
    let extent = axis.extent(size);
    let cross_extent = axis.other().extent(size);
    let enlarged = text.text_scale() > 1.0;
    let spacing = node.length("spacing").unwrap_or(0.0).max(0.0);
    let widget = node.kind == "TabWidget";
    let mut sizes: Vec<_> = node
        .children
        .iter()
        .map(|tab| {
            if !tab.is_visible() {
                return None;
            }
            let explicit = if widget {
                (axis == Axis::Vertical && !enlarged).then_some(36.0)
            } else {
                tab.relative_length(size_property, extent)
            };
            let close_space =
                if axis == Axis::Horizontal && xrack_runtime::tab_is_closable(node, tab) {
                    if enlarged {
                        (cross_extent - 6.0).clamp(0.0, close_target(true)) + 4.0
                    } else {
                        28.0
                    }
                } else {
                    0.0
                };
            let size = explicit.unwrap_or_else(|| {
                if let Some(content) = tab.control_part("ButtonContent") {
                    let padding = 2.0 * ButtonContent::new(tab).padding;
                    let inner_cross = (cross_extent - padding).max(0.0);
                    let (available, lower_limit) = match axis {
                        Axis::Horizontal => (Size::new(f32::INFINITY, inner_cross), 48.0),
                        Axis::Vertical => (Size::new(inner_cross, f32::INFINITY), 36.0),
                    };
                    let content = if content.is_visible() {
                        measure_replacement_content(content, available, text)
                    } else {
                        Size::default()
                    };
                    return (axis.extent(content) + padding).max(lower_limit) + close_space;
                }
                match axis {
                    Axis::Horizontal => {
                        let label = tab
                            .text(if widget { "title" } else { "text" })
                            .unwrap_or("");
                        let font_size = tab_font_size(tab, text, cross_extent);
                        let natural = (text.measure(label, font_size, f32::INFINITY).width + 24.0)
                            .max(48.0)
                            + close_space;
                        if enlarged {
                            natural.min(extent)
                        } else {
                            natural
                        }
                    }
                    Axis::Vertical if enlarged => scaled_tab_height(node, tab, text),
                    Axis::Vertical => 36.0,
                }
            });
            (size > 0.0).then_some((size, explicit.is_none()))
        })
        .collect();
    let count = sizes.iter().flatten().count();
    let gap_count = count.saturating_sub(1);
    if widget && axis == Axis::Horizontal && count != 0 {
        let equal = ((extent - gap_count as f32 * spacing) / count as f32).max(0.0);
        if sizes.iter().flatten().all(|&(minimum, _)| minimum <= equal) {
            for (size, flexible) in sizes.iter_mut().flatten() {
                (*size, *flexible) = (equal, false);
            }
        }
    }
    let minimum =
        sizes.iter().flatten().map(|(size, _)| *size).sum::<f32>() + gap_count as f32 * spacing;
    let flexible = sizes
        .iter()
        .flatten()
        .filter(|(_, flexible)| *flexible)
        .count();
    let extra = (extent - minimum).max(0.0) / flexible.max(1) as f32;
    let max_offset = (minimum - extent).max(0.0);
    // Scroll offsets stay in physical coordinates so track, wheel and pan keep
    // moving the content under the pointer in the same direction.
    let content = axis.rect(bounds, 0.0, 0.0, extent + max_offset, cross_extent);
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
            place_in_direction(rect, content, direction)
        })
        .collect();
    let translate = |mut tab: Rect, offset| {
        match axis {
            Axis::Horizontal => tab.x -= offset,
            Axis::Vertical => tab.y -= offset,
        }
        tab
    };
    let start_offset = if direction == LayoutDirection::RightToLeft {
        max_offset
    } else {
        0.0
    };
    let mut scroll = ScrollLayout::new(requested.unwrap_or(start_offset), max_offset);
    if reveal && let Some(tab) = group_index(node).and_then(|index| rects.get(index)) {
        let offset = tab_reveal_offset(
            bounds,
            translate(*tab, scroll.offset),
            node.orientation(),
            direction,
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
    direction: LayoutDirection,
    offset: f32,
) -> f32 {
    let (start, extent, available) = match orientation {
        Orientation::Horizontal => (tab.x - viewport.x, tab.width, viewport.width),
        Orientation::Vertical => (tab.y - viewport.y, tab.height, viewport.height),
    };
    // For a tab wider than the viewport, reveal its leading portion.
    let leading =
        if orientation == Orientation::Horizontal && direction == LayoutDirection::RightToLeft {
            (extent - available).max(0.0)
        } else {
            0.0
        };
    let start = start + offset + leading;
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
    pub(crate) bar: Rect,
    pub(crate) content: Rect,
}

impl TabGeometry {
    pub(crate) fn new(
        node: &Node,
        bounds: Rect,
        direction: LayoutDirection,
        text: &impl TextMeasurer,
    ) -> Self {
        let (bar, content) = match node.orientation() {
            Orientation::Horizontal => {
                // A short, explicitly sized widget must still leave room for its page.
                let page_minimum = if text.text_scale() > 1.0 {
                    (bounds.height - 34.0).clamp(0.0, 44.0)
                } else {
                    0.0
                };
                let height = (bounds.height - page_minimum).min(tab_strip_height(node, text, 34.0));
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
            bar: place_in_direction(bar, bounds, direction),
            content: place_in_direction(content, bounds, direction),
        }
    }
}

pub(super) struct Tabs {
    pub(super) standalone: bool,
}

impl Item for Tabs {
    fn layout(&self) -> super::ItemLayout {
        if self.standalone {
            super::ItemLayout::TabBar
        } else {
            super::ItemLayout::Pages(super::PageLayout::Tabs)
        }
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> ItemPaint {
        ItemPaint::Tabs {
            standalone: self.standalone,
        }
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> crate::linear::Metrics {
        let size = crate::selection::preferred_tab_view_size(cx.node, cx.layout);
        cx.intrinsic(size)
    }
}
