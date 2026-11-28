use crate::{Interaction, LayoutNode, OverscrollFrame, Rect, Size};
use std::{
    collections::BTreeMap,
    rc::{Rc, Weak},
    sync::atomic::{AtomicU64, Ordering},
};
use xrack_runtime::{ChangeSignal, Node, Property, RowKey};

struct FrameInput {
    scene: Rc<Node>,
    viewport: Size,
    offsets: BTreeMap<Vec<usize>, f32>,
    horizontal_offsets: BTreeMap<Vec<usize>, f32>,
    tumbler_preview: Option<(Vec<usize>, f32)>,
    slider_preview: Option<(Vec<usize>, f64)>,
    scene_slider_previews: Vec<(Vec<usize>, Option<usize>, f64)>,
    switch_previews: Vec<(Vec<usize>, Option<RowKey>, f32)>,
    swipe_preview: Option<(Vec<usize>, Option<RowKey>, f32)>,
    drawer_preview: Option<(Vec<usize>, f32)>,
    overscroll: OverscrollFrame,
    floating_keyboard: Option<Rect>,
    keyboard_avoidance: Option<(Vec<usize>, f32)>,
}

/// Work performed by the layout cache to prepare a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FramePreparation {
    Reused,
    Reprojected,
    LaidOut,
}

/// The final scene and geometry for one frame.
///
/// This pairs scene nodes with their matching layout. Live handles inside
/// the scene still refer to their runtime state.
#[derive(Clone)]
pub struct PreparedFrame {
    scene: Rc<Node>,
    layout: Rc<LayoutNode>,
    viewport: Size,
    generation: u64,
    preparation: FramePreparation,
    uses_base_scene: bool,
}

impl PreparedFrame {
    pub fn scene(&self) -> &Rc<Node> {
        &self.scene
    }

    pub fn layout(&self) -> &Rc<LayoutNode> {
        &self.layout
    }

    pub fn parts(&self) -> (&Rc<Node>, &Rc<LayoutNode>) {
        (&self.scene, &self.layout)
    }

    pub fn into_parts(self) -> (Rc<Node>, Rc<LayoutNode>) {
        (self.scene, self.layout)
    }

    /// Logical viewport used to prepare this frame.
    pub fn viewport(&self) -> Size {
        self.viewport
    }

    /// Changes when the scene or its layout changes.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn preparation(&self) -> FramePreparation {
        self.preparation
    }

    /// Whether preparation used the base tree without creating a derived scene.
    pub fn uses_base_scene(&self) -> bool {
        self.uses_base_scene
    }
}

/// Reuses layout until the base scene, geometry, or an observed row changes.
#[derive(Default)]
#[doc(hidden)]
pub struct FrameCache {
    input: Option<FrameInput>,
    input_changed: ChangeSignal,
    prepared: Property<(Option<Rc<Node>>, Rc<LayoutNode>)>,
    uses_base_scene: bool,
    typography: Option<Weak<()>>,
    generation: u64,
    layout: Weak<LayoutNode>,
    recyclable: Option<Rc<LayoutNode>>,
}

