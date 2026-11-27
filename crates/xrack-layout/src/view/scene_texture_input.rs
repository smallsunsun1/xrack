//! Resolve a geometric hit to the window-local 2D material source.
use std::sync::Arc;

use xrack_runtime::{
    Component, FIRST_SCENE_TOUCH_FINGER_ID, GeometryHandle, KeyModifiers, Length, Node, Point2D,
    PointerButton, PointerEvent, PointerEventKind, RangeAction, RangePositionHandle, SceneFeature,
    SceneTouchRequest, Value,
    scene_effect::SceneScissor,
    scene3d::{
        SceneCamera, SceneItemTexture, SceneTextureHit, SceneTextureMapping, SceneTextureSources,
    },
};

use crate::{
    Interaction, LayoutNode, Point, PointerSequence, Target,
    input::{pointer_range_action, scene_control_live},
    range::{RangePointerAction, RangePointerMotion, RangePointerSample, has_range_pointer},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Capture {
    view: Target,
    source: Target,
    name: Arc<str>,
    object: String,
    row: Option<u32>,
    placement: Option<(u32, Option<u32>)>,
    tile: [i64; 2],
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
    if SceneScissor::for_view(node).is_some_and(|clip| !clip.contains([point.x.0, point.y.0])) {
        return None;
    }
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
            tile: hit.tile,
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
        if node.scene_texture_source() == Some(name) {
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
    if !can_use_source(node, item) {
        return None;
    }
    let row = scene.row_key_at(&path);
    Some(Target::from_node(path, node, false, row))
}

fn geometry_uses_source(node: &Node, camera: &SceneCamera, name: &str, visible_only: bool) -> bool {
    let Some(Value::SceneObjects(model)) = node.properties.get("model") else {
        return false;
    };
    let geometry = model.geometry(camera);
    let visibility = visible_only.then(|| geometry.visibility(camera));
    let eye = camera.eye_position();
    geometry.iter().enumerate().any(|(index, object)| {
        object.is_pickable()
            && object.layers & camera.layer_mask() != 0
            && object.visible_at_distance(eye)
            && visibility
                .as_ref()
                .is_none_or(|visible| visible.contains(index as u32))
            && object.mesh.uses_base_items()
            && object.mesh.materials().iter().any(|material| {
                material.base_item.as_deref().map(SceneItemTexture::source) == Some(name)
            })
    })
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
            && matches!(node.properties.get("model"), Some(Value::SceneObjects(_)))
            && let Some(camera) =
                SceneCamera::for_view(node, [item.bounds.width.into(), item.bounds.height.into()])
        {
            if SceneScissor::for_view(node).is_some_and(|clip| {
                let [left, top, right, bottom] = clip.relative_to(camera.content_rect());
                right <= left || bottom <= top
            }) {
                return false;
            }
            if geometry_uses_source(node, &camera, name, true) {
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

fn can_use_source(node: &Node, item: &LayoutNode) -> bool {
    item.enabled
        && node.kind == "Rectangle"
        && !node.contains_scene_feature(SceneFeature::SceneViewport)
}

/// Snapshot just the sources eligible for the same mapping used by pointer dispatch.
pub(crate) fn query_sources(scene: &Node, layout: &LayoutNode) -> Option<Arc<SceneTextureSources>> {
    fn collect(node: &Node, item: &LayoutNode, sources: &mut SceneTextureSources) {
        if !item.visible || !node.contains_scene_feature(SceneFeature::SceneTextureSource) {
            return;
        }
        if let Some(name) = node.scene_texture_source() {
            sources
                .entry(Arc::from(name))
                .and_modify(|entry| *entry = None)
                .or_insert_with(|| {
                    can_use_source(node, item)
                        .then(|| source_mapping(item))
                        .flatten()
                });
        }
        for (_, child, layout) in item.children_in_paint_order(node) {
            collect(child, layout, sources);
        }
    }

    let mut sources = SceneTextureSources::new();
    collect(scene, layout, &mut sources);
    sources
        .values()
        .any(Option::is_some)
        .then(|| Arc::new(sources))
}

fn source_mapping(item: &LayoutNode) -> Option<SceneTextureMapping> {
    if !item.visible || !item.enabled || item.bounds.width <= 0.0 || item.bounds.height <= 0.0 {
        return None;
    }
    let bounds = item.bounds;
    Some(SceneTextureMapping {
        bounds: [bounds.x, bounds.y, bounds.width, bounds.height],
        inverse: item.local_transform().inverse()?.coefficients(),
    })
}

fn source_point(item: &LayoutNode, uv: [f32; 2]) -> Option<Point> {
    let [x, y] = source_mapping(item)?.layout_point(uv);
    Some(item.map_to_window(Point { x, y }))
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

/// Programmatic contacts in this window. A request is resolved against the
/// current mounted view and panel, and never moves the physical cursor.
#[doc(hidden)]
pub struct SceneTouches {
    active: Vec<SceneTouch>,
    next_finger: i64,
}

struct SceneTouch {
    view: GeometryHandle,
    id: i64,
    name: String,
    source: Target,
    last: Point2D,
    capture: SceneContact,
}

enum SceneContact {
    TouchArea {
        sequence: PointerSequence,
        click: bool,
    },
    Button {
        target: Target,
        finger: i64,
        kind: &'static str,
        handle: Option<xrack_runtime::ButtonPressHandle>,
    },
    Range(SceneRange),
}

struct SceneRange {
    target: Target,
    finger: i64,
    kind: &'static str,
    thumb: Option<usize>,
    motion: RangePointerMotion,
    preview: Option<f64>,
    handle: Option<RangePositionHandle>,
}

impl Default for SceneTouches {
    fn default() -> Self {
        Self {
            active: Vec::new(),
            next_finger: FIRST_SCENE_TOUCH_FINGER_ID,
        }
    }
}

struct TouchContext<'a> {
    input: &'a mut Interaction,
    scene: &'a Node,
    layout: &'a LayoutNode,
}

impl TouchContext<'_> {
    fn source(&self, view: &GeometryHandle, name: &str) -> Option<Target> {
        fn find_view<'a>(
            node: &'a Node,
            item: &'a LayoutNode,
            view: &GeometryHandle,
            path: &mut Vec<usize>,
        ) -> Option<(&'a Node, &'a LayoutNode)> {
            if !item.visible
                || !item.enabled
                || !node.contains_scene_feature(SceneFeature::SceneViewport)
            {
                return None;
            }
            if node.kind == "SceneView3D" && node.geometry_handle.as_ref() == Some(view) {
                return Some((node, item));
            }
            for (index, child, layout) in item.children_in_scene_order(node) {
                path.push(index);
                if let Some(found) = find_view(child, layout, view, path) {
                    return Some(found);
                }
                path.pop();
            }
            None
        }

        if self.input.menu_open() || self.input.choice_open() {
            return None;
        }
        let source = source_target(self.scene, self.layout, name)?;
        let mut path = Vec::new();
        let (view, item) = find_view(self.scene, self.layout, view, &mut path)?;
        if !view.boolean("texture-input", false)
            || !self.input.allows_popup(self.layout, &path)
            || !self.input.allows_popup(self.layout, &source.path)
        {
            return None;
        }
        let camera =
            SceneCamera::for_view(view, [item.bounds.width.into(), item.bounds.height.into()])?;
        geometry_uses_source(view, &camera, name, false).then_some(source)
    }

    fn map(&self, source: &Target, local: Point2D, inside: bool) -> Option<Point> {
        let panel = self.layout.at_path(&source.path)?;
        let bounds = panel.bounds;
        let (x, y) = (local.x.0 as f32, local.y.0 as f32);
        if !(x.is_finite() && y.is_finite()) {
            return None;
        }
        if inside && !((0.0..bounds.width).contains(&x) && (0.0..bounds.height).contains(&y)) {
            return None;
        }
        Some(panel.map_to_window(Point {
            x: bounds.x + x,
            y: bounds.y + y,
        }))
    }
}

fn scene_button(kind: &str) -> bool {
    matches!(
        kind,
        "Button" | "RoundButton" | "ToolButton" | "CheckBox" | "RadioButton" | "Switch"
    )
}

impl SceneRange {
    fn begin(
        context: &mut TouchContext<'_>,
        component: &mut impl Component,
        target: Target,
        finger: i64,
        point: Point,
    ) -> Option<Self> {
        let node = target.node_in(context.scene)?;
        let item = context.layout.at_path(&target.path)?;
        let local = item.map_from_window(point)?;
        let mut motion = RangePointerMotion::default();
        let sample = motion.press(node, item, local);
        let mut drag = Self {
            target: target.clone(),
            finger,
            kind: node.kind,
            thumb: None,
            motion,
            preview: None,
            handle: node.range_position_handle.clone(),
        };
        context
            .input
            .begin_scene_control(context.scene, target, finger, None);
        if let Some(sample) = sample {
            drag.apply_sample(context, component, node, sample);
        }
        Some(drag)
    }

    fn adjust(&self, component: &mut impl Component, scene: &Node, action: RangeAction) {
        if let Some(thumb) = self.thumb {
            let mut path = self.target.path.clone();
            path.push(thumb);
            component.adjust_range_at(scene, &path, action);
        } else {
            component.adjust_range_at(scene, &self.target.path, action);
        }
    }

    fn apply_sample(
        &mut self,
        context: &mut TouchContext<'_>,
        component: &mut impl Component,
        node: &Node,
        sample: RangePointerSample,
    ) {
        if let Some(thumb) = sample.thumb {
            self.thumb = Some(thumb);
        }
        let (preview, action) = match sample.action {
            Some(RangePointerAction::Fraction(fraction)) => pointer_range_action(node, fraction),
            Some(RangePointerAction::Direct(action)) => (None, Some(action)),
            None => (None, None),
        };
        self.preview = preview;
        context
            .input
            .update_scene_control(self.finger, true, self.thumb, preview);
        if let Some(handle) = &self.handle {
            handle.publish_preview(preview.map(|value| (self.thumb.unwrap_or(0), value)));
        }
        if let Some(action) = action {
            self.adjust(component, context.scene, action);
        }
    }

    fn move_to(
        &mut self,
        context: &mut TouchContext<'_>,
        component: &mut impl Component,
        point: Point,
    ) {
        if let Some((node, item)) = self
            .target
            .node_in(context.scene)
            .zip(context.layout.at_path(&self.target.path))
            && let Some(local) = item.map_from_window(point)
            && let Some(sample) = self.motion.move_to(node, item, local, true)
        {
            self.apply_sample(context, component, node, sample);
        }
    }

    fn clear_preview(&self) {
        if let Some(handle) = &self.handle {
            handle.publish_preview(None);
        }
    }

    fn end(self, context: &mut TouchContext<'_>, component: &mut impl Component, commit: bool) {
        context.input.finish_scene_control(self.finger, false);
        if let Some(handle) = &self.handle {
            handle.publish_preview(context.input.physical_range_preview(
                &self.target,
                context.scene,
                context.layout,
            ));
        }
        if commit {
            let action = self
                .preview
                .map_or(RangeAction::Release, RangeAction::CommitFraction);
            self.adjust(component, context.scene, action);
        }
    }
}

impl SceneContact {
    fn live(&self, context: &TouchContext<'_>) -> bool {
        match self {
            Self::TouchArea { sequence, .. } => {
                sequence.is_live(context.input, context.scene, context.layout)
            }
            Self::Button {
                target,
                kind,
                handle,
                ..
            } => {
                scene_control_live(target, context.scene, context.layout)
                    && !context.input.physical_button_pressed(target)
                    && target.node_in(context.scene).is_some_and(|node| {
                        node.kind == *kind && &node.button_press_handle == handle
                    })
            }
            Self::Range(drag) => {
                scene_control_live(&drag.target, context.scene, context.layout)
                    && !context.input.physical_range_pressed(
                        &drag.target,
                        context.scene,
                        context.layout,
                    )
                    && drag.target.node_in(context.scene).is_some_and(|node| {
                        node.kind == drag.kind && node.range_position_handle == drag.handle
                    })
            }
        }
    }

    fn move_to(
        &mut self,
        context: &mut TouchContext<'_>,
        component: &mut impl Component,
        source: &Target,
        point: Point,
    ) -> bool {
        match self {
            Self::TouchArea { sequence, click } => {
                let Some(delivery) = sequence.sample(
                    context.input,
                    context.scene,
                    context.layout,
                    point,
                    PointerEventKind::Move,
                    KeyModifiers::default(),
                ) else {
                    return false;
                };
                *click &= delivery.event.inside;
                delivery.apply(component, context.scene);
            }
            Self::Button { target, finger, .. } => {
                let inside = context
                    .input
                    .scene_touch_target(context.scene, context.layout, &source.path, point)
                    .as_ref()
                    == Some(target);
                context.input.sample_scene_button(
                    *finger,
                    context.scene,
                    context.layout,
                    point,
                    inside,
                );
                context
                    .input
                    .publish_button_presses(context.scene, context.layout);
            }
            Self::Range(drag) => drag.move_to(context, component, point),
        }
        true
    }

    fn cancel(self, component: &mut impl Component, context: &mut TouchContext<'_>) {
        match self {
            Self::TouchArea { sequence, .. } => {
                sequence
                    .cancel(KeyModifiers::default())
                    .apply(component, context.scene);
            }
            Self::Button { finger, .. } => {
                context.input.finish_scene_control(finger, false);
                context.input.dispatch_button_press_events(
                    component,
                    context.scene,
                    context.layout,
                );
            }
            Self::Range(drag) => drag.end(context, component, false),
        }
    }

    fn clear(self) {
        match self {
            Self::TouchArea { sequence, .. } => sequence
                .cancel(KeyModifiers::default())
                .clear_canceled_state(),
            Self::Button {
                handle: Some(handle),
                ..
            } => handle.publish(false),
            Self::Button { handle: None, .. } => {}
            Self::Range(drag) => drag.clear_preview(),
        }
    }
}

impl SceneTouch {
    fn mapped_point(&self, context: &TouchContext<'_>, position: Point2D) -> Option<Point> {
        (context.source(&self.view, &self.name).as_ref() == Some(&self.source)
            && self.capture.live(context))
        .then(|| context.map(&self.source, position, false))
        .flatten()
    }

    fn release(
        self,
        component: &mut impl Component,
        context: &mut TouchContext<'_>,
        position: Point2D,
        click: bool,
    ) {
        let Some(point) = self.mapped_point(context, position) else {
            self.capture.cancel(component, context);
            return;
        };
        match self.capture {
            SceneContact::TouchArea {
                mut sequence,
                click: eligible,
            } => {
                let Some(delivery) = sequence.sample(
                    context.input,
                    context.scene,
                    context.layout,
                    point,
                    PointerEventKind::Up,
                    KeyModifiers::default(),
                ) else {
                    sequence
                        .cancel(KeyModifiers::default())
                        .apply(component, context.scene);
                    return;
                };
                let click = click && eligible && delivery.event.inside;
                if delivery.apply(component, context.scene) && click {
                    component.dispatch_at(context.scene, &sequence.target().path, "clicked");
                }
            }
            SceneContact::Button {
                target,
                finger,
                kind,
                ..
            } => {
                let inside = context
                    .input
                    .scene_touch_target(context.scene, context.layout, &self.source.path, point)
                    .as_ref()
                    == Some(&target);
                let tap = context.input.sample_scene_button(
                    finger,
                    context.scene,
                    context.layout,
                    point,
                    inside,
                );
                let action =
                    context
                        .input
                        .scene_switch_action(finger, context.scene, context.layout);
                context.input.finish_scene_control(finger, tap);
                context.input.dispatch_button_press_events(
                    component,
                    context.scene,
                    context.layout,
                );
                if let Some(input) = action {
                    component.control_at(context.scene, &input.path, input.action);
                } else if click && tap {
                    if kind == "RadioButton"
                        && let Some(input) = context.input.select_group_option(
                            context.scene,
                            context.layout,
                            &target,
                        )
                    {
                        component.control_at(context.scene, &input.path, input.action);
                    } else {
                        component.dispatch_at(context.scene, &target.path, "clicked");
                    }
                }
            }
            SceneContact::Range(mut drag) => {
                drag.move_to(context, component, point);
                drag.end(context, component, true);
            }
        }
    }
}

impl SceneTouches {
    pub fn apply(
        &mut self,
        input: &mut Interaction,
        component: &mut impl Component,
        scene: &Node,
        layout: &LayoutNode,
        request: SceneTouchRequest,
    ) -> bool {
        let mut context = TouchContext {
            input,
            scene,
            layout,
        };
        let active = self
            .active
            .iter()
            .position(|touch| touch.view == request.view && touch.id == request.id);
        if let Some(index) = active {
            let mut touch = self.active.remove(index);
            if !request.pressed || request.source != touch.name {
                let same = request.source == touch.name;
                let position = if same { request.position } else { touch.last };
                touch.release(component, &mut context, position, same && !request.pressed);
                if !request.pressed {
                    return true;
                }
            } else {
                if let Some(point) = touch.mapped_point(&context, request.position)
                    && touch
                        .capture
                        .move_to(&mut context, component, &touch.source, point)
                {
                    touch.last = request.position;
                    self.active.insert(index, touch);
                } else {
                    touch.capture.cancel(component, &mut context);
                }
                return true;
            }
        }
        if !request.pressed || request.source.is_empty() {
            return active.is_some();
        }
        let Some((source, point)) =
            context
                .source(&request.view, &request.source)
                .and_then(|source| {
                    context
                        .map(&source, request.position, true)
                        .map(|point| (source, point))
                })
        else {
            return active.is_some();
        };
        let Some(next) = self
            .next_finger
            .checked_add(1)
            .filter(|id| *id < FIRST_SCENE_TOUCH_FINGER_ID * 2)
        else {
            return active.is_some();
        };
        let Some(target) = context
            .input
            .scene_touch_target(scene, layout, &source.path, point)
        else {
            return active.is_some();
        };
        let Some(node) = target.node_in(scene) else {
            return active.is_some();
        };
        let finger = self.next_finger;
        let capture = if scene_button(node.kind) {
            if !context.input.scene_control_available(&target) {
                return active.is_some();
            }
            let handle = node.button_press_handle.clone();
            context
                .input
                .begin_scene_control(scene, target.clone(), finger, Some((layout, point)));
            context
                .input
                .dispatch_button_press_events(component, scene, layout);
            SceneContact::Button {
                target,
                finger,
                kind: node.kind,
                handle,
            }
        } else if has_range_pointer(node) {
            if !context.input.scene_control_available(&target)
                || context.input.physical_range_pressed(&target, scene, layout)
            {
                return active.is_some();
            }
            let Some(drag) = SceneRange::begin(&mut context, component, target, finger, point)
            else {
                return active.is_some();
            };
            SceneContact::Range(drag)
        } else {
            let event = PointerEvent {
                button: PointerButton::Primary,
                is_touch: true,
                touch_finger_id: finger,
                ..Default::default()
            };
            let Some((mut sequence, delivery)) = PointerSequence::begin(
                target,
                context.input,
                scene,
                layout,
                (point, None),
                event,
                std::time::Instant::now(),
            ) else {
                return active.is_some();
            };
            let shared = node
                .pointer_handle
                .as_ref()
                .is_some_and(|handle| handle.active_touch_count() > 0);
            if shared && !node.boolean("multi-touch", false) {
                return active.is_some();
            }
            if shared {
                for active in &mut self.active {
                    if let SceneContact::TouchArea {
                        sequence: first,
                        click,
                    } = &mut active.capture
                        && first.target() == sequence.target()
                    {
                        *click = false;
                    }
                }
            }
            sequence.disable_press_and_hold();
            let click = !shared && delivery.event.inside;
            delivery.apply(component, scene);
            SceneContact::TouchArea { sequence, click }
        };
        self.next_finger = next;
        self.active.push(SceneTouch {
            view: request.view,
            id: request.id,
            name: request.source,
            source,
            last: request.position,
            capture,
        });
        true
    }

    pub fn reconcile(
        &mut self,
        input: &mut Interaction,
        component: &mut impl Component,
        scene: &Node,
        layout: &LayoutNode,
    ) -> bool {
        if self.active.is_empty() {
            return false;
        }
        let mut context = TouchContext {
            input,
            scene,
            layout,
        };
        let mut changed = false;
        let mut index = 0;
        while index < self.active.len() {
            let touch = &self.active[index];
            if context.source(&touch.view, &touch.name).as_ref() == Some(&touch.source)
                && touch.capture.live(&context)
            {
                index += 1;
            } else {
                self.active
                    .remove(index)
                    .capture
                    .cancel(component, &mut context);
                changed = true;
            }
        }
        changed
    }
}

impl Drop for SceneTouches {
    fn drop(&mut self) {
        for touch in self.active.drain(..) {
            touch.capture.clear();
        }
    }
}
