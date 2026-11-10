use std::{collections::BTreeMap, rc::Rc};
use xrack_layout::{Interaction, LayoutNode, OverscrollFrame, Size};
use xrack_runtime::{ChangeSignal, Node, Property, RowKey};

struct FrameInput {
    scene: Rc<Node>,
    viewport: Size,
    offsets: BTreeMap<Vec<usize>, f32>,
    horizontal_offsets: BTreeMap<Vec<usize>, f32>,
    tumbler_preview: Option<(Vec<usize>, f32)>,
    swipe_preview: Option<(Vec<usize>, Option<RowKey>, f32)>,
    overscroll: OverscrollFrame,
}

/// Reuses layout until the base scene, geometry, or an observed row changes.
#[derive(Default)]
pub(crate) struct FrameCache {
    input: Option<FrameInput>,
    input_changed: ChangeSignal,
    prepared: Property<(Option<Rc<Node>>, Rc<LayoutNode>)>,
    uses_base_scene: bool,
}

impl FrameCache {
    pub(crate) fn is_dirty(&self) -> bool {
        self.prepared.is_dirty()
    }

    pub(crate) fn prepare(
        &mut self,
        scene: Rc<Node>,
        viewport: Size,
        input: &Interaction,
        resolve: impl FnOnce(Rc<Node>) -> (Rc<Node>, LayoutNode),
    ) -> (Rc<Node>, Rc<LayoutNode>) {
        let offsets = input.scroll_offsets();
        let horizontal_offsets = input.horizontal_scroll_offsets();
        let tumbler_preview = input.tumbler_preview();
        let swipe_preview = input.swipe_preview();
        let overscroll = input.overscroll_frame();
        let same_geometry_inputs = self.input.as_ref().is_some_and(|last| {
            last.viewport == viewport
                && &last.offsets == offsets
                && &last.horizontal_offsets == horizontal_offsets
                && last.overscroll == overscroll
                && last
                    .tumbler_preview
                    .as_ref()
                    .map(|(path, offset)| (path.as_slice(), *offset))
                    == tumbler_preview
                && last
                    .swipe_preview
                    .as_ref()
                    .map(|(path, key, offset)| (path.as_slice(), *key, *offset))
                    == swipe_preview
        });
        if same_geometry_inputs {
            let last = self.input.as_mut().expect("matching frame input");
            if !Rc::ptr_eq(&last.scene, &scene) {
                // Prepared scenes with materialized rows must be rebuilt from
                // the new base, even if the base changed only in paint data.
                if !self.uses_base_scene || !last.scene.same_frame_inputs(&scene) {
                    self.input_changed.notify();
                }
                last.scene = scene.clone();
            }
        } else {
            self.input = Some(FrameInput {
                scene: scene.clone(),
                viewport,
                offsets: offsets.clone(),
                horizontal_offsets: horizontal_offsets.clone(),
                overscroll,
                tumbler_preview: tumbler_preview.map(|(path, offset)| (path.to_vec(), offset)),
                swipe_preview: swipe_preview
                    .map(|(path, key, offset)| (path.to_vec(), key, offset)),
            });
            self.input_changed.notify();
        }
        let (prepared, layout) = self.prepared.get(|| {
            self.input_changed.track();
            let (prepared, layout) = resolve(scene.clone());
            self.uses_base_scene = Rc::ptr_eq(&scene, &prepared);
            ((!self.uses_base_scene).then_some(prepared), Rc::new(layout))
        });
        (prepared.unwrap_or(scene), layout)
    }
}
