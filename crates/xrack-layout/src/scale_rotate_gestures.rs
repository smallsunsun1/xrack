use crate::{
    Interaction, LayoutNode, Point,
    scale_rotate::{ScaleRotateTarget, TouchTransform, touch_centroid},
};
use xrack_runtime::{Component, Node, ScaleRotateSample};

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ContactId<Device>(pub Device, pub u64);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TouchPhase {
    Started,
    Moved,
    Ended,
    Cancelled,
}

pub struct Context<'a> {
    pub scene: &'a Node,
    pub layout: &'a LayoutNode,
    pub input: &'a Interaction,
}

pub struct Delivery {
    target: ScaleRotateTarget,
    pub event: &'static str,
    sample: Option<ScaleRotateSample>,
}

impl Delivery {
    pub fn matches(&self, scene: &Node) -> bool {
        scene
            .at_path(&self.target.path)
            .and_then(|node| node.scale_rotate_handle.as_ref())
            == Some(&self.target.handle)
    }

    pub fn apply(self, component: &mut impl Component, scene: &Node) -> bool {
        self.target.handle.publish(self.sample);
        self.matches(scene) && component.dispatch_at(scene, &self.target.path, self.event)
    }
}

#[derive(Clone, Copy)]
struct Contact<Device> {
    id: ContactId<Device>,
    point: Point,
}

enum Source<Device> {
    Touch {
        pair: [Contact<Device>; 2],
        transform: TouchTransform,
    },
    Trackpad {
        device: Device,
        channels: u8,
        center: Point,
    },
}

struct Capture<Device> {
    target: ScaleRotateTarget,
    source: Source<Device>,
    sample: ScaleRotateSample,
}

impl<Device> Capture<Device> {
    fn delivery(&self, event: &'static str) -> Delivery {
        Delivery {
            target: self.target.clone(),
            event,
            sample: matches!(event, "started" | "updated").then_some(self.sample),
        }
    }

    fn update(&mut self, sample: ScaleRotateSample) -> Option<Delivery> {
        if self.sample == sample {
            return None;
        }
        self.sample = sample;
        Some(self.delivery("updated"))
    }
}

#[derive(Clone, Copy)]
pub enum TrackpadDelta {
    Scale(f64),
    Rotation(f64),
}

pub struct Gestures<Device> {
    pending: Option<(Contact<Device>, ScaleRotateTarget)>,
    capture: Option<Capture<Device>>,
    /// Contacts consumed by a two-finger capture stay consumed until they lift.
    held: Vec<ContactId<Device>>,
}

impl<Device> Default for Gestures<Device> {
    fn default() -> Self {
        Self {
            pending: None,
            capture: None,
            held: Vec::new(),
        }
    }
}

impl<Device: Copy + Eq> Gestures<Device> {
    pub fn is_active(&self) -> bool {
        self.capture.is_some()
    }

    pub fn captured_touch_pair(&self) -> Option<[ContactId<Device>; 2]> {
        match &self.capture.as_ref()?.source {
            Source::Touch { pair, .. } => Some(pair.map(|contact| contact.id)),
            Source::Trackpad { .. } => None,
        }
    }

    pub fn cancel(&mut self) -> Option<Delivery> {
        self.pending = None;
        self.capture
            .take()
            .map(|capture| capture.delivery("cancelled"))
    }

    pub fn reset(&mut self) -> Option<Delivery> {
        self.held.clear();
        self.cancel()
    }

    pub fn reconcile(&mut self, context: &Context<'_>) -> Option<Delivery> {
        if self.capture.as_ref().is_some_and(|capture| {
            !capture
                .target
                .is_live(context.scene, context.layout, context.input)
        }) {
            self.cancel()
        } else {
            None
        }
    }

