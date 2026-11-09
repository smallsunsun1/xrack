use crate::{LayoutNode, Point, Target};
use std::collections::BTreeSet;
use xrack_runtime::{Component, DragAction, DragEvent, Length, Node, Value, is_window_overlay};

const THRESHOLD: f32 = 6.0;

#[derive(Debug)]
pub struct DragDispatch {
    pub target: Target,
    pub event: DragEvent,
}

impl DragDispatch {
    pub fn apply(self, component: &mut impl Component, scene: &Node) -> bool {
        let path = &self.target.path;
        scene.row_key_at(path) == self.target.row_key
            && scene.at_path(path).is_some_and(|node| {
                node.id == self.target.id && node.kind == self.event.target_kind()
            })
            && component.drag_event_at(scene, path, &self.event)
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
}

#[derive(Clone, Debug)]
struct Active {
    offer: Offer,
    hover: Option<Hover>,
}

#[derive(Clone, Debug)]
pub(crate) struct DragSession {
    source: Target,
    origin: Point,
    pub point: Point,
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
    fn node(&self, target: &Target, kind: &str) -> Option<&Node> {
        if self.blocked
            || self
                .modal
                .is_some_and(|modal| !target.path.starts_with(modal))
            || self
                .transitions
                .iter()
                .any(|path| target.path.starts_with(path))
            || self.scene.row_key_at(&target.path) != target.row_key
        {
            return None;
        }
        let node = self.scene.at_path(&target.path)?;
        let item = self.layout.at_path(&target.path)?;
        (node.kind == kind
            && node.id == target.id
            && item.visible
            && item.enabled
            && node.boolean("enabled", true))
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
            if !layout.visible
                || !layout.enabled
                || transitions.contains(path)
                || !layout.clip.intersect(layout.bounds).contains(point)
            {
                return None;
            }
            for (index, (child, item)) in
                node.children.iter().zip(&layout.children).enumerate().rev()
            {
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
            node.boolean("enabled", true)
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
        Some((
            Target {
                id: self.scene.at_path(&path)?.id,
                row_key: self.scene.row_key_at(&path),
                path,
                focusable: false,
            },
            result,
        ))
    }

    fn hover(&self, offer: &Offer, point: Point, requested: Option<DragAction>) -> Option<Hover> {
        let (target, (action, local)) = self.find(point, &|node, layout| {
            (node.kind == "DropArea")
                .then(|| {
                    let action = offer.action_for(node, requested)?;
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
        Some(Hover {
            target,
            action,
            local,
        })
    }
}

pub(crate) fn send(update: &mut DragUpdate, target: &Target, event: DragEvent) {
    update.events.push(DragDispatch {
        target: target.clone(),
        event,
    });
}

impl DragSession {
    pub fn arm(context: Context<'_>, point: Point) -> Option<Self> {
        let (source, ()) = context.find(point, &|node, _| {
            (node.kind == "DragArea" && Offer::actions(node).next().is_some()).then_some(())
        })?;
        Some(Self {
            source,
            origin: point,
            point,
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
            .is_some_and(|over| over.target.path == path && over.target.id == node.id)
    }

    pub fn motion(
        &mut self,
        context: Context<'_>,
        point: Point,
        requested: Option<DragAction>,
    ) -> (DragUpdate, bool) {
        self.point = point;
        let Some(source) = context.node(&self.source, "DragArea") else {
            return (self.cancel(), false);
        };
        let mut update = DragUpdate::default();
        if self.active.is_none() {
            if (point.x - self.origin.x).hypot(point.y - self.origin.y) < THRESHOLD {
                return (update, true);
            }
            let Some(offer) = Offer::from_node(source) else {
                return (update, false);
            };
            self.active = Some(Active { offer, hover: None });
            send(&mut update, &self.source, DragEvent::Started);
        }
        update.captured = true;
        let active = self.active.as_mut().unwrap();
        let next = context.hover(&active.offer, point, requested);
        active.update_hover(next, &mut update);
        (update, true)
    }

    pub fn reconcile(
        &mut self,
        context: Context<'_>,
        point: Point,
        requested: Option<DragAction>,
    ) -> (DragUpdate, bool) {
        if context.node(&self.source, "DragArea").is_none() {
            return (self.cancel(), false);
        }
        let mut update = DragUpdate::default();
        if let Some(active) = &mut self.active
            && active.hover.as_ref().is_some_and(|over| {
                !context
                    .hover(&active.offer, point, requested)
                    .is_some_and(|next| next.target == over.target && next.action == over.action)
            })
        {
            active.update_hover(None, &mut update);
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
        let (mut update, valid) = self.motion(context, point, requested);
        if !valid {
            return update;
        }
        let active = self.active.take().unwrap();
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
        send(&mut update, &self.source, DragEvent::Finished(action));
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
        active.update_hover(None, &mut update);
        send(
            &mut update,
            &self.source,
            DragEvent::Finished(DragAction::None),
        );
        update
    }
}

impl Active {
    fn update_hover(&mut self, next: Option<Hover>, update: &mut DragUpdate) {
        let same_target = self
            .hover
            .as_ref()
            .zip(next.as_ref())
            .is_some_and(|(last, next)| last.target == next.target && last.action == next.action);
        if !same_target {
            if let Some(last) = &self.hover {
                send(update, &last.target, DragEvent::Exited);
            }
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
        if let Some(next) = &next
            && (!same_target
                || self
                    .hover
                    .as_ref()
                    .is_none_or(|last| last.local != next.local))
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
        self.hover = next;
    }
}
