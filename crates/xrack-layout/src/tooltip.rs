use crate::{LayoutContext, LayoutNode, Point, Rect, Size, TextMeasurer, linear};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use xrack_runtime::{Color, Length, Node, RowKey, is_modal_overlay};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Source {
    #[default]
    Pointer,
    Focus,
}

#[derive(Clone, Copy, Debug)]
enum Anchor {
    Pointer(Point),
    Focus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TooltipKey {
    owner: Vec<usize>,
    slot: usize,
    row: Option<RowKey>,
    source: Source,
}

impl TooltipKey {
    fn new(scene: &Node, path: &[usize], source: Source) -> Option<Self> {
        let slot = scene.at_path(path)?.children.iter().position(has_content)?;
        Some(Self {
            owner: path.to_vec(),
            slot,
            row: scene.row_key_at(path),
            source,
        })
    }

    fn node<'a>(&self, scene: &'a Node) -> Option<&'a Node> {
        if scene.row_key_at(&self.owner) != self.row {
            return None;
        }
        scene
            .at_path(&self.owner)?
            .children
            .get(self.slot)
            .filter(|node| has_content(node))
    }
}

fn has_content(node: &Node) -> bool {
    node.kind == "Tooltip"
        && node.is_visible()
        && (node
            .text("text")
            .is_some_and(|text| !text.trim().is_empty())
            || node.children.iter().any(Node::is_visible))
}

pub(crate) fn hovered(
    scene: &Node,
    layout: &LayoutNode,
    scope: &[usize],
    point: Point,
) -> Option<TooltipKey> {
    fn path_at(node: &Node, item: &LayoutNode, point: Point, path: &mut Vec<usize>) -> bool {
        if !item.visible || !item.bounds.intersect(item.clip).contains(point) {
            return false;
        }
        for (index, (child, layout)) in node.children.iter().zip(&item.children).enumerate().rev() {
            if is_modal_overlay(child.kind) {
                continue;
            }
            path.push(index);
            if path_at(child, layout, point, path) {
                return true;
            }
            path.pop();
        }
        true
    }
    let mut path = scope.to_vec();
    if !path_at(
        scene.at_path(scope)?,
        layout.at_path(scope)?,
        point,
        &mut path,
    ) {
        return None;
    }
    find_owner(scene, &path, scope.len(), Source::Pointer)
}

pub(crate) fn focused(scene: &Node, path: &[usize], scope: &[usize]) -> Option<TooltipKey> {
    if !path.starts_with(scope) {
        return None;
    }
    find_owner(scene, path, scope.len(), Source::Focus)
}

fn find_owner(
    scene: &Node,
    path: &[usize],
    min_depth: usize,
    source: Source,
) -> Option<TooltipKey> {
    (min_depth..=path.len())
        .rev()
        .find_map(|depth| TooltipKey::new(scene, &path[..depth], source))
}

#[derive(Clone, Copy, Debug)]
enum Phase {
    Waiting(Instant),
    Showing { since: Instant, anchor: Anchor },
    Suppressed,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TooltipState {
    pub(crate) source: Source,
    key: Option<TooltipKey>,
    phase: Option<Phase>,
    deadline: Option<Instant>,
    suppress_next: bool,
}

fn after(now: Instant, millis: i64) -> Instant {
    now.checked_add(Duration::from_millis(millis.max(0) as u64))
        .unwrap_or_else(|| now + Duration::from_secs(86_400))
}

impl TooltipState {
    pub(crate) fn focus_source(&mut self) {
        self.source = Source::Focus;
        self.suppress_next = false;
    }
    pub(crate) fn dismiss(&mut self) -> bool {
        let was_visible = self.visible();
        self.phase = Some(Phase::Suppressed);
        self.deadline = None;
        self.suppress_next = self.key.is_none();
        was_visible
    }

    pub(crate) fn pointer_left(&mut self) -> bool {
        if self.source != Source::Pointer {
            return false;
        }
        let visible = self.visible();
        self.key = None;
        self.phase = None;
        self.deadline = None;
        self.suppress_next = false;
        visible
    }

