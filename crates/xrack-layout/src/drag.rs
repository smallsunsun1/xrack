use crate::{LayoutNode, Point, Target};
use crate::{scroll_gesture::SCROLL_FRAME_INTERVAL, table_drag_selection::edge_delta};
use std::{
    collections::BTreeSet,
    rc::Rc,
    time::{Duration, Instant},
};
use xrack_runtime::{
    Component, DragAction, DragEvent, Length, ListRowRenderer, Node, Orientation, RowKey, Value,
    is_window_overlay,
};

const THRESHOLD: f32 = 6.0;

#[derive(Debug)]
pub struct DragDispatch {
    pub target: Target,
    pub event: DragEvent,
    captured_scene: Option<Rc<Node>>,
}

impl DragDispatch {
    pub fn new(target: Target, event: DragEvent) -> Self {
        Self {
            target,
            event,
            captured_scene: None,
        }
    }

    pub fn apply(self, component: &mut impl Component, scene: &Node) -> bool {
        let scene = self.captured_scene.as_deref().unwrap_or(scene);
        self.target
            .node_in(scene)
            .is_some_and(|node| node.kind == self.event.target_kind())
            && component.drag_event_at(scene, &self.target.path, &self.event)
    }
}

#[derive(Debug, Default)]
pub struct DragUpdate {
    /// A captured gesture suppresses the underlying click and pointer actions.
    pub captured: bool,
    pub events: Vec<DragDispatch>,
}

#[derive(Clone, Debug)]
struct Offer {
    key: String,
    data: String,
    allowed: Vec<DragAction>,
}

impl Offer {
    fn actions(node: &Node) -> impl Iterator<Item = DragAction> + '_ {
        [
            ("allow-move", DragAction::Move),
            ("allow-copy", DragAction::Copy),
            ("allow-link", DragAction::Link),
        ]
        .into_iter()
        .filter_map(|(name, action)| node.boolean(name, false).then_some(action))
    }

    fn from_node(node: &Node) -> Option<Self> {
        let allowed: Vec<_> = Self::actions(node).collect();
        (!allowed.is_empty()).then(|| Self {
            key: node.text("drag-key").unwrap_or("").into(),
            data: node.text("data").unwrap_or("").into(),
            allowed,
        })
    }

    fn action_for(&self, node: &Node, requested: Option<DragAction>) -> Option<DragAction> {
        let keys = node.strings("keys");
        if !keys.is_empty() && !keys.contains(&self.key) {
            return None;
        }
        let action = match node.properties.get("drop-action") {
            Some(Value::DragAction(action))
                if requested.is_none_or(|request| request == *action) =>
            {
                *action
            }
            Some(Value::DragAction(_)) => return None,
            _ => requested.unwrap_or(self.allowed[0]),
        };
        self.allowed.contains(&action).then_some(action)
    }
}

#[derive(Clone, Debug)]
struct Hover {
    target: Target,
    action: DragAction,
    local: Point,
    list_row: Option<ListTarget>,
    captured_scene: Option<Rc<Node>>,
}

#[derive(Clone, Copy, Debug)]
struct ListTarget {
    scope: u64,
    depth: usize,
}

impl Hover {
    fn same_destination(&self, other: &Self) -> bool {
        self.action == other.action
            && match (self.list_row, other.list_row) {
                (Some(left), Some(right)) => {
                    left.scope == right.scope
                        && self.target.row_key == other.target.row_key
                        && self.target.element_id() == other.target.element_id()
                        && self.target.path[left.depth..] == other.target.path[right.depth..]
                }
                (None, None) => self.target == other.target,
                _ => false,
            }
    }
}

#[derive(Clone, Debug)]
struct Active {
    offer: Offer,
    hover: Option<Hover>,
    scroll_wake: Instant,
    rehit_after_scroll: bool,
}

#[derive(Clone, Debug)]
struct VirtualSource {
    viewport: Target,
    renderer: ListRowRenderer,
    row: RowKey,
    index_hint: usize,
    scene: Rc<Node>,
}

impl VirtualSource {
    fn capture(context: &Context<'_>, source: &Target) -> Option<Self> {
        let (depth, viewport, row) = context.list_row(&source.path)?;
        let path = &source.path[..depth - 1];
        if !viewport.boolean("drag-auto-scroll", false) {
            return None;
        }
        let row = row.row_key?;
        let renderer = viewport.list_row_renderer.clone()?;
        let index_hint = viewport.list?.first_row + source.path[depth - 1];
        viewport.displayed_row_for_key(row, index_hint)?;
        Some(Self {
            viewport: Target::from_node(
                path.to_vec(),
                viewport,
                false,
                context.scene.row_key_at(path),
            ),
            renderer,
            row,
            index_hint,
            scene: Rc::new(context.scene.clone()),
        })
    }

