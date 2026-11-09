use crate::{LayoutNode, Target, input::valid};
use xrack_runtime::{Component, EventResult, KeyEvent, KeyPhase, Node};

/// The focus path for a single keyboard event.
pub struct KeyRoute {
    focus: Target,
    scopes: Vec<Target>,
}

impl KeyRoute {
    pub(crate) fn new(focus: Target, scopes: Vec<Target>) -> Option<Self> {
        (!scopes.is_empty()).then_some(Self { focus, scopes })
    }

    pub fn focused(&self) -> &Target {
        &self.focus
    }

    /// A phase returns None when no scope has a live handler for it.
    pub fn dispatch(
        &self,
        component: &mut impl Component,
        scene: &Node,
        layout: &LayoutNode,
        phase: KeyPhase,
        event: &KeyEvent,
    ) -> Option<EventResult> {
        if !valid(&self.focus, scene, layout) {
            return None;
        }
        let mut result = None;
        let mut visit = |scope: &Target| {
            if !valid(scope, scene, layout)
                || !scene.at_path(&scope.path).is_some_and(|node| {
                    node.kind == "FocusScope" && node.key_handlers & phase.mask() != 0
                })
            {
                return false;
            }
            result = Some(component.key_event_at(scene, &scope.path, phase, event));
            result == Some(EventResult::Accept)
        };
        let _ = match phase {
            KeyPhase::CapturePressed | KeyPhase::CaptureReleased => {
                self.scopes.iter().any(&mut visit)
            }
            KeyPhase::Pressed | KeyPhase::Released => self.scopes.iter().rev().any(&mut visit),
        };
        result
    }
}