impl FrameCache {
    pub fn update_typography(&mut self, revision: Option<Rc<()>>) {
        let revision = revision.as_ref().map(Rc::downgrade);
        let same = match (&self.typography, &revision) {
            (Some(old), Some(new)) => old.ptr_eq(new),
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.typography = revision;
            self.input_changed.notify();
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.prepared.is_dirty()
    }

    pub fn slider_preview_changed(&self, input: &Interaction) -> bool {
        self.input
            .as_ref()
            .and_then(|last| last.slider_preview.as_ref())
            .map(|(path, fraction)| (path.as_slice(), *fraction))
            != input.slider_preview_position()
            || !self
                .input
                .as_ref()
                .into_iter()
                .flat_map(|last| &last.scene_slider_previews)
                .map(|(path, thumb, fraction)| (path.as_slice(), *thumb, *fraction))
                .eq(input.scene_slider_previews())
    }

    pub fn prepare(
        &mut self,
        scene: Rc<Node>,
        viewport: Size,
        input: &Interaction,
        resolve: impl FnOnce(Rc<Node>) -> (Rc<Node>, LayoutNode),
    ) -> PreparedFrame {
        self.prepare_impl(scene, viewport, input, false, |scene, _| resolve(scene))
    }

    #[doc(hidden)]
    pub fn prepare_reusing(
        &mut self,
        scene: Rc<Node>,
        viewport: Size,
        input: &Interaction,
        resolve: impl FnOnce(Rc<Node>, Option<LayoutNode>) -> (Rc<Node>, LayoutNode),
    ) -> PreparedFrame {
        self.prepare_impl(scene, viewport, input, true, resolve)
    }

    fn prepare_impl(
        &mut self,
        scene: Rc<Node>,
        viewport: Size,
        input: &Interaction,
        recycle: bool,
        resolve: impl FnOnce(Rc<Node>, Option<LayoutNode>) -> (Rc<Node>, LayoutNode),
    ) -> PreparedFrame {
        let changed_scene = self
            .input
            .as_ref()
            .is_none_or(|input| !Rc::ptr_eq(&input.scene, &scene));
        let offsets = input.scroll_offsets();
        let horizontal_offsets = input.horizontal_scroll_offsets();
        let tumbler_preview = input.tumbler_preview();
        let switch_previews = input.switch_previews();
        let swipe_preview = input.swipe_preview();
        let drawer_preview = input.drawer_preview();
        let overscroll = input.overscroll_frame();
        let floating_keyboard = input.floating_keyboard();
        let keyboard_avoidance = input.keyboard_avoidance();
        let same_geometry_inputs = self.input.as_ref().is_some_and(|last| {
            last.viewport == viewport
                && &last.offsets == offsets
                && &last.horizontal_offsets == horizontal_offsets
                && last.overscroll == overscroll
                && last.floating_keyboard == floating_keyboard
                && last
                    .keyboard_avoidance
                    .as_ref()
                    .map(|(path, shift)| (path.as_slice(), *shift))
                    == keyboard_avoidance
                && !self.slider_preview_changed(input)
                && last
                    .tumbler_preview
                    .as_ref()
                    .map(|(path, offset)| (path.as_slice(), *offset))
                    == tumbler_preview
                && last
                    .switch_previews
                    .iter()
                    .map(|(path, key, position)| (path.as_slice(), *key, *position))
                    .eq(switch_previews)
                && last
                    .swipe_preview
                    .as_ref()
                    .map(|(path, key, offset)| (path.as_slice(), *key, *offset))
                    == swipe_preview
                && last
                    .drawer_preview
                    .as_ref()
                    .map(|(path, reveal)| (path.as_slice(), *reveal))
                    == drawer_preview
        });
        let mut projection = None;
        if same_geometry_inputs && let Some(last) = &mut self.input {
            if !Rc::ptr_eq(&last.scene, &scene) {
                // Prepared scenes with materialized rows must be rebuilt from
                // the new base, even if the base changed only in paint data.
                if !self.uses_base_scene || !last.scene.same_frame_inputs(&scene) {
                    if self.uses_base_scene
                        && !self.prepared.is_dirty()
                        && let Some(layout) = self.layout.upgrade()
                    {
                        projection =
                            crate::frame_projection::reproject(&last.scene, &scene, &layout);
                    }
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
                floating_keyboard,
                keyboard_avoidance: keyboard_avoidance.map(|(path, shift)| (path.to_vec(), shift)),
                tumbler_preview: tumbler_preview.map(|(path, offset)| (path.to_vec(), offset)),
                slider_preview: input
                    .slider_preview_position()
                    .map(|(path, fraction)| (path.to_vec(), fraction)),
                scene_slider_previews: input
                    .scene_slider_previews()
                    .map(|(path, thumb, fraction)| (path.to_vec(), thumb, fraction))
                    .collect(),
                switch_previews: input
                    .switch_previews()
                    .map(|(path, key, position)| (path.to_vec(), key, position))
                    .collect(),
                swipe_preview: swipe_preview
                    .map(|(path, key, offset)| (path.to_vec(), key, offset)),
                drawer_preview: drawer_preview.map(|(path, reveal)| (path.to_vec(), reveal)),
            });
            self.input_changed.notify();
        }
        let recyclable = if recycle && self.prepared.is_dirty() && projection.is_none() {
            // The host can still hold the current layout for reconciliation.
            // Give it a frame to release that snapshot before taking ownership.
            let previous = self.recyclable.take().and_then(Rc::into_inner);
            self.recyclable = self.layout.upgrade();
            previous
        } else {
            self.recyclable = None;
            None
        };
        let mut reused = true;
        let projected = projection.is_some();
        let (prepared, layout) = self.prepared.get(|| {
            reused = false;
            self.input_changed.track();
            if let Some(layout) = projection {
                return (None, Rc::new(layout));
            }
            let (prepared, layout) = resolve(scene.clone(), recyclable);
            self.uses_base_scene = Rc::ptr_eq(&scene, &prepared);
            ((!self.uses_base_scene).then_some(prepared), Rc::new(layout))
        });
        self.layout = Rc::downgrade(&layout);
        let preparation = if reused {
            xrack_runtime::update_profile::layout(true);
            FramePreparation::Reused
        } else if projected {
            xrack_runtime::update_profile::layout_reprojected();
            FramePreparation::Reprojected
        } else {
            xrack_runtime::update_profile::layout(false);
            FramePreparation::LaidOut
        };
        if changed_scene || !reused {
            static NEXT_FRAME: AtomicU64 = AtomicU64::new(1);
            self.generation = NEXT_FRAME.fetch_add(1, Ordering::Relaxed);
        }
        PreparedFrame {
            scene: prepared.unwrap_or(scene),
            layout,
            viewport,
            generation: self.generation,
            preparation,
            uses_base_scene: self.uses_base_scene,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TextMeasurer, layout_with_input};
    use xrack_runtime::Length;

    struct Metrics;

    impl TextMeasurer for Metrics {
        fn measure(&mut self, _: &str, _: f32, _: f32) -> Size {
            Size::default()
        }
    }

    #[test]
    fn prepared_frames_distinguish_reuse_projection_and_layout() {
        use xrack_runtime::{Color, update_profile};

        let mut window = Node::new("Window", None);
        let mut child = Node::new("Rectangle", None);
        child.mark_paint_only_background();
        child.properties.insert("width", Length(40.0).into());
        child.properties.insert("height", Length(32.0).into());
        window.children.push(child);
        let mut base = Rc::new(window);
        let mut cache = FrameCache::default();
        let size = Size::new(120.0, 90.0);
        let input = Interaction::default();
        let resolve = |base: Rc<Node>| {
            let geometry = layout_with_input(&base, size, &mut Metrics, &input);
            (base, geometry)
        };
        let first = cache.prepare(base.clone(), size, &input, resolve);
        assert_eq!(first.preparation(), FramePreparation::LaidOut);

        Rc::make_mut(&mut base).children[0]
            .properties
            .insert("background", Color(0x123456ff).into());
        let mut paint_generation = first.generation();
        let profile = update_profile::measure(|| {
            let next = cache.prepare(base.clone(), size, &input, |_| panic!("paint only"));
            paint_generation = next.generation();
            assert_eq!(next.preparation(), FramePreparation::Reused);
            assert!(Rc::ptr_eq(next.parts().1, first.parts().1));
            assert!(next.generation() > first.generation());
        })
        .unwrap();
        assert_eq!(
            (
                profile.layout_direct_reuses,
                profile.layout_reprojections,
                profile.layout_runs
            ),
            (1, 0, 0)
        );

        Rc::make_mut(&mut base).children[0]
            .properties
            .insert("transform-rotation", 20.0.into());
        let profile = update_profile::measure(|| {
            let next = cache.prepare(base.clone(), size, &input, |_| panic!("transform only"));
            assert_eq!(next.preparation(), FramePreparation::Reprojected);
            assert_eq!(next.parts().1.as_ref(), &resolve(base.clone()).1);
            assert!(next.generation() > paint_generation);
        })
        .unwrap();
        assert_eq!(
            (
                profile.layout_direct_reuses,
                profile.layout_reprojections,
                profile.layout_runs
            ),
            (0, 1, 0)
        );
        assert_eq!(profile.layout_reuses, 1);

        Rc::make_mut(&mut base).children[0]
            .properties
            .insert("width", Length(64.0).into());
        let profile = update_profile::measure(|| {
            let next = cache.prepare(base.clone(), size, &input, resolve);
            assert_eq!(next.preparation(), FramePreparation::LaidOut);
            assert_eq!(next.parts().1.as_ref(), &resolve(base.clone()).1);
        })
        .unwrap();
        assert_eq!(
            (
                profile.layout_direct_reuses,
                profile.layout_reprojections,
                profile.layout_runs
            ),
            (0, 0, 1)
        );
    }

    #[test]
    fn recycling_keeps_shared_snapshots_and_returns_an_exclusive_older_layout() {
        let mut child = Node::new("Rectangle", None);
        child.properties.insert("width", Length(16.0).into());
        child.properties.insert("height", Length(16.0).into());
        let mut window = Node::new("Window", None);
        window.children.push(child);
        let mut scene = Rc::new(window);
        let mut cache = FrameCache::default();
        let viewport = Size::new(128.0, 96.0);
        let input = Interaction::default();

        let (_, held) = cache
            .prepare_reusing(scene.clone(), viewport, &input, |scene, previous| {
                assert!(previous.is_none());
                let fresh = layout_with_input(&scene, viewport, &mut Metrics, &input);
                (scene, fresh)
            })
            .into_parts();
        let saved = held.as_ref().clone();

        for step in 1..=4 {
            let width = 16 + step;
            Rc::make_mut(&mut scene).children[0]
                .properties
                .insert("width", Length(f64::from(width)).into());
            let (_, current) = cache
                .prepare_reusing(scene.clone(), viewport, &input, |scene, previous| {
                    assert_eq!(
                        previous
                            .as_ref()
                            .map(|layout| layout.children[0].bounds.width),
                        (step >= 3).then_some((width - 2) as f32),
                    );
                    let fresh = layout_with_input(&scene, viewport, &mut Metrics, &input);
                    (scene, fresh)
                })
                .into_parts();
            assert_eq!(current.children[0].bounds.width, width as f32);
        }
        assert_eq!(*held, saved);
    }
}