    fn is_current(&self, context: &Context<'_>, source: &Target) -> bool {
        let depth = self.viewport.path.len();
        let Some(viewport) = self.viewport.node_in(context.scene) else {
            return false;
        };
        let Some(viewport) = context.node(&self.viewport, viewport.kind) else {
            return false;
        };
        if viewport.list_row_renderer.as_ref() != Some(&self.renderer) {
            return false;
        }
        let Some(index) = viewport.displayed_row_for_key(self.row, self.index_hint) else {
            return false;
        };
        let Some(list) = viewport.list else {
            return false;
        };
        if let Some(slot) = index
            .checked_sub(list.first_row)
            .filter(|&slot| slot < viewport.children.len())
        {
            let mut target = source.clone();
            target.path[depth] = slot;
            return context.node(&target, "DragArea").is_some();
        }

        // Check only the detached source delegate, including live changes to its enabled state.
        let rows = self
            .renderer
            .render_in_scene(viewport, index..index + 1, &Default::default());
        rows.first()
            .filter(|row| row.row_key == Some(self.row))
            .and_then(|row| {
                if source.path.get(depth + 1) != Some(&0) {
                    return None;
                }
                row.interactive_at_path(&source.path[depth + 2..])
            })
            .is_some_and(|node| node.kind == "DragArea" && source.matches_node(node))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct DragSession {
    source: Target,
    origin: Point,
    pub point: Point,
    touch_hold: Option<(Instant, Duration)>,
    virtual_source: Option<VirtualSource>,
    active: Option<Active>,
}

pub(crate) struct Context<'a> {
    pub scene: &'a Node,
    pub layout: &'a LayoutNode,
    pub scope: &'a [usize],
    pub modal: Option<&'a [usize]>,
    pub transitions: &'a BTreeSet<Vec<usize>>,
    pub blocked: bool,
}

impl Context<'_> {
    fn list_row(&self, path: &[usize]) -> Option<(usize, &Node, &Node)> {
        (1..=path.len()).rev().find_map(|depth| {
            let row = self.scene.at_path(&path[..depth])?;
            if row.kind != "ListRow" {
                return None;
            }
            let viewport = self.scene.at_path(&path[..depth - 1])?;
            Some((depth, viewport, row))
        })
    }

    fn node(&self, target: &Target, kind: &str) -> Option<&Node> {
        if self.blocked
            || self
                .modal
                .is_some_and(|modal| !target.path.starts_with(modal))
            || self
                .transitions
                .iter()
                .any(|path| target.path.starts_with(path))
        {
            return None;
        }
        let node = target.node_in(self.scene)?;
        let item = self.layout.at_path(&target.path)?;
        (node.kind == kind && item.visible && item.enabled && node.boolean("enabled", true))
            .then_some(node)
    }

    pub(crate) fn find<T>(
        &self,
        point: Point,
        accept: &impl Fn(&Node, &LayoutNode) -> Option<T>,
    ) -> Option<(Target, T)> {
        fn pick<T>(
            node: &Node,
            layout: &LayoutNode,
            point: Point,
            path: &mut Vec<usize>,
            transitions: &BTreeSet<Vec<usize>>,
            accept: &impl Fn(&Node, &LayoutNode) -> Option<T>,
        ) -> Option<(Vec<usize>, T)> {
            if !layout.enabled
                || transitions.contains(path)
                || !layout.contains_subtree_point(point)
            {
                return None;
            }
            for (index, child, item) in layout.children_in_paint_order(node).rev() {
                if is_window_overlay(child.kind) {
                    continue;
                }
                path.push(index);
                let found = pick(child, item, point, path, transitions, accept);
                path.pop();
                if found.is_some() {
                    return found;
                }
            }
            (node.boolean("enabled", true)
                && layout
                    .map_from_window(point)
                    .is_some_and(|point| layout.bounds.contains(point)))
            .then(|| accept(node, layout))
            .flatten()
            .map(|value| (path.clone(), value))
        }
        if self.blocked {
            return None;
        }
        let (path, result) = pick(
            self.scene.at_path(self.scope)?,
            self.layout.at_path(self.scope)?,
            point,
            &mut self.scope.to_vec(),
            self.transitions,
            accept,
        )?;
        let node = self.scene.at_path(&path)?;
        let row_key = self.scene.row_key_at(&path);
        Some((Target::from_node(path, node, false, row_key), result))
    }