    pub fn touch(
        &mut self,
        context: &Context<'_>,
        id: ContactId<Device>,
        phase: TouchPhase,
        point: Point,
    ) -> (bool, [Option<Delivery>; 2]) {
        let terminal = matches!(phase, TouchPhase::Ended | TouchPhase::Cancelled);
        if let Some(cancelled) = self.reconcile(context) {
            let consumed = self.held.contains(&id) || phase == TouchPhase::Started;
            if terminal {
                self.held.retain(|held| *held != id);
            } else if phase == TouchPhase::Started && !self.held.contains(&id) {
                self.held.push(id);
            }
            return (consumed, [Some(cancelled), None]);
        }
        if let Some(capture) = &mut self.capture {
            if phase != TouchPhase::Started && !self.held.contains(&id) {
                return (false, [None, None]);
            }
            if phase == TouchPhase::Started && !self.held.contains(&id) {
                self.held.push(id);
            }
            if terminal {
                self.held.retain(|held| *held != id);
            }
            let Source::Touch { pair, transform } = &mut capture.source else {
                return (true, [None, None]);
            };
            let Some(contact) = pair.iter_mut().find(|contact| contact.id == id) else {
                return (true, [None, None]);
            };
            if phase == TouchPhase::Cancelled {
                return (true, [self.cancel(), None]);
            }
            if phase == TouchPhase::Started {
                return (true, [None, None]);
            }
            contact.point = point;
            let points = pair.map(|contact| contact.point);
            let delivery = transform.update(points).and_then(|(scale, rotation)| {
                let sample = capture.target.sample(
                    context.layout,
                    touch_centroid(points),
                    scale,
                    rotation,
                )?;
                capture.update(sample)
            });
            let ended = if phase == TouchPhase::Ended {
                self.capture.take().map(|capture| capture.delivery("ended"))
            } else {
                None
            };
            return (true, [delivery, ended]);
        }
        if let Some(index) = self.held.iter().position(|held| *held == id) {
            if terminal {
                self.held.swap_remove(index);
            }
            return (true, [None, None]);
        }
        if !self.held.is_empty() && phase == TouchPhase::Started {
            self.held.push(id);
            return (true, [None, None]);
        }
        if phase == TouchPhase::Started {
            let Some(target) =
                context
                    .input
                    .scale_rotate_target_at(context.scene, context.layout, point)
            else {
                return (false, [None, None]);
            };
            let second = Contact { id, point };
            let Some((first, initial)) = &self.pending else {
                self.pending = Some((second, target));
                return (false, [None, None]);
            };
            if first.id == id
                || first.id.0 != id.0
                || *initial != target
                || context
                    .input
                    .scale_rotate_target_at(context.scene, context.layout, first.point)
                    .as_ref()
                    != Some(&target)
            {
                return (false, [None, None]);
            }
            let points = [first.point, point];
            let Some(transform) = TouchTransform::new(points) else {
                return (false, [None, None]);
            };
            let Some(sample) = target.sample(context.layout, touch_centroid(points), 1.0, 0.0)
            else {
                return (false, [None, None]);
            };
            let pair = [*first, second];
            self.held.extend(pair.map(|contact| contact.id));
            let capture = Capture {
                target,
                source: Source::Touch { pair, transform },
                sample,
            };
            let started = capture.delivery("started");
            self.capture = Some(capture);
            self.pending = None;
            return (true, [Some(started), None]);
        }
        if let Some((first, _)) = &mut self.pending
            && first.id == id
        {
            if terminal {
                self.pending = None;
            } else {
                first.point = point;
            }
        }
        (false, [None, None])
    }

    pub fn trackpad(
        &mut self,
        context: &Context<'_>,
        device: Device,
        phase: TouchPhase,
        delta: TrackpadDelta,
    ) -> [Option<Delivery>; 2] {
        if let Some(cancelled) = self.reconcile(context) {
            return [Some(cancelled), None];
        }
        let channel = match delta {
            TrackpadDelta::Scale(_) => 1,
            TrackpadDelta::Rotation(_) => 2,
        };
        let mut started = None;
        if self.capture.is_none()
            && phase == TouchPhase::Started
            && self.held.is_empty()
            && self.pending.is_none()
        {
            let Some(center) = context.input.pointer_position() else {
                return [None, None];
            };
            let Some(target) =
                context
                    .input
                    .scale_rotate_target_at(context.scene, context.layout, center)
            else {
                return [None, None];
            };
            let Some(sample) = target.sample(context.layout, center, 1.0, 0.0) else {
                return [None, None];
            };
            let capture = Capture {
                target,
                source: Source::Trackpad {
                    device,
                    channels: channel,
                    center,
                },
                sample,
            };
            started = Some(capture.delivery("started"));
            self.capture = Some(capture);
            self.pending = None;
        }
        let Some(capture) = &mut self.capture else {
            return [None, None];
        };
        let Source::Trackpad {
            device: owner,
            channels,
            center,
        } = &mut capture.source
        else {
            return [None, None];
        };
        if *owner != device {
            return [None, None];
        }
        match phase {
            TouchPhase::Cancelled => return [self.cancel(), None],
            TouchPhase::Ended => {
                if *channels & channel == 0 {
                    return [None, None];
                }
                *channels &= !channel;
            }
            _ => *channels |= channel,
        }
        let ending = *channels == 0;
        *center = context.input.pointer_position().unwrap_or(*center);
        let center = *center;
        let values = match delta {
            TrackpadDelta::Scale(delta) if delta.is_finite() && delta > -1.0 => Some((
                capture.sample.scale * (1.0 + delta),
                capture.sample.rotation,
            )),
            TrackpadDelta::Rotation(delta) if delta.is_finite() => {
                Some((capture.sample.scale, capture.sample.rotation - delta))
            }
            _ => None,
        };
        let updated = values.and_then(|(scale, rotation)| {
            if !scale.is_finite() || scale <= 0.0 || !rotation.is_finite() {
                return None;
            }
            let sample = capture
                .target
                .sample(context.layout, center, scale, rotation)?;
            capture.update(sample)
        });
        if ending {
            [
                updated,
                self.capture.take().map(|capture| capture.delivery("ended")),
            ]
        } else {
            [started, updated]
        }
    }
}

impl<Device> Drop for Gestures<Device> {
    fn drop(&mut self) {
        if let Some(capture) = &self.capture {
            capture.target.handle.publish(None);
        }
    }
}
