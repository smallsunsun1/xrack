//! Resolve a geometric hit to the window-local 2D material source.
use std::sync::Arc;

use xrack_runtime::{
    Length, Node, Point2D, SceneFeature, Value,
    scene3d::{SceneCamera, SceneTextureHit},
};

use crate::{LayoutNode, Point, Target};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Capture {
    view: Target,
    source: Target,
    name: Arc<str>,
    object: String,
    row: Option<u32>,
    placement: Option<(u32, Option<u32>)>,
}

#[derive(Clone, Debug)]
pub(crate) struct Projection {
    pub capture: Capture,
    pub point: Point,
}

pub(crate) fn viewport_hit(
    node: &Node,
    item: &LayoutNode,
    local: Point,
) -> Option<Option<SceneTextureHit>> {
    if node.kind != "SceneView3D" || !node.boolean("texture-input", false) {
        return None;
    }
    let Some(Value::SceneObjects(model)) = node.properties.get("model") else {
        return None;
    };
    let bounds = item.bounds;
    let camera = SceneCamera::for_view(node, [f64::from(bounds.width), f64::from(bounds.height)])?;
    let point = Point2D::new(
        Length(f64::from(local.x - bounds.x)),
        Length(f64::from(local.y - bounds.y)),
    );
    camera.item_texture_at(model, point)
}

pub(crate) fn project(
    scene: &Node,
    layout: &LayoutNode,
    view_path: Vec<usize>,
    hit: SceneTextureHit,
    scope: &[usize],
) -> Option<Projection> {
    if !scene.contains_scene_feature(SceneFeature::SceneTextureSource) {
        return None;
    }
    let view = view_target(scene, &view_path)?;
    let source = source_target(scene, layout, &hit.source)?;
    if !source.path.starts_with(scope) {
        return None;
    }
    let item = layout.at_path(&source.path)?;
    Some(Projection {
        point: source_point(item, hit.uv)?,
        capture: Capture {
            view,
            source,
            name: hit.source,
            object: hit.id,
            row: hit.row,
            placement: hit.placement,
        },
    })
}

fn view_target(scene: &Node, path: &[usize]) -> Option<Target> {
    Some(Target::from_node(
        path.to_vec(),
        scene.at_path(path)?,
        false,
        scene.row_key_at(path),
    ))
}

fn source_target(scene: &Node, layout: &LayoutNode, name: &str) -> Option<Target> {
    fn find(
        node: &Node,
        layout: &LayoutNode,
        name: &str,
        path: &mut Vec<usize>,
        found: &mut Option<Vec<usize>>,
    ) -> Option<()> {
        if !layout.visible || !node.contains_scene_feature(SceneFeature::SceneTextureSource) {
            return Some(());
        }
        if node.kind == "Rectangle" && node.text("texture-source") == Some(name) {
            if found.is_some() {
                return None;
            }
            *found = Some(path.clone());
        }
        for (slot, child, item) in layout.children_in_paint_order(node) {
            path.push(slot);
            find(child, item, name, path, found)?;
            path.pop();
        }
        Some(())
    }
    let mut found = None;
    find(scene, layout, name, &mut Vec::new(), &mut found)?;
    let path = found?;
    let node = scene.at_path(&path)?;
    let item = layout.at_path(&path)?;
    if !item.enabled || node.contains_scene_feature(SceneFeature::SceneViewport) {
        return None;
    }
    let row = scene.row_key_at(&path);
    Some(Target::from_node(path, node, false, row))
}

/// Whether the source can be reached through an enabled, visible 3D view.
#[doc(hidden)]
pub fn scene_texture_source_active(
    scene: &Node,
    layout: &LayoutNode,
    path: &[usize],
    name: &str,
) -> bool {
    fn uses_source(node: &Node, item: &LayoutNode, name: &str) -> bool {
        if !item.visible
            || !item.enabled
            || !node.contains_scene_feature(SceneFeature::SceneViewport)
        {
            return false;
        }
        if node.kind == "SceneView3D"
            && node.boolean("texture-input", false)
            && let Some(Value::SceneObjects(model)) = node.properties.get("model")
            && let Some(camera) =
                SceneCamera::for_view(node, [item.bounds.width.into(), item.bounds.height.into()])
        {
            let geometry = model.geometry(&camera);
            let visibility = geometry.visibility(&camera);
            let eye = camera.eye_position();
            if geometry.iter().enumerate().any(|(index, object)| {
                object.is_pickable()
                    && object.layers & camera.layer_mask() != 0
                    && object.visible_at_distance(eye)
                    && visibility.contains(index as u32)
                    && object.mesh.uses_base_items()
                    && object
                        .mesh
                        .materials()
                        .iter()
                        .any(|material| material.base_item.as_deref() == Some(name))
            }) {
                return true;
            }
        }
        item.children_in_paint_order(node)
            .any(|(_, child, layout)| uses_source(child, layout, name))
    }
    uses_source(scene, layout, name)
        && source_target(scene, layout, name).is_some_and(|target| target.path == path)
}

pub(crate) fn focus_allowed(scene: &Node, layout: &LayoutNode, path: &[usize]) -> bool {
    if !scene.contains_scene_feature(SceneFeature::SceneTextureSource) {
        return true;
    }
    let mut node = scene;
    for depth in 0..=path.len() {
        if node.scene_texture_only() {
            return node.text("texture-source").is_some_and(|name| {
                scene_texture_source_active(scene, layout, &path[..depth], name)
            });
        }
        if let Some(&slot) = path.get(depth) {
            let Some(child) = node.child_at(slot) else {
                break;
            };
            node = child;
        }
    }
    true
}

fn source_point(item: &LayoutNode, uv: [f32; 2]) -> Option<Point> {
    if !item.visible || !item.enabled || item.bounds.width <= 0.0 || item.bounds.height <= 0.0 {
        return None;
    }
    let [u, v] = uv.map(|value| value.min(1.0 - f32::EPSILON));
    let captured = Point {
        x: item.bounds.x + u * item.bounds.width,
        y: item.bounds.y + v * item.bounds.height,
    };
    let local = item.local_transform().inverse()?.map_point(captured);
    Some(item.map_to_window(local))
}

impl Capture {
    pub fn view_path(&self) -> &[usize] {
        &self.view.path
    }

    pub fn source_path(&self) -> &[usize] {
        &self.source.path
    }

    pub fn is_attached(&self, scene: &Node, layout: &LayoutNode) -> bool {
        let visible = |path: &[usize]| {
            layout
                .at_path(path)
                .is_some_and(|item| item.visible && item.enabled)
        };
        self.view
            .node_in(scene)
            .is_some_and(|node| node.kind == "SceneView3D" && node.boolean("texture-input", false))
            && visible(self.view_path())
            && self.source.node_in(scene).is_some_and(|node| {
                node.kind == "Rectangle" && node.text("texture-source") == Some(&self.name)
            })
            && visible(self.source_path())
    }
}