    fn hover(&self, offer: &Offer, point: Point, requested: Option<DragAction>) -> Option<Hover> {
        let (target, (action, local)) = self.find(point, &|node, layout| {
            (node.kind == "DropArea")
                .then(|| {
                    let action = offer.action_for(node, requested)?;
                    let point = layout.map_from_window(point)?;
                    Some((
                        action,
                        Point {
                            x: point.x - layout.bounds.x,
                            y: point.y - layout.bounds.y,
                        },
                    ))
                })
                .flatten()
        })?;
        let list_row = self
            .list_row(&target.path)
            .and_then(|(depth, viewport, row)| {
                row.row_key?;
                Some(ListTarget {
                    scope: viewport.list_row_renderer.as_ref()?.scope(),
                    depth,
                })
            });
        Some(Hover {
            target,
            action,
            local,
            list_row,
            captured_scene: None,
        })
    }
}

pub(crate) fn send(update: &mut DragUpdate, target: &Target, event: DragEvent) {
    update.events.push(DragDispatch::new(target.clone(), event));
}

impl DragSession {
    pub fn arm(context: Context<'_>, point: Point, touch_started: Option<Instant>) -> Option<Self> {
        let (source, delay) = context.find(point, &|node, _| {
            (node.kind == "DragArea" && Offer::actions(node).next().is_some())
                .then(|| node.int("touch-drag-delay").unwrap_or_default().max(0) as u64)
        })?;
        Some(Self {
            source,
            origin: point,
            point,
            touch_hold: touch_started.map(|started| (started, Duration::from_millis(delay))),
            virtual_source: None,
            active: None,
        })
    }

    pub fn active(&self) -> bool {
        self.active.is_some()
    }

    pub fn action(&self) -> Option<DragAction> {
        self.active.as_ref().map(|active| {
            active
                .hover
                .as_ref()
                .map_or(DragAction::None, |over| over.action)
        })
    }

    pub fn over(&self, node: &Node, path: &[usize]) -> bool {
        self.active
            .as_ref()
            .and_then(|active| active.hover.as_ref())
            .is_some_and(|over| over.target.path == path && over.target.matches_node(node))
    }

    pub fn motion(
        &mut self,
        context: Context<'_>,
        point: Point,
        requested: Option<DragAction>,
        now: Instant,
    ) -> (DragUpdate, bool) {
        self.point = point;
        let mut update = DragUpdate::default();
        if self.active.is_none() {
            let Some(source) = context.node(&self.source, "DragArea") else {
                return (update, false);
            };
            if (point.x - self.origin.x).hypot(point.y - self.origin.y) < THRESHOLD {
                return (update, true);
            }
            if self
                .touch_hold
                .is_some_and(|(started, delay)| now.saturating_duration_since(started) < delay)
            {
                return (update, false);
            }
            let Some(offer) = Offer::from_node(source) else {
                return (update, false);
            };
            self.virtual_source = VirtualSource::capture(&context, &self.source);
            self.active = Some(Active {
                offer,
                hover: None,
                scroll_wake: now + SCROLL_FRAME_INTERVAL,
                rehit_after_scroll: false,
            });
            send(&mut update, &self.source, DragEvent::Started);
        } else if !self.source_is_current(&context) {
            return (self.cancel(), false);
        }
        update.captured = true;
        let Some(active) = &mut self.active else {
            return (update, false);
        };
        let next = context.hover(&active.offer, point, requested);
        active.update_hover(next, context.scene, &mut update);
        (update, true)
    }

    pub fn reconcile(
        &mut self,
        context: Context<'_>,
        point: Point,
        requested: Option<DragAction>,
    ) -> (DragUpdate, bool) {
        if !self.source_is_current(&context) {
            return (self.cancel(), false);
        }
        let mut update = DragUpdate::default();
        if let Some(active) = &mut self.active
            && (active.rehit_after_scroll || active.hover.is_some())
        {
            let next = context.hover(&active.offer, point, requested);
            let same_destination = active
                .hover
                .as_ref()
                .zip(next.as_ref())
                .filter(|(old, next)| old.same_destination(next));
            let relocated = same_destination.is_some_and(|(old, next)| old.target != next.target);
            if std::mem::take(&mut active.rehit_after_scroll) || relocated {
                active.update_hover(next, context.scene, &mut update);
            } else if same_destination.is_none() {
                active.leave_hover(&mut update);
            }
        }
        (update, true)
    }

