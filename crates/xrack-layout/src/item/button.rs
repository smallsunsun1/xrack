use super::{
    ArrangeContext, ControlPartPlacement, Item, MeasureContext,
    control_content::ControlContentLayout,
};
use crate::grid::Placement;
use crate::{Rect, Size, dimension};
use xrack_runtime::{ButtonDisplay, ImageAsset, LayoutDirection, Node, Value, button_text};

/// Areas used by the built-in button icon and label.
pub struct ButtonPlacement {
    pub icon: Option<Rect>,
    pub label: Rect,
}

/// Shared intrinsic content for button measurement and painting.
pub struct ButtonContent<'a> {
    pub label: &'a str,
    pub icon: Option<(ImageAsset, f32)>,
    pub padding: f32,
    authored_padding: bool,
    display: ButtonDisplay,
    spacing: f32,
}

impl<'a> ButtonContent<'a> {
    pub fn new(node: &'a Node) -> Self {
        let display = match node.properties.get("display") {
            Some(Value::ButtonDisplay(value)) => *value,
            _ if node.boolean("icon-only", false) => ButtonDisplay::IconOnly,
            _ => ButtonDisplay::default(),
        };
        let size = dimension(node, "icon-size").unwrap_or(18.0);
        let icon = node
            .image_asset("icon")
            .filter(|image| {
                display != ButtonDisplay::TextOnly && size > 0.0 && !image.size().contains(&0)
            })
            .map(|image| (image, size));
        let label = if icon.is_some() && display == ButtonDisplay::IconOnly {
            ""
        } else {
            button_text(node)
        };
        let padding = dimension(node, "padding");
        Self {
            label,
            icon,
            padding: padding.unwrap_or(if node.kind == "ToolButton" { 8.0 } else { 10.0 }),
            authored_padding: padding.is_some(),
            display,
            spacing: dimension(node, "spacing").unwrap_or(6.0).max(0.0),
        }
    }

    fn stacked(&self) -> bool {
        self.display == ButtonDisplay::TextUnderIcon
    }

    fn gap(&self) -> f32 {
        if self.icon.is_some() && !self.label.is_empty() {
            self.spacing
        } else {
            0.0
        }
    }

    pub fn natural_label_width(&self, width: f32) -> f32 {
        if self.stacked() {
            width.max(0.0)
        } else {
            (width - self.icon.as_ref().map_or(0.0, |(_, size)| *size) - self.gap()).max(0.0)
        }
    }

    pub fn natural_size(&self, label: Size) -> Size {
        let icon = self.icon.as_ref().map_or(0.0, |(_, size)| *size);
        if self.stacked() {
            Size::new(label.width.max(icon), label.height + icon + self.gap())
        } else {
            Size::new(label.width + icon + self.gap(), label.height.max(icon))
        }
    }

    fn fitted_icon(&self, area: Rect, line_height: f32) -> f32 {
        let mut height = area.height;
        if self.stacked() && !self.label.is_empty() {
            height -= line_height + self.gap();
        }
        self.icon
            .as_ref()
            .map_or(0.0, |(_, size)| size.min(area.width).min(height).max(0.0))
    }

    /// Fit enlarged text inside a button whose allocated height may be fixed.
    #[doc(hidden)]
    pub fn scaled_label_metrics(&self, bounds: Rect, font_size: f32) -> (Rect, f32) {
        let mut area = bounds.inset(self.padding);
        if self.label.is_empty() {
            return (area, font_size);
        }
        let line = font_size * 1.4;
        let icon = if self.stacked() {
            self.icon
                .as_ref()
                .map_or(0.0, |(_, size)| size.min(area.width))
        } else {
            0.0
        };
        let gap = if icon > 0.0 { self.gap() } else { 0.0 };
        let preferred = line + icon + gap;
        if preferred > area.height && !self.authored_padding {
            let padding =
                ((bounds.height - preferred) * 0.5).clamp(self.padding.min(4.0), self.padding);
            area.y = bounds.y + padding;
            area.height = (bounds.height - 2.0 * padding).max(0.0);
        }
        if preferred <= area.height {
            return (area, font_size);
        }
        // Scale the text and the stacked icon together when they cannot fit at
        // their requested sizes, so both can still appear in a compact button.
        let label_height = (area.height - gap).max(0.0) * (line / (line + icon));
        (area, (label_height / 1.4).max(1.0))
    }

    #[doc(hidden)]
    pub fn label_height(&self, area: Rect, line_height: f32) -> f32 {
        let icon = self.fitted_icon(area, line_height);
        if self.stacked() && icon > 0.0 {
            (area.height - icon - self.gap()).max(0.0)
        } else {
            area.height
        }
    }

    pub fn label_width(&self, area: Rect, line_height: f32) -> f32 {
        let size = self.fitted_icon(area, line_height);
        if self.stacked() || size == 0.0 {
            area.width
        } else {
            (area.width - size - self.gap()).max(0.0)
        }
    }

