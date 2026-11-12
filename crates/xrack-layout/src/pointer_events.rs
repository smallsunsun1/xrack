use crate::{Interaction, LayoutNode, Point, Target};
use std::time::{Duration, Instant};
use xrack_runtime::{
    Component, KeyModifiers, Length, Node, PointerButton, PointerEvent, PointerEventKind,
    PointerHandle,
};

pub fn pointer_click_follows(previous: (Point, Instant), current: (Point, Instant)) -> bool {
    current
        .1
        .checked_duration_since(previous.1)
        .is_some_and(|elapsed| elapsed <= Duration::from_millis(450))
        && (current.0.x - previous.0.x).hypot(current.0.y - previous.0.y) <= 6.0
}

enum PressAndHold {
    Pending {
        origin: Point,
        point: Point,
        deadline: Instant,
    },
    Triggered,
}

/// One physical contact, tied to the handler and model row that received Down.
pub struct PointerSequence {
    target: Target,
    handle: PointerHandle,
    last: PointerEvent,
    press_and_hold: Option<PressAndHold>,
}

pub struct PointerDelivery {
    target: Target,
    handle: PointerHandle,
    pub event: PointerEvent,
}

impl PointerDelivery {
    pub fn apply(self, component: &mut impl Component, scene: &Node) -> bool {
        let live = live_handler(scene, &self.target, &self.handle);
        if live || self.event.kind == PointerEventKind::Cancel {
            self.handle.publish_event(&self.event);
        }
        if live {
            component.pointer_event_at(scene, &self.target.path, &self.event);
            if self.event.kind == PointerEventKind::Move {
                component.dispatch_at(scene, &self.target.path, "moved");
            }
        }
        live
    }
}

pub(crate) fn local_position(item: &LayoutNode, point: Point) -> Option<(Point, Length, Length)> {
    let local = item.map_from_window(point)?;
    (local.x.is_finite() && local.y.is_finite()).then(|| {
        (
            local,
            Length(f64::from(local.x - item.bounds.x)),
            Length(f64::from(local.y - item.bounds.y)),
        )
    })
}

fn live_handler(scene: &Node, target: &Target, handle: &PointerHandle) -> bool {
    target
        .node_in(scene)
        .is_some_and(|node| node.pointer_handle.as_ref() == Some(handle))
}

impl PointerSequence {
    pub(crate) fn begin(
        target: Target,
        input: &Interaction,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        event: PointerEvent,
        now: Instant,
    ) -> Option<(Self, PointerDelivery)> {
        let node = scene.interactive_at_path(&target.path)?;
        if node.kind != "TouchArea" {
            return None;
        }
        let mut sequence = Self {
            handle: node.pointer_handle.clone()?,
            target,
            last: event,
            press_and_hold: None,
        };
        let delivery = sequence.sample(
            input,
            scene,
            layout,
            point,
            PointerEventKind::Down,
            event.modifiers,
        )?;
        if node.press_and_hold_handler && event.button == PointerButton::Primary {
            let interval = node.int("press-and-hold-interval").unwrap_or(800);
            if interval > 0
                && let Some(deadline) = now.checked_add(Duration::from_millis(interval as u64))
            {
                sequence.press_and_hold = Some(PressAndHold::Pending {
                    origin: point,
                    point,
                    deadline,
                });
            }
        }
        Some((sequence, delivery))
    }

    pub fn target(&self) -> &Target {
        &self.target
    }

    pub fn is_live(&self, input: &Interaction, scene: &Node, layout: &LayoutNode) -> bool {
        let path = &self.target.path;
        live_handler(scene, &self.target, &self.handle)
            && scene
                .interactive_at_path(path)
                .is_some_and(|node| node.accepts_pointer_button(self.last.button))
            && layout
                .at_path(path)
                .is_some_and(|item| item.visible && item.enabled)
            && input.allows_popup(layout, path)
            && !input.menu_open()
            && !input.choice_open()
    }

    pub fn press_and_hold_deadline(&self) -> Option<Instant> {
        match self.press_and_hold {
            Some(PressAndHold::Pending { deadline, .. }) => Some(deadline),
            _ => None,
        }
    }

    pub fn disable_press_and_hold(&mut self) {
        if self.press_and_hold_deadline().is_some() {
            self.press_and_hold = None;
        }
    }

    pub fn press_and_hold_triggered(&self) -> bool {
        matches!(self.press_and_hold, Some(PressAndHold::Triggered))
    }

    fn update_hold_position(&mut self, point: Point, inside: bool) {
        if let Some(PressAndHold::Pending {
            origin,
            point: previous,
            ..
        }) = &mut self.press_and_hold
        {
            if inside && (point.x - origin.x).hypot(point.y - origin.y) <= 6.0 {
                *previous = point;
            } else {
                self.press_and_hold = None;
            }
        }
    }

    pub fn constrain_press_and_hold(&mut self, layout: &LayoutNode, position: Option<Point>) {
        if let Some(PressAndHold::Pending { point, .. }) = self.press_and_hold {
            let point = position.unwrap_or(point);
            let inside = layout.at_path(&self.target.path).is_some_and(|item| {
                item.contains_clip_point(point)
                    && item
                        .map_from_window(point)
                        .is_some_and(|local| item.bounds.contains(local))
            });
            self.update_hold_position(point, inside);
        }
    }

    pub fn advance_press_and_hold(
        &mut self,
        input: &Interaction,
        scene: &Node,
        layout: &LayoutNode,
        component: &mut impl Component,
        now: Instant,
    ) -> bool {
        if self
            .press_and_hold_deadline()
            .is_none_or(|deadline| now < deadline)
        {
            return false;
        }
        self.constrain_press_and_hold(layout, None);
        let pending = self.press_and_hold.take().is_some();
        if pending
            && !input.direct_pointer_claimed()
            && input.drag_position().is_none()
            && self.is_live(input, scene, layout)
            && component.dispatch_at(scene, &self.target.path, "press-and-hold")
        {
            self.press_and_hold = Some(PressAndHold::Triggered);
            true
        } else {
            false
        }
    }

    pub fn sample(
        &mut self,
        input: &Interaction,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        kind: PointerEventKind,
        modifiers: KeyModifiers,
    ) -> Option<PointerDelivery> {
        if !self.is_live(input, scene, layout) {
            return None;
        }
        let item = layout.at_path(&self.target.path)?;
        let (local, x, y) = local_position(item, point)?;
        self.last = PointerEvent {
            kind,
            x,
            y,
            inside: item.bounds.contains(local) && item.contains_clip_point(point),
            modifiers,
            ..self.last
        };
        if matches!(kind, PointerEventKind::Up | PointerEventKind::Cancel) {
            if self.press_and_hold_deadline().is_some() {
                self.press_and_hold = None;
            }
        } else {
            self.update_hold_position(point, self.last.inside);
        }
        Some(PointerDelivery {
            target: self.target.clone(),
            handle: self.handle.clone(),
            event: self.last,
        })
    }

    pub fn cancel(self, modifiers: KeyModifiers) -> PointerDelivery {
        PointerDelivery {
            target: self.target,
            handle: self.handle,
            event: PointerEvent {
                kind: PointerEventKind::Cancel,
                inside: false,
                modifiers,
                ..self.last
            },
        }
    }
}