    pub fn finish(
        mut self,
        context: Context<'_>,
        point: Point,
        requested: Option<DragAction>,
    ) -> DragUpdate {
        if !self.active() {
            return DragUpdate::default();
        }
        let (mut update, valid) = self.motion(context, point, requested, Instant::now());
        if !valid {
            return update;
        }
        let Some(active) = self.active.take() else {
            return update;
        };
        let action = active.hover.map_or(DragAction::None, |over| {
            send(
                &mut update,
                &over.target,
                DragEvent::Dropped {
                    data: active.offer.data,
                    action: over.action,
                },
            );
            over.action
        });
        self.send_finished(&mut update, action);
        update
    }

    pub fn cancel(&mut self) -> DragUpdate {
        let Some(mut active) = self.active.take() else {
            return DragUpdate::default();
        };
        let mut update = DragUpdate {
            captured: true,
            events: Vec::new(),
        };
        active.leave_hover(&mut update);
        self.send_finished(&mut update, DragAction::None);
        update
    }

    fn source_is_current(&self, context: &Context<'_>) -> bool {
        if let Some(source) = &self.virtual_source {
            return source.is_current(context, &self.source);
        }
        context.node(&self.source, "DragArea").is_some()
    }

    fn send_finished(&self, update: &mut DragUpdate, action: DragAction) {
        // Cleanup events resolve the captured row even after its display slot has been recycled.
        let captured_scene = self
            .virtual_source
            .as_ref()
            .map(|source| source.scene.clone());
        update.events.push(DragDispatch {
            target: self.source.clone(),
            event: DragEvent::Finished(action),
            captured_scene,
        });
    }

    pub fn auto_scroll_offset(&self, context: Context<'_>) -> Option<(&[usize], f32)> {
        let target = &self.active.as_ref()?.hover.as_ref()?.target;
        if !target.path.starts_with(context.scope) {
            return None;
        }
        context.node(target, "DropArea")?;
        (context.scope.len()..target.path.len())
            .rev()
            .find_map(|depth| {
                let path = &target.path[..depth];
                let node = context.scene.at_path(path)?;
                if !node.boolean("drag-auto-scroll", false) {
                    return None;
                }
                let layout = context.layout.at_path(path)?;
                let point = layout.map_from_window(self.point)?;
                let rect = layout.clip_rect(layout.scroll_viewport(Orientation::Vertical));
                if !rect.contains(point) {
                    return None;
                }
                let scroll = layout.scroll?;
                let offset = (scroll.offset + edge_delta(point.y, rect.y, rect.height))
                    .clamp(0.0, scroll.max_offset);
                (offset != scroll.offset).then_some((path, offset))
            })
    }

    pub fn scroll_deadline(&self) -> Option<Instant> {
        self.active.as_ref().map(|active| active.scroll_wake)
    }

    pub fn advance_scroll_deadline(&mut self, now: Instant, scrolled: bool) {
        if let Some(active) = &mut self.active {
            active.scroll_wake = now + SCROLL_FRAME_INTERVAL;
            active.rehit_after_scroll |= scrolled;
        }
    }
}

impl Active {
    fn leave_hover(&mut self, update: &mut DragUpdate) {
        if let Some(old) = self.hover.take() {
            update.events.push(DragDispatch {
                target: old.target,
                event: DragEvent::Exited,
                captured_scene: old.captured_scene,
            });
        }
    }

    fn update_hover(&mut self, mut next: Option<Hover>, scene: &Node, update: &mut DragUpdate) {
        let same_target = self
            .hover
            .as_ref()
            .zip(next.as_ref())
            .is_some_and(|(last, next)| last.same_destination(next));
        if !same_target {
            self.leave_hover(update);
            if let Some(next) = &next {
                send(
                    update,
                    &next.target,
                    DragEvent::Entered {
                        data: self.offer.data.clone(),
                        action: next.action,
                    },
                );
            }
        }
        if let Some(next) = &mut next {
            if next.target.row_key.is_some() {
                next.captured_scene = self
                    .hover
                    .as_ref()
                    .filter(|old| old.target == next.target)
                    .and_then(|old| old.captured_scene.clone())
                    .or_else(|| Some(Rc::new(scene.clone())));
            }
            if self
                .hover
                .as_ref()
                .is_none_or(|old| old.local != next.local)
            {
                send(
                    update,
                    &next.target,
                    DragEvent::Moved {
                        x: Length(next.local.x.into()),
                        y: Length(next.local.y.into()),
                    },
                );
            }
        }
        self.hover = next;
    }
}