    pub fn place(
        &self,
        area: Rect,
        label: Size,
        line_height: f32,
        centered: bool,
        direction: LayoutDirection,
    ) -> ButtonPlacement {
        let size = self.fitted_icon(area, line_height);
        let right_to_left = direction == LayoutDirection::RightToLeft;
        let align = |width: f32| {
            if centered {
                area.x + (area.width - width) * 0.5
            } else if right_to_left {
                area.x + area.width - width
            } else {
                area.x
            }
        };
        if size == 0.0 {
            return ButtonPlacement {
                icon: None,
                label: Rect::new(
                    area.x,
                    area.y + (area.height - label.height).max(0.0) * 0.5,
                    area.width,
                    area.height.min(label.height),
                ),
            };
        }

        let gap = self.gap();
        if self.stacked() {
            let label_height = label.height.min((area.height - size - gap).max(0.0));
            let top = area.y + (area.height - size - gap - label_height).max(0.0) * 0.5;
            ButtonPlacement {
                icon: Some(Rect::new(align(size), top, size, size)),
                label: Rect::new(area.x, top + size + gap, area.width, label_height),
            }
        } else {
            let band = size + gap.min((area.width - size).max(0.0));
            let available = (area.width - band).max(0.0);
            let label_width = label.width.min(available);
            let left = align(band + label_width);
            let icon_left = if right_to_left {
                left + label_width + band - size
            } else {
                left
            };
            let label_left = if right_to_left { left } else { left + band };
            ButtonPlacement {
                icon: Some(Rect::new(
                    icon_left,
                    area.y + (area.height - size) * 0.5,
                    size,
                    size,
                )),
                label: Rect::new(
                    label_left,
                    area.y + (area.height - label.height).max(0.0) * 0.5,
                    label_width,
                    area.height.min(label.height),
                ),
            }
        }
    }
}

pub(super) struct Button(pub(super) Size, pub(super) super::ButtonStyle);

impl Item for Button {
    fn baseline(&self, node: &Node, size: Size, text: &mut dyn super::LayoutServices) -> f32 {
        let original = Rect::new(0.0, 0.0, size.width, size.height);
        let content = ButtonContent::new(node);
        let mut area = original.inset(content.padding);
        if let Some(part) = node.control_part("ButtonContent") {
            return area.y
                + part.children.first().map_or(0.0, |child| {
                    super::baseline(child, Size::new(area.width, area.height), text)
                });
        }
        if content.label.is_empty() {
            return 0.0;
        }
        let mut font = text.font_size(node);
        if text.text_scale() > 1.0 {
            (area, font) = content.scaled_label_metrics(original, font);
        }
        let line = font * 1.4;
        let label_width = content.label_width(area, line);
        let measured = text.measure_node_at_size(node, content.label, font, label_width);
        let height = if text.text_scale() > 1.0 {
            measured
                .height
                .min((content.label_height(area, line) / line).floor() * line)
        } else {
            measured.height
        };
        let placed = content.place(
            area,
            Size::new(measured.width, height),
            line,
            true,
            node.layout_direction().unwrap_or_default(),
        );
        placed.label.y + text.font_ascent(node, font)
    }

    fn size_policy(&self, _node: &xrack_runtime::Node) -> super::SizePolicy {
        super::SizePolicy {
            preferred_minimum: true,
            ..super::SizePolicy::INTRINSIC
        }
    }

    fn contains(&self, item: super::ItemRef<'_, '_>, point: crate::Point) -> bool {
        let bounds = item.local_rect();
        if self.1 == super::ButtonStyle::Round {
            crate::RoundedClip {
                bounds,
                radii: crate::corner_radii(item.node, bounds),
            }
            .contains(point)
        } else {
            bounds.contains(point)
        }
    }

    fn focusable(&self, _node: &Node) -> bool {
        true
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> super::ItemPaint {
        super::ItemPaint::Button(self.1)
    }

    fn arrange(&self, cx: &mut ArrangeContext<'_>) -> Option<Vec<Placement>> {
        cx.node.control_part("ButtonContent")?;
        let layout = ControlContentLayout::uniform(ButtonContent::new(cx.node).padding, self.0);
        let content = layout.bounds(cx.bounds, cx.direction);
        Some(cx.control_children(|child| {
            (child.kind == "ButtonContent").then_some(ControlPartPlacement::Plain(content))
        }))
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> crate::linear::Metrics {
        let button = ButtonContent::new(cx.node);
        let layout = ControlContentLayout::uniform(button.padding, self.0);
        let available = layout.available(cx.available);
        let content = if let Some(content) = cx.node.control_part("ButtonContent") {
            cx.layout.measure_children(content, available)
        } else {
            let label = if button.label.is_empty() {
                Size::default()
            } else {
                cx.layout.measure_node(
                    cx.node,
                    button.label,
                    button.natural_label_width(available.width),
                )
            };
            button.natural_size(label)
        };
        cx.intrinsic(layout.preferred(content))
    }
}