    fn visible(&self) -> bool {
        matches!(self.phase, Some(Phase::Showing { .. }))
    }

    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub(crate) fn advance(
        &mut self,
        scene: &Node,
        key: Option<TooltipKey>,
        pointer: Option<Point>,
        blocked: bool,
        now: Instant,
    ) -> bool {
        let was_visible = self.visible();
        let changed = self.key != key;
        self.deadline = None;
        let suppress = std::mem::take(&mut self.suppress_next) || blocked;
        if changed {
            self.key = key;
            self.phase = Some(Phase::Waiting(now));
        }
        let Some(key) = &self.key else {
            self.phase = None;
            return was_visible;
        };
        let Some(node) = key.node(scene) else {
            self.phase = None;
            return was_visible;
        };
        if suppress {
            self.phase = Some(Phase::Suppressed);
        }
        if let Some(Phase::Waiting(since)) = self.phase {
            let show = after(since, node.int("delay").unwrap_or(600));
            if now < show {
                self.deadline = Some(show);
            } else {
                let anchor = match key.source {
                    Source::Focus => Anchor::Focus,
                    Source::Pointer => pointer.map_or(Anchor::Focus, Anchor::Pointer),
                };
                self.phase = Some(Phase::Showing { since: now, anchor });
            }
        }
        if let Some(Phase::Showing { since, .. }) = self.phase
            && let Some(timeout) = node.int("timeout").filter(|timeout| *timeout >= 0)
        {
            let hide = after(since, timeout);
            if now >= hide {
                self.phase = Some(Phase::Suppressed);
            } else {
                self.deadline = Some(hide);
            }
        }
        self.visible() != was_visible || changed && was_visible
    }

    pub(crate) fn popup<'a>(
        &self,
        scene: &'a Node,
        layout: &LayoutNode,
        scope: &[usize],
    ) -> Option<TooltipPopup<'a>> {
        let Some(Phase::Showing { anchor, .. }) = self.phase else {
            return None;
        };
        let key = self.key.as_ref()?;
        if !key.owner.starts_with(scope) {
            return None;
        }
        let node = key.node(scene)?;
        let owner = layout.at_path(&key.owner)?;
        let bounds = owner.bounds.intersect(owner.clip);
        if !owner.visible || bounds.width <= 0.0 || bounds.height <= 0.0 {
            return None;
        }
        Some(TooltipPopup {
            node,
            anchor,
            owner: bounds,
        })
    }
}

pub struct TooltipPopup<'a> {
    pub node: &'a Node,
    anchor: Anchor,
    owner: Rect,
}

/// Place a tooltip surface in window coordinates, using the normal text and layout engine.
pub fn layout_tooltip(
    popup: TooltipPopup<'_>,
    viewport: Rect,
    text: &mut impl TextMeasurer,
) -> (Node, LayoutNode) {
    let TooltipPopup {
        node,
        anchor,
        owner,
    } = popup;
    let area = viewport.inset(6.0);
    let mut surface = Node::new("VerticalLayout", None);
    surface.properties.insert(
        "background",
        node.properties
            .get("background")
            .cloned()
            .unwrap_or_else(|| Color(0x25334bff).into()),
    );
    surface.properties.insert(
        "padding",
        Length(f64::from(
            node.length("padding").unwrap_or(8.0).clamp(0.0, 32.0),
        ))
        .into(),
    );
    surface
        .properties
        .insert("border-radius", Length(5.0).into());
    surface
        .properties
        .insert("border-width", Length(1.0).into());
    surface
        .properties
        .insert("border-color", Color(0x627596ff).into());
    if let Some(value) = node.text("text") {
        let mut label = Node::new("Text", None);
        label.properties.insert("text", value.to_owned().into());
        label.properties.insert(
            "font-size",
            Length(f64::from(node.length("font-size").unwrap_or(14.0))).into(),
        );
        if let Some(color) = node.color("color") {
            label.properties.insert("color", color.into());
        }
        surface.children.push(label);
    } else {
        surface.children.clone_from(&node.children);
    }

    let cap = node
        .relative_length("max-width", area.width)
        .unwrap_or(320.0)
        .clamp(0.0, area.width);
    let width = node
        .relative_length("width", area.width)
        .unwrap_or_else(|| {
            linear::measure(&surface, Size::new(cap, area.height), text)
                .x
                .preferred
        })
        .clamp(0.0, cap);
    let height = node
        .relative_length("height", area.height)
        .unwrap_or_else(|| {
            linear::measure(&surface, Size::new(width, area.height), text)
                .y
                .preferred
        })
        .clamp(0.0, area.height);
    let (x, below, above) = match anchor {
        Anchor::Pointer(point) => (point.x + 12.0, point.y + 18.0, point.y - 8.0),
        Anchor::Focus => (
            owner.x + (owner.width - width) * 0.5,
            owner.y + owner.height + 8.0,
            owner.y - 8.0,
        ),
    };
    let bottom = area.y + area.height;
    let y = if below + height <= bottom || bottom - below >= above - area.y {
        below
    } else {
        above - height
    };
    let bounds = Rect::new(
        x.clamp(area.x, (area.x + area.width - width).max(area.x)),
        y.clamp(area.y, (bottom - height).max(area.y)),
        width,
        height,
    );
    let mut context = LayoutContext {
        text,
        offsets: &BTreeMap::new(),
        horizontal_offsets: None,
        path: Vec::new(),
        viewport,
    };
    let layout = context.place(&surface, bounds, area, true, true, None);
    (surface, layout)
}
