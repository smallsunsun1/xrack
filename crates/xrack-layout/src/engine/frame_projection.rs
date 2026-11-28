use crate::{
    ClipChain, LayoutNode, Rect, RoundedClip, Transform2D, TransformPlacement, TransformSpace,
};
use std::rc::Rc;
use xrack_runtime::Node;

pub(crate) fn apply(
    node: &Node,
    bounds: Rect,
    clip: Rect,
    space: &mut Option<Rc<TransformSpace>>,
    shapes: &mut Option<Rc<ClipChain>>,
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
    *shapes = Some(Rc::new(ClipChain {
        shape: RoundedClip {
            bounds: clip,
            radii: [0.0; 4],
        },
        parent: shapes.clone(),
        space: parent_space.clone(),
    }));
    *space = Some(Rc::new(TransformSpace {
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
pub(crate) fn reproject(old: &Node, node: &Node, layout: &LayoutNode) -> Option<LayoutNode> {
    if node.kind != "Window" || !can_reproject(old, node, layout, None) {
        return None;
    }
    let mut projected = layout.clone();
    let clip = layout.clip;
    project(node, &mut projected, clip, None, None)?;
    Some(projected)
}

// Fixed rectangles in absolute-position containers can move without measuring
// their siblings. Keep the clip, state, and paint order.
pub(crate) fn absolute_rectangle_changes(
    old: &Node,
    node: &Node,
    layout: &LayoutNode,
) -> Option<Vec<(Vec<usize>, Rect)>> {
    if node.kind != "Window" {
        return None;
    }
    let mut moved = false;
    let mut changes = Vec::new();
    collect_rectangle_changes(old, node, layout, &mut Vec::new(), &mut changes, &mut moved)?;
    moved.then_some(changes)
}

fn collect_rectangle_changes(
    old: &Node,
    node: &Node,
    layout: &LayoutNode,
    path: &mut Vec<usize>,
    changes: &mut Vec<(Vec<usize>, Rect)>,
    moved: &mut bool,
) -> Option<()> {
    if !matches!(node.kind, "Window" | "Rectangle")
        || node.application_window
        || node.list.is_some()
        || node.table_header.is_some()
        || layout.table_column_window != xrack_runtime::TableColumnWindow::default()
        || !layout.visible
        || !layout.world_transform().is_identity()
        || !old.same_own_paint_inputs(node)
        || layout.children.len() != node.children.len()
        || old.children.len() != node.children.len()
    {
        return None;
    }
    let content = if node.kind == "Window" {
        crate::linear::Padding::read(node).inner_rect(crate::window_safe_area(node, layout.bounds))
    } else {
        layout.bounds
    };
    let reference = crate::Size::new(content.width, content.height);
    for (index, (old, child)) in old.children.iter().zip(&node.children).enumerate() {
        // Moving one child can change a sibling's anchored geometry.
        if super::anchors::has_anchors(child) {
            return None;
        }
        if old.shares_storage_with(child) {
            continue;
        }
        let placed = &layout.children[index];
        if !child.children.is_empty() {
            path.push(index);
            collect_rectangle_changes(old, child, placed, path, changes, moved)?;
            path.pop();
            continue;
        }
        if child.kind != "Rectangle"
            || !crate::participates_in_flow(child)
            || !placed.visible
            || placed.details.is_some()
            || placed.space.is_some()
            || !old.same_own_paint_except_position_and_background(child)
        {
            return None;
        }
        let size = crate::linear::fixed_size(child, reference)?;
        if placed.bounds != crate::absolute_child_bounds(old, content, size) {
            return None;
        }
        let bounds = crate::absolute_child_bounds(child, content, size);
        if !bounds.x.is_finite() || !bounds.y.is_finite() {
            return None;
        }
        let position_changed = placed.bounds != bounds;
        *moved |= position_changed;
        if position_changed
            || old.properties.get("background") != child.properties.get("background")
        {
            path.push(index);
            changes.push((path.clone(), bounds));
            path.pop();
        }
    }
    Some(())
}

fn can_reproject(old: &Node, node: &Node, layout: &LayoutNode, parent: Option<&Node>) -> bool {
    // Moving a sibling can resize or move an anchored item.
    crate::item::item(node.kind).reuses_placement()
        && !node.application_window
        && !super::anchors::has_anchors(node)
        && layout.visible
        && layout.overlay_index().is_none()
        && layout.children.len() == node.children.len()
        && old.children.len() == node.children.len()
        && parent.map_or_else(
            || old.same_measurement_inputs_at_node(node),
            |parent| old.same_measurement_inputs_as_child(node, parent),
        )
        && old
            .children
            .iter()
            .zip(&node.children)
            .zip(&layout.children)
            .all(|((before, after), child)| can_reproject(before, after, child, Some(node)))
}

fn project(
    node: &Node,
    layout: &mut LayoutNode,
    parent_clip: Rect,
    mut space: Option<Rc<TransformSpace>>,
    mut shapes: Option<Rc<ClipChain>>,
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
    layout.set_transform_placement(transform);
    layout.space = space.clone();
    layout.ancestor_shapes = shapes.clone();
    if let Some(shape) = layout.child_clip_shape() {
        shapes = Some(Rc::new(ClipChain {
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
    fn fixed_window_rectangles_can_move_without_measuring_siblings() {
        let size = Size::new(120.0, 96.0);
        let input = Interaction::default();
        let mut cache = FrameCache::default();
        let mut scene = Rc::new(Node::new("Window", None));
        let window = Rc::make_mut(&mut scene);
        crate::set_window_safe_area(window, [8.0, 12.0, 3.0, 4.0]);
        window.properties.insert("padding", Length(3.0).into());
        let mut rectangle = Node::new("Rectangle", None);
        for (name, length) in [("x", 2.0), ("width", 18.0), ("height", 12.0)] {
            rectangle.properties.insert(name, Length(length).into());
        }
        window.children.push(rectangle);
        let mut caption = Node::new("Label", None);
        caption
            .properties
            .insert("text", "Status".to_owned().into());
        window.children.push(caption);
        let prepare = |scene: Rc<Node>| {
            let layout = layout_with_input(&scene, size, &mut Metrics, &input);
            (scene, layout)
        };
        let (_, first) = cache
            .prepare(scene.clone(), size, &input, prepare)
            .into_parts();
        for (x, y) in [(13.0, 16.0), (-5.0, 74.0), (115.0, 8.0)] {
            let rectangle = &mut Rc::make_mut(&mut scene).children[0];
            rectangle.properties.insert("x", Length(x).into());
            rectangle.properties.insert("y", Length(y).into());
            let (_, layout) = cache
                .prepare(scene.clone(), size, &input, |_| {
                    panic!("moving a fixed rectangle should reuse the measured layout")
                })
                .into_parts();
            assert_eq!(*layout, prepare(scene.clone()).1);
            assert_ne!(layout.children[0].bounds, first.children[0].bounds);
            assert_eq!(layout.children[1], first.children[1]);
        }

        Rc::make_mut(&mut scene).children[0]
            .properties
            .insert("width", Length(28.0).into());
        let mut measured = false;
        let (_, layout) = cache
            .prepare(scene.clone(), size, &input, |scene| {
                measured = true;
                prepare(scene)
            })
            .into_parts();
        assert!(measured);
        assert_eq!(*layout, prepare(scene).1);
    }

    #[test]
    fn moving_a_rectangle_repositions_its_anchored_sibling() {
        let size = Size::new(120.0, 96.0);
        let input = Interaction::default();
        let mut cache = FrameCache::default();
        let mut tile = Node::new("Rectangle", Some("tile"));
        for (name, length) in [("x", 12.0), ("width", 20.0), ("height", 8.0)] {
            tile.properties.insert(name, Length(length).into());
        }
        let mut follower = Node::new("Rectangle", None);
        follower
            .properties
            .insert("anchor-left", "tile.right".to_owned().into());
        follower.properties.insert("width", Length(12.0).into());
        follower.properties.insert("height", Length(8.0).into());
        let mut window = Node::new("Window", None);
        window.children.extend([tile, follower]);
        let mut scene = Rc::new(window);
        let resolve = |scene: Rc<Node>| {
            let layout = layout_with_input(&scene, size, &mut Metrics, &input);
            (scene, layout)
        };

        let first = cache.prepare(scene.clone(), size, &input, resolve);
        assert_eq!(first.layout().children[1].bounds.x, 32.0);
        Rc::make_mut(&mut scene).children[0]
            .properties
            .insert("x", Length(26.0).into());
        let current = cache.prepare(scene.clone(), size, &input, resolve);
        assert_eq!(current.layout().children[1].bounds.x, 46.0);
        assert_eq!(*current.layout().as_ref(), resolve(scene).1);
    }

    #[test]
    fn rectangles_inside_clipped_panels_keep_the_fresh_layout_and_hit_bounds() {
        let input = Interaction::default();
        let size = Size::new(196.0, 128.0);
        let place = |mut node: Node, lengths| {
            for (name, value) in ["x", "y", "width", "height"].into_iter().zip(lengths) {
                node.properties.insert(name, Length(value).into());
            }
            node
        };
        let mut outer = place(Node::new("Rectangle", None), [12.0, 9.0, 100.0, 72.0]);
        let mut inner = place(Node::new("Rectangle", None), [7.0, 6.0, 60.0, 45.0]);
        inner.properties.insert("clip", true.into());
        inner
            .children
            .push(place(Node::new("Rectangle", None), [5.0, 4.0, 20.0, 12.0]));
        outer.children.push(inner);
        let mut window = Node::new("Window", None);
        window.children.push(outer);
        let mut scene = Rc::new(window);
        let mut cache = FrameCache::default();
        let (before_scene, before) = cache
            .prepare_reusing(scene.clone(), size, &input, |scene, _| {
                let layout = layout_with_input(&scene, size, &mut Metrics, &input);
                (scene, layout)
            })
            .into_parts();
        Rc::make_mut(&mut scene).children[0].children[0].children[0]
            .properties
            .insert("x", Length(49.0).into());
        let (current_scene, current) = cache
            .prepare_reusing(scene.clone(), size, &input, |_, _| {
                panic!(
                    "a fixed rectangle moving inside an absolute-position panel should reuse layout"
                )
            })
            .into_parts();

        assert_eq!(
            *current,
            layout_with_input(&scene, size, &mut Metrics, &input)
        );
        let path = [0, 0, 0];
        let tile = current.at_path(&path).unwrap();
        assert!(tile.contains_subtree_point(crate::Point { x: 72.0, y: 22.0 }));
        assert!(!tile.contains_subtree_point(crate::Point { x: 83.0, y: 22.0 }));
        assert_eq!(
            current.window_child_changes_from(&before, &before_scene, &current_scene),
            Some(&[(path.to_vec(), tile.bounds)][..]),
        );
        assert_eq!(before.at_path(&path).unwrap().bounds.x, 24.0);
    }

    #[test]
    fn transform_and_fixed_caption_updates_reuse_the_measured_layout() {
        let mut window = Node::new("Window", None);
        let mut group = Node::new("Rectangle", None);
        let mut caption = Node::new("Label", None);
        caption.properties.insert("width", Length(60.0).into());
        caption.properties.insert("height", Length(18.0).into());
        caption.properties.insert("text", "Ready".to_owned().into());
        group.children.push(caption);
        window.children.push(group);
        let mut scene = Rc::new(window);
        let input = Interaction::default();
        let size = Size::new(120.0, 96.0);
        let mut cache = FrameCache::default();
        cache.prepare(scene.clone(), size, &input, |scene| {
            let layout = layout_with_input(&scene, size, &mut Metrics, &input);
            (scene, layout)
        });

        let group = &mut Rc::make_mut(&mut scene).children[0];
        group.properties.insert("transform-rotation", 18.0.into());
        group.children[0]
            .properties
            .insert("text", "A new status".to_owned().into());
        let (prepared, reused) = cache
            .prepare(scene.clone(), size, &input, |_| {
                panic!("a transform and fixed caption change should only project the layout")
            })
            .into_parts();
        assert_eq!(
            prepared.children[0].children[0].text("text"),
            Some("A new status")
        );
        assert_eq!(
            *reused,
            layout_with_input(&scene, size, &mut Metrics, &input)
        );
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
            let (_, cached) = cache
                .prepare(scene.clone(), size, &input, |_| {
                    panic!("transform-only changes should reuse measured bounds")
                })
                .into_parts();
            let fresh = layout_with_input(&scene, size, &mut Metrics, &input);
            assert_eq!(*cached, fresh);
        }
        for scale in [0.0_f64, 0.9] {
            Rc::make_mut(&mut scene).children[0]
                .properties
                .insert("transform-scale", scale.into());
            let mut measured = false;
            let (_, actual) = cache
                .prepare(scene.clone(), size, &input, |scene| {
                    measured = true;
                    prepare(scene)
                })
                .into_parts();
            assert!(measured);
            assert_eq!(
                *actual,
                layout_with_input(&scene, size, &mut Metrics, &input)
            );
        }
    }
}
