use std::{collections::BTreeMap, time::Instant};
use xrack_layout::{
    Interaction, LayoutNode, Point, PointerSequence, Target, pointer_click_follows,
};
use xrack_runtime::{
    Component, Node, PointerButton, PointerEvent, PointerEventKind, PointerHandle, RowKey,
};

#[derive(Clone, Copy)]
pub struct Contact {
    pub id: u64,
    pub primary: bool,
    pub event: PointerEvent,
}

pub(crate) struct Context<'a> {
    pub(crate) input: &'a mut Interaction,
    pub(crate) component: &'a mut Box<dyn Component>,
    pub(crate) scene: &'a Node,
    pub(crate) layout: &'a LayoutNode,
}

struct Capture {
    contact: Contact,
    // Keep the contact until its terminal event even if its target vanished.
    sequence: Option<PointerSequence>,
}

struct Click {
    handle: PointerHandle,
    row: Option<RowKey>,
    touch: bool,
    point: Point,
    time: Instant,
}

pub(crate) struct Release {
    pub(crate) target: Option<Target>,
}

#[derive(Default)]
pub(crate) struct Captures {
    active: BTreeMap<u64, Capture>,
    last_click: Option<Click>,
    next_touch: i64,
}

impl Captures {
    pub(crate) fn contains(&self, id: u64) -> bool {
        self.active.contains_key(&id)
    }

    pub(crate) fn begin(
        &mut self,
        cx: &mut Context<'_>,
        mut contact: Contact,
        point: Point,
    ) -> bool {
        if self
            .active
            .get(&contact.id)
            .is_some_and(|capture| capture.sequence.is_some())
        {
            return true;
        }
        self.active.remove(&contact.id);
        if contact.event.is_touch {
            let Some(id) = self.next_touch.checked_add(1) else {
                return false;
            };
            contact.event.touch_finger_id = id;
        }
        let Some((mut sequence, delivery)) = cx.input.begin_pointer_sequence(
            cx.scene,
            cx.layout,
            point,
            contact.event,
            Instant::now(),
        ) else {
            return false;
        };
        if contact.event.is_touch {
            if !contact.primary
                && !cx
                    .scene
                    .at_path(&sequence.target().path)
                    .is_some_and(|node| node.boolean("multi-touch", false))
            {
                return false;
            }
            let mut shared = false;
            for capture in self
                .active
                .values_mut()
                .filter(|capture| capture.contact.event.is_touch)
            {
                if let Some(active) = &mut capture.sequence
                    && active.target() == sequence.target()
                {
                    active.disable_press_and_hold();
                    shared = true;
                }
            }
            if shared {
                cx.input.cancel_pointer_activation(sequence.target());
            }
            if shared || !contact.primary {
                sequence.disable_press_and_hold();
            }
            self.next_touch = contact.event.touch_finger_id;
        } else if contact.event.button == PointerButton::Primary {
            cx.input.pointer_press_in_scene(cx.scene, cx.layout);
            if !cx.input.owns_pointer_press(sequence.target()) {
                return false;
            }
        }
        self.active.insert(
            contact.id,
            Capture {
                contact,
                sequence: Some(sequence),
            },
        );
        delivery.apply(cx.component, cx.scene);
        true
    }

    pub(crate) fn advance(&mut self, cx: &mut Context<'_>, contact: Contact, point: Point) -> bool {
        let Some(capture) = self.active.get_mut(&contact.id) else {
            return false;
        };
        capture.contact.event.modifiers = contact.event.modifiers;
        let Some(sequence) = &mut capture.sequence else {
            return false;
        };
        if (!capture.contact.primary || !cx.input.direct_pointer_claimed())
            && let Some(delivery) = sequence.sample(
                cx.input,
                cx.scene,
                cx.layout,
                point,
                PointerEventKind::Move,
                contact.event.modifiers,
            )
        {
            return delivery.apply(cx.component, cx.scene);
        }
        let canceled = cancel_capture(cx, capture);
        if canceled {
            self.last_click = None;
        }
        canceled
    }

