use std::{collections::BTreeMap, rc::Rc};
use xrack_layout::{Interaction, LayoutNode, Size};
use xrack_runtime::{ChangeSignal, Node, Property};

struct FrameInput {
    scene: Rc<Node>,
    viewport: Size,
    offsets: BTreeMap<Vec<usize>, f32>,
    horizontal_offsets: BTreeMap<Vec<usize>, f32>,
}

/// Reuses layout until the base scene, geometry, or an observed row changes.
#[derive(Default)]
pub(crate) struct FrameCache {
    input: Option<FrameInput>,
    input_changed: ChangeSignal,
    prepared: Property<(Rc<Node>, Rc<LayoutNode>)>,
}

impl FrameCache {
    pub(crate) fn prepare(
        &mut self,
        scene: Rc<Node>,
        viewport: Size,
        input: &Interaction,
        resolve: impl FnOnce(Rc<Node>) -> (Rc<Node>, LayoutNode),
    ) -> (Rc<Node>, Rc<LayoutNode>) {
        let offsets = input.scroll_offsets();
        let horizontal_offsets = input.horizontal_scroll_offsets();
        if !self.input.as_ref().is_some_and(|last| {
            Rc::ptr_eq(&last.scene, &scene)
                && last.viewport == viewport
                && &last.offsets == offsets
                && &last.horizontal_offsets == horizontal_offsets
        }) {
            self.input = Some(FrameInput {
                scene: scene.clone(),
                viewport,
                offsets: offsets.clone(),
                horizontal_offsets: horizontal_offsets.clone(),
            });
            self.input_changed.notify();
        }
        self.prepared.get(|| {
            self.input_changed.track();
            let (scene, layout) = resolve(scene);
            (scene, Rc::new(layout))
        })
    }
}
