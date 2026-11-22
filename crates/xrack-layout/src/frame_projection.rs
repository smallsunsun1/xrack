use crate::{
    ClipChain, LayoutNode, Rect, RoundedClip, Transform2D, TransformPlacement, TransformSpace,
};
use std::sync::Arc;
use xrack_runtime::Node;

pub(super) fn apply(
    node: &Node,
    bounds: Rect,
    clip: Rect,
    space: &mut Option<Arc<TransformSpace>>,
    shapes: &mut Option<Arc<ClipChain>>,
) -> (Rect, Option<Box<TransformPlacement>>) {
    let local = Transform2D::for_node(node, bounds);
    if local.is_identity() {
        return (clip, None);
    }
    let parent_space = space.clone();
    let world = local.then(
        parent_space
            .as_deref()
            .map_or(Transform2D::IDENTITY, |space| space.world),
    );
    *shapes = Some(Arc::new(ClipChain {
        shape: RoundedClip {
            bounds: clip,
            radii: [0.0; 4],
        },
        parent: shapes.clone(),
        space: parent_space.clone(),
    }));
    *space = Some(Arc::new(TransformSpace {
        world,
        inverse: world.inverse(),
    }));
    let transform = TransformPlacement {
        local,
        parent_clip: clip,
        parent_space,
    };
    let clip = local
        .inverse()
        .map_or(Rect::default(), |inverse| inverse.map_rect(clip));
    (clip, Some(Box::new(transform)))
}

/// Reuse measured bounds for transform updates in trees without
/// viewport-driven layout or placement-dependent controls.
pub(super) fn reproject(old: &Node, node: &Node, layout: &LayoutNode) -> Option<LayoutNode> {
    if node.kind != "Window" || !can_reproject(old, node, layout) {
        return None;
    }
    let mut projected = layout.clone();
    let clip = layout.clip;
    project(node, &mut projected, clip, None, None)?;
    Some(projected)
}

fn can_reproject(old: &Node, node: &Node, layout: &LayoutNode) -> bool {
    matches!(
        node.kind,
        "Window"
            | "Rectangle"
            | "Text"
            | "Label"
            | "Image"
            | "HorizontalLayout"
            | "VerticalLayout"
            | "GridLayout"
    ) && !node.application_window
        && layout.visible
        && layout.overlays.is_none()
        && layout.children.len() == node.children.len()
        && old.children.len() == node.children.len()
        && old.same_measurement_inputs_at_node(node)
        && old
            .children
            .iter()
            .zip(&node.children)
            .zip(&layout.children)
            .all(|((before, after), child)| can_reproject(before, after, child))
}

fn project(
    node: &Node,
    layout: &mut LayoutNode,
    parent_clip: Rect,
    mut space: Option<Arc<TransformSpace>>,
    mut shapes: Option<Arc<ClipChain>>,
) -> Option<()> {
    let (clip, transform) = apply(node, layout.bounds, parent_clip, &mut space, &mut shapes);
    // A singular ancestor can collapse the bounds of its children during
    // layout. Rebuild on that transition, including when it becomes visible.
    if space
        .as_ref()
        .is_some_and(|current| current.inverse.is_none())
    {
        return None;
    }
    layout.clip = clip;
    layout.transform = transform;
    layout.space = space.clone();
    layout.ancestor_shapes = shapes.clone();
    if let Some(shape) = layout.child_shape {
        shapes = Some(Arc::new(ClipChain {
            shape,
            parent: shapes,
            space: space.clone(),
        }));
    }
    let child_clip = if layout.overflow {
        clip
    } else {
        let bounds = if node.kind == "Rectangle" && node.boolean("clip", false) {
            RoundedClip::content_box(node, layout.bounds).bounds
        } else {
            layout.bounds
        };
        clip.intersect(bounds)
    };
    for (child, placed) in node.children.iter().zip(&mut layout.children) {
        project(child, placed, child_clip, space.clone(), shapes.clone())?;
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use crate::{FrameCache, Interaction, Size, TextMeasurer, layout_with_input};
    use std::rc::Rc;
    use xrack_runtime::{Length, Node};

    struct Metrics;

    impl TextMeasurer for Metrics {
        fn measure(&mut self, text: &str, size: f32, _: f32) -> Size {
            Size::new(text.len() as f32 * size * 0.5, size)
        }
    }

    #[test]
    fn nested_visual_changes_reuse_layout_and_match_fresh_clipping() {
        let mut window = Node::new("Window", None);
        let mut group = Node::new("Rectangle", Some("group"));
        for (key, value) in [
            ("x", 12.0),
            ("y", 18.0),
            ("width", 50.0),
            ("height", 42.0),
            ("border-radius", 9.0),
        ] {
            group.properties.insert(key, Length(value).into());
        }
        group.properties.insert("clip", true.into());
        let mut text = Node::new("Text", Some("content"));
        text.properties.insert("text", "Hello".to_owned().into());
        text.properties.insert("transform-scale-x", (-1.0).into());
        group.children.push(text);
        window.children.push(group);
        let mut scene = Rc::new(window);
        let mut cache = FrameCache::default();
        let size = Size::new(110.0, 98.0);
        let input = Interaction::default();
        let prepare = |scene: Rc<Node>| {
            let layout = layout_with_input(&scene, size, &mut Metrics, &input);
            (scene, layout)
        };
        cache.prepare(scene.clone(), size, &input, prepare);
        for (angle, scale) in [(37.0, 0.8), (-22.0, 1.1)] {
            let group = &mut Rc::make_mut(&mut scene).children[0];
            group.properties.insert("transform-rotation", angle.into());
            group.properties.insert("transform-scale", scale.into());
            let (_, cached) = cache.prepare(scene.clone(), size, &input, |_| {
                panic!("transform-only changes should reuse measured bounds")
            });
            let fresh = layout_with_input(&scene, size, &mut Metrics, &input);
            assert_eq!(*cached, fresh);
        }
        for scale in [0.0_f64, 0.9] {
            Rc::make_mut(&mut scene).children[0]
                .properties
                .insert("transform-scale", scale.into());
            let mut measured = false;
            let (_, actual) = cache.prepare(scene.clone(), size, &input, |scene| {
                measured = true;
                prepare(scene)
            });
            assert!(measured);
            assert_eq!(
                *actual,
                layout_with_input(&scene, size, &mut Metrics, &input)
            );
        }
    }
}