    pub(crate) fn release(
        &mut self,
        cx: &mut Context<'_>,
        contact: Contact,
        point: Point,
        canceled: bool,
    ) -> Option<Release> {
        let mut capture = self.active.remove(&contact.id)?;
        capture.contact.event.modifiers = contact.event.modifiers;
        let mut target = None;
        if let Some(mut sequence) = capture.sequence.take() {
            if !canceled
                && (!capture.contact.primary || !cx.input.direct_pointer_claimed())
                && let Some(delivery) = sequence.sample(
                    cx.input,
                    cx.scene,
                    cx.layout,
                    point,
                    PointerEventKind::Up,
                    contact.event.modifiers,
                )
            {
                if capture.contact.primary
                    && capture.contact.event.button == PointerButton::Primary
                    && delivery.event.inside
                    && !sequence.press_and_hold_triggered()
                {
                    target = Some(sequence.target().clone());
                } else if sequence.press_and_hold_triggered() {
                    cx.input.cancel_pointer_activation(sequence.target());
                    self.last_click = None;
                }
                delivery.apply(cx.component, cx.scene);
            } else {
                capture.sequence = Some(sequence);
                cancel_capture(cx, &mut capture);
                self.last_click = None;
            }
        }
        Some(Release { target })
    }

    pub(crate) fn click(
        &mut self,
        cx: &mut Context<'_>,
        target: &Target,
        contact: Contact,
        point: Point,
    ) {
        let Some(handle) = cx
            .scene
            .at_path(&target.path)
            .and_then(|node| node.pointer_handle.clone())
        else {
            return;
        };
        let now = Instant::now();
        let double = self.last_click.take().is_some_and(|previous| {
            previous.handle == handle
                && previous.row == target.row_key
                && previous.touch == contact.event.is_touch
                && pointer_click_follows((previous.point, previous.time), (point, now))
        });
        cx.component.dispatch_at(cx.scene, &target.path, "clicked");
        if double {
            cx.component
                .dispatch_at(cx.scene, &target.path, "double-clicked");
        } else {
            self.last_click = Some(Click {
                handle,
                row: target.row_key,
                touch: contact.event.is_touch,
                point,
                time: now,
            });
        }
    }

    pub(crate) fn cancel(&mut self, cx: &mut Context<'_>, id: u64) {
        if self
            .active
            .get_mut(&id)
            .is_some_and(|capture| cancel_capture(cx, capture))
        {
            self.last_click = None;
        }
    }

    pub(crate) fn cancel_all(&mut self, cx: &mut Context<'_>) -> bool {
        let mut canceled = false;
        for capture in self.active.values_mut() {
            canceled |= cancel_capture(cx, capture);
        }
        if canceled {
            self.last_click = None;
        }
        canceled
    }

    pub(crate) fn clear_click(&mut self) {
        self.last_click = None;
    }

    pub(crate) fn clear(&mut self) {
        self.active.clear();
        self.last_click = None;
    }

    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.active
            .values()
            .filter_map(|capture| capture.sequence.as_ref()?.press_and_hold_deadline())
            .min()
    }

    pub(crate) fn sync(&mut self, cx: &mut Context<'_>, now: Instant) -> bool {
        let mut changed = false;
        for capture in self.active.values_mut() {
            let Some(sequence) = &mut capture.sequence else {
                continue;
            };
            let live = sequence.is_live(cx.input, cx.scene, cx.layout);
            if !live
                || (capture.contact.primary && cx.input.direct_pointer_claimed())
                || (capture.contact.event.is_touch
                    && !capture.contact.primary
                    && !cx
                        .scene
                        .at_path(&sequence.target().path)
                        .is_some_and(|node| node.boolean("multi-touch", false)))
            {
                changed |= cancel_capture(cx, capture);
                self.last_click = None;
                continue;
            }
            sequence.constrain_press_and_hold(cx.layout, None);
            if sequence.advance_press_and_hold(cx.input, cx.scene, cx.layout, cx.component, now) {
                changed = true;
                self.last_click = None;
            }
        }
        changed
    }
}

fn cancel_capture(cx: &mut Context<'_>, capture: &mut Capture) -> bool {
    let Some(sequence) = capture.sequence.take() else {
        return false;
    };
    if capture.contact.primary && capture.contact.event.button == PointerButton::Primary {
        cx.input.cancel_pointer_activation(sequence.target());
    }
    sequence
        .cancel(capture.contact.event.modifiers)
        .apply(cx.component, cx.scene);
    true
}
