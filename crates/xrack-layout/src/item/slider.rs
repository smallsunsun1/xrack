use super::{ArrangeContext, Item, ItemAction, ItemEvent, ItemPaint, ItemRef, MeasureContext};
use crate::{DialGeometry, NavigationKey, Rect, Size, SliderGeometry, grid::Placement};
use xrack_runtime::{LayoutDirection, Node, Orientation, RangeAction, SliderInterval, SliderRange};

pub(super) struct Slider {
    pub interval: bool,
}

impl Item for Slider {
    fn size_policy(&self, _node: &xrack_runtime::Node) -> super::SizePolicy {
        super::SizePolicy::INTRINSIC_HEIGHT
    }

    fn reserves_pointer_drag(&self, _node: &xrack_runtime::Node) -> bool {
        true
    }

    fn hit_behavior(&self, _node: &Node) -> super::HitBehavior {
        if self.interval {
            super::HitBehavior::ProxyTarget
        } else {
            super::HitBehavior::Normal
        }
    }

    fn focusable(&self, _node: &Node) -> bool {
        !self.interval
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> crate::linear::Metrics {
        let size = match cx.node.orientation() {
            Orientation::Horizontal => Size::new(160.0, 28.0),
            Orientation::Vertical => Size::new(28.0, 160.0),
        };
        cx.intrinsic(size)
    }

    fn arrange(&self, cx: &mut ArrangeContext<'_>) -> Option<Vec<Placement>> {
        if self.interval {
            return Some(arrange_interval(cx));
        }
        cx.node.control_part("SliderHandle")?;
        let geometry = SliderGeometry::with_layout_direction(cx.bounds, cx.node, cx.direction);
        let range = SliderRange::from_node(cx.node);
        let preview = cx
            .input
            .filter(|_| !cx.node.boolean("live", true))
            .and_then(|input| input.slider_preview(cx.path));
        let fraction =
            preview.map_or_else(|| range.fraction(), |value| range.preview_fraction(value));
        Some(
            cx.node
                .children
                .iter()
                .map(|child| {
                    Placement::plain(if child.kind == "SliderHandle" {
                        geometry.thumb_bounds(0, fraction)
                    } else {
                        cx.layout.absolute_child(child, cx.bounds)
                    })
                })
                .collect(),
        )
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> ItemPaint {
        if self.interval {
            ItemPaint::RangeSlider
        } else {
            ItemPaint::Slider
        }
    }

    fn event(&self, item: ItemRef<'_, '_>, event: ItemEvent) -> Option<ItemAction> {
        let ItemEvent::Navigate(key) = event else {
            return None;
        };
        let horizontal = item.node.orientation() == Orientation::Horizontal;
        let mirrored = item.layout.layout_direction == LayoutDirection::RightToLeft;
        range_key(key, horizontal, !horizontal, mirrored)
    }
}

pub(super) struct Dial;

impl Item for Dial {
    fn size_policy(&self, _node: &xrack_runtime::Node) -> super::SizePolicy {
        super::SizePolicy::INTRINSIC_HEIGHT
    }

    fn reserves_pointer_drag(&self, _node: &xrack_runtime::Node) -> bool {
        true
    }

    fn focusable(&self, _node: &Node) -> bool {
        true
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> crate::linear::Metrics {
        cx.intrinsic(Size::new(96.0, 96.0))
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> ItemPaint {
        ItemPaint::Dial
    }

    fn event(&self, item: ItemRef<'_, '_>, event: ItemEvent) -> Option<ItemAction> {
        match event {
            ItemEvent::Navigate(key) => range_key(key, true, true, false),
            ItemEvent::HitTest(point) => (!DialGeometry::new(item.local_rect(), item.node)
                .contains(point))
            .then_some(ItemAction::SkipHit),
            _ => None,
        }
    }
}

pub(super) struct ColorPlane;

impl Item for ColorPlane {
    fn reserves_pointer_drag(&self, _node: &xrack_runtime::Node) -> bool {
        true
    }

    fn focusable(&self, _node: &Node) -> bool {
        true
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> ItemPaint {
        ItemPaint::ColorPlane
    }

    fn event(&self, item: ItemRef<'_, '_>, event: ItemEvent) -> Option<ItemAction> {
        let ItemEvent::Navigate(key) = event else {
            return None;
        };
        let saturation = item.node.number("saturation").unwrap_or_default();
        let brightness = item.node.number("brightness").unwrap_or_default();
        let step = 0.01;
        let (x, y) = match key {
            NavigationKey::Left => (saturation - step, brightness),
            NavigationKey::Right => (saturation + step, brightness),
            NavigationKey::Up => (saturation, brightness + step),
            NavigationKey::Down => (saturation, brightness - step),
            _ => return None,
        };
        Some(ItemAction::Range(RangeAction::SetPoint {
            x: x.clamp(0.0, 1.0),
            y: y.clamp(0.0, 1.0),
        }))
    }
}

fn range_key(
    key: NavigationKey,
    horizontal: bool,
    vertical: bool,
    mirrored: bool,
) -> Option<ItemAction> {
    let action = match key {
        NavigationKey::Left if horizontal && mirrored => RangeAction::Increase,
        NavigationKey::Right if horizontal && mirrored => RangeAction::Decrease,
        NavigationKey::Left if horizontal => RangeAction::Decrease,
        NavigationKey::Right if horizontal => RangeAction::Increase,
        NavigationKey::Down if vertical => RangeAction::Decrease,
        NavigationKey::Up if vertical => RangeAction::Increase,
        NavigationKey::Home => RangeAction::Minimum,
        NavigationKey::End => RangeAction::Maximum,
        _ => return None,
    };
    Some(ItemAction::Range(action))
}

fn arrange_interval(cx: &ArrangeContext<'_>) -> Vec<Placement> {
    let geometry = SliderGeometry::with_layout_direction(cx.bounds, cx.node, cx.direction);
    let interval = SliderInterval::from_node(cx.node);
    let preview = cx
        .input
        .filter(|_| cx.enabled)
        .and_then(|input| input.interval_preview(cx.node, cx.path));
    let fractions = preview.map_or_else(
        || interval.fractions(),
        |(index, fraction)| interval.preview_fractions(index, fraction),
    );
    cx.node
        .children
        .iter()
        .enumerate()
        .map(|(slot, child)| {
            let index = if slot < Node::RANGE_THUMBS {
                Some(slot)
            } else {
                Node::RANGE_HANDLE_PARTS
                    .iter()
                    .position(|kind| *kind == child.kind)
            };
            Placement::plain(index.map_or(Rect::default(), |index| {
                geometry.thumb_bounds(index, fractions[index])
            }))
        })
        .collect()
}
