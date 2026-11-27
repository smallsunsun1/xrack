use super::{LayoutNode, Node, PaintFrame, Palette, Rect, Renderer, Role, TextMeasurer, shadow};
use std::{
    cell::OnceCell,
    collections::BTreeMap,
    rc::Rc,
    time::{Duration, Instant},
};

/// A nonempty rectangle in physical window pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DamageRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl DamageRect {
    fn from_rect(rect: Rect) -> Self {
        Self {
            x: rect.x as u32,
            y: rect.y as u32,
            width: rect.width as u32,
            height: rect.height as u32,
        }
    }

    pub(super) fn as_rect(self) -> Rect {
        Rect::new(
            self.x as f32,
            self.y as f32,
            self.width as f32,
            self.height as f32,
        )
    }
}

const MAX_REGIONS: usize = 4;

/// Disjoint rectangles whose contents can be redrawn in one paint traversal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DamageRegions {
    rects: [DamageRect; MAX_REGIONS],
    len: usize,
}

impl DamageRegions {
    pub fn rectangles(&self) -> &[DamageRect] {
        &self.rects[..self.len]
    }

    fn intersects(self, extent: Rect) -> bool {
        self.rectangles()
            .iter()
            .any(|rect| overlap(rect.as_rect(), extent))
    }

    fn extend(&mut self, other: Self) -> bool {
        other
            .rectangles()
            .iter()
            .all(|rect| self.insert(rect.as_rect()))
    }

    // Restart when a union grows. That union may reach an earlier rectangle.
    fn insert(&mut self, mut extent: Rect) -> bool {
        if !shadow::nonempty(extent) {
            return true;
        }
        let mut index = 0;
        while index < self.len {
            let region = self.rects[index].as_rect();
            if overlap(region, extent) {
                if shadow::union(extent, region) == region {
                    return true;
                }
                extent = shadow::union(extent, region);
                self.len -= 1;
                self.rects.copy_within(index + 1..=self.len, index);
                self.rects[self.len] = DamageRect::default();
                index = 0;
            } else {
                index += 1;
            }
        }
        if self.len == MAX_REGIONS {
            return false;
        }
        self.rects[self.len] = DamageRect::from_rect(extent);
        self.len += 1;
        self.rects[..self.len].sort_unstable_by_key(|rect| (rect.y, rect.x));
        true
    }
}

fn overlap(first: Rect, second: Rect) -> bool {
    shadow::nonempty(first.intersect(second))
}

/// Work performed to update the persistent window image, in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowDamage {
    Full,
    Partial {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    PartialRegions(DamageRegions),
    Unchanged,
}

impl WindowDamage {
    fn regions(self) -> Option<DamageRegions> {
        match self {
            Self::Partial {
                x,
                y,
                width,
                height,
            } => {
                let mut regions = DamageRegions::default();
                regions.rects[0] = DamageRect {
                    x,
                    y,
                    width,
                    height,
                };
                regions.len = 1;
                Some(regions)
            }
            Self::PartialRegions(regions) => Some(regions),
            Self::Full | Self::Unchanged => None,
        }
    }

    fn between(before: &Scene, after: &Scene) -> Self {
        if before.style != after.style {
            return Self::Full;
        }
        let same_geometry = Rc::ptr_eq(&before.geometry, &after.geometry);
        let mut damage = after.paint_updates;
        if !before.viewport_damage(after, &mut damage) {
            return Self::Full;
        }
        if same_geometry {
            // Input-only changes can compare sparse state on retained bounds.
            // Editors and active drop outlines can repaint at an unchanged path.
            if !before.input.iter().chain(&after.input).all(|(&index, _)| {
                !before.input_changed(index, after, index)
                    || damage.insert(after.geometry.items[index].extent)
            }) {
                return Self::Full;
            }
        } else if !damage.extend(before.paint_since_geometry)
            || !before.changed_geometry(after, &mut damage)
        {
            return Self::Full;
        }
        if damage.len == 0 {
            return Self::Unchanged;
        }
        // Bound large texture replays independently of the CPU savings
        // estimate used to choose whether an eligible plan is profitable.
        let maximum_area =
            after.geometry.index.viewport.width * after.geometry.index.viewport.height * 0.7;
        // Preserve complete overlapping paint groups and batches, including
        // their original coverage math. A group crossing two regions merges them.
        for _ in 0..4 {
            let old = damage;
            for region in old.rectangles() {
                let query = region.as_rect();
                if !after
                    .geometry
                    .index
                    .visit(query, |extent, _| damage.insert(extent))
                {
                    return Self::Full;
                }
                if !same_geometry
                    && !before
                        .geometry
                        .batches
                        .iter()
                        .filter(|&&extent| overlap(extent, query))
                        .all(|&extent| damage.insert(extent))
                {
                    return Self::Full;
                }
            }
            let area: f32 = damage
                .rectangles()
                .iter()
                .map(|r| r.width as f32 * r.height as f32)
                .sum();
            if area >= maximum_area {
                return Self::Full;
            }
            if damage == old {
                return if let [
                    DamageRect {
                        x,
                        y,
                        width,
                        height,
                    },
                ] = damage.rectangles()
                {
                    Self::Partial {
                        x: *x,
                        y: *y,
                        width: *width,
                        height: *height,
                    }
                } else {
                    Self::PartialRegions(damage)
                };
            }
        }
        Self::Full
    }
}

struct Scene {
    root: Node,
    layout: Option<Rc<LayoutNode>>,
    geometry: Rc<Geometry>,
    input: BTreeMap<usize, PaintInput>,
    active_viewports: Vec<usize>,
    style: SceneStyle,
    paint_updates: DamageRegions,
    paint_since_geometry: DamageRegions,
}

struct SceneStyle {
    theme: Palette,
    font_context: Rc<()>,
    font_style: (u16, bool, Option<f32>),
    text_scale: f32,
    generation: u64,
}

impl PartialEq for SceneStyle {
    fn eq(&self, other: &Self) -> bool {
        self.theme == other.theme
            && Rc::ptr_eq(&self.font_context, &other.font_context)
            && self.font_style == other.font_style
            && self.text_scale == other.text_scale
            && self.generation == other.generation
    }
}

impl Scene {
    fn capture(
        renderer: &Renderer,
        scene: &Node,
        layout: &LayoutNode,
        frame: PaintFrame<'_>,
        target: &wgpu::Texture,
        previous: Option<&Self>,
    ) -> Option<Self> {
        let input = frame.input;
        if scene.kind != "Window"
            || scene.opacity() != 1.0
            || !layout.local_transform().is_identity()
            || !window_uses_only_clear(scene, layout, frame.scale)
            || renderer.inspection_outline.is_some()
            || frame
                .transitions
                .is_some_and(super::StackTransitions::is_active)
            || input.overlay_layers(layout).next().is_some()
            || input.opening_drawer_preview(scene, layout).is_some()
            || input.tooltip_popup(scene, layout).is_some()
            || input.touch_selection_target().is_some()
            || input.choice_menu(scene, layout).is_some()
            || !input.menu_panels(scene, layout).is_empty()
        {
            return None;
        }
        let (_, weight, italic, size) = renderer.text.default_style();
        let style = SceneStyle {
            theme: frame.theme,
            font_context: renderer.text.context_key().clone(),
            font_style: (weight, italic, size),
            text_scale: renderer.text.text_scale(),
            generation: renderer.scene_generation,
        };
        let layout_key = frame.prepared_layout.cloned();
        let mut paint_updates = DamageRegions::default();
        let mut paint_since_geometry = DamageRegions::default();
        let can_reuse = previous
            .filter(|previous| {
                previous.style == style
                    && same_layout(previous.layout.as_ref(), layout_key.as_ref())
                    && previous.geometry.scale == frame.scale
                    && previous.geometry.index.viewport.width == target.width() as f32
                    && previous.geometry.index.viewport.height == target.height() as f32
                    && collect_background_changes(
                        &previous.root,
                        scene,
                        layout,
                        frame,
                        previous.geometry.index.viewport,
                        &mut paint_updates,
                    )
            })
            .filter(|previous| {
                paint_since_geometry = previous.paint_since_geometry;
                paint_since_geometry.extend(paint_updates)
            });
        let geometry = if let Some(previous) = can_reuse {
            previous.geometry.index.enable_grid();
            previous.geometry.clone()
        } else {
            paint_updates = DamageRegions::default();
            paint_since_geometry = DamageRegions::default();
            let viewport = Rect::new(0.0, 0.0, target.width() as f32, target.height() as f32);
            let mut geometry = Geometry::new(viewport, frame.scale);
            geometry.capture_item(scene, layout, &mut Vec::new(), frame)?;
            xrack_runtime::update_profile::build_window_geometry();
            Rc::new(geometry)
        };
        Some(Self {
            root: scene.clone(),
            layout: layout_key,
            input: geometry.paint_input(frame),
            active_viewports: geometry.active_viewports(scene, layout, frame),
            geometry,
            style,
            paint_updates,
            paint_since_geometry,
        })
    }

    fn viewport_damage(&self, after: &Self, damage: &mut DamageRegions) -> bool {
        let old = &self.geometry.islands;
        let new = &after.geometry.islands;
        if old.len() != new.len() {
            return false;
        }
        let same_layout = same_layout(self.layout.as_ref(), after.layout.as_ref());
        old.iter()
            .zip(new)
            .enumerate()
            .all(|(position, (&old, &new))| {
                let before = &self.geometry.items[old];
                let current = &after.geometry.items[new];
                if before.path != current.path {
                    return false;
                }
                let active = self.active_viewports.binary_search(&position).is_ok()
                    || after.active_viewports.binary_search(&position).is_ok();
                // Scrolling changes the viewport's placement even when
                // its large scene subtree is mostly unchanged.
                let unchanged = !active
                    && (same_layout
                        || self.layout.as_ref().zip(after.layout.as_ref()).is_some_and(
                            |(old, new)| old.at_path(&before.path) == new.at_path(&current.path),
                        ))
                    && (before.node.shares_storage_with(&current.node)
                        || before.node == current.node);
                unchanged || damage.insert(before.extent) && damage.insert(current.extent)
            })
    }

    fn input_changed(&self, old: usize, after: &Self, new: usize) -> bool {
        let old = self.input.get(&old).copied().unwrap_or_default();
        let new = after.input.get(&new).copied().unwrap_or_default();
        old.editing || new.editing || old.drop_target || new.drop_target || old != new
    }

    fn changed_geometry(&self, after: &Self, damage: &mut DamageRegions) -> bool {
        let old = &self.geometry.items;
        let new = &after.geometry.items;
        let same = |i: usize, j: usize| old[i].same(&new[j]) && !self.input_changed(i, after, j);
        let first = (0..old.len().min(new.len()))
            .take_while(|&i| same(i, i))
            .count();
        if old.len() == new.len() {
            for i in first..old.len() {
                if !same(i, i) && (!damage.insert(old[i].extent) || !damage.insert(new[i].extent)) {
                    return false;
                }
            }
        } else {
            let suffix = (0..old.len().min(new.len()) - first)
                .take_while(|&i| same(old.len() - 1 - i, new.len() - 1 - i))
                .count();
            if !old[first..old.len() - suffix]
                .iter()
                .chain(&new[first..new.len() - suffix])
                .all(|item| damage.insert(item.extent))
            {
                return false;
            }
        }
        let old = &self.geometry.batches;
        let new = &after.geometry.batches;
        let shared = old.len().min(new.len());
        old.iter()
            .zip(new)
            .all(|(&old, &new)| old == new || damage.insert(old) && damage.insert(new))
            && old[shared..]
                .iter()
                .chain(&new[shared..])
                .all(|&extent| damage.insert(extent))
    }
}

fn collect_background_changes(
    before: &Node,
    after: &Node,
    layout: &LayoutNode,
    frame: PaintFrame<'_>,
    viewport: Rect,
    damage: &mut DamageRegions,
) -> bool {
    if before.shares_storage_with(after) {
        return true;
    }
    if is_paint_viewport(before.kind)
        || before.children.len() != after.children.len()
        || after.children.len() != layout.children.len()
    {
        return false;
    }
    if !before.same_own_paint_inputs(after) {
        let visible_fill = |node| {
            super::rectangle::Solid::from_node(node, layout, frame.scale)
                .is_some_and(|rectangle| rectangle.visible())
        };
        let opaque = |node: &Node| {
            node.color("background")
                .is_some_and(|color| color.0 & 0xff == 0xff)
        };
        if after.kind != "Rectangle"
            || !after.children.is_empty()
            || !before.same_own_paint_except_background(after)
            || opaque(before) != opaque(after)
            || !visible_fill(before)
            || !visible_fill(after)
        {
            return false;
        }
        let bounds = paint_extent(after, layout, frame);
        return damage
            .insert(device_extent(after, layout, bounds, frame.scale).intersect(viewport));
    }
    before
        .children
        .iter()
        .zip(&after.children)
        .zip(&layout.children)
        .all(|((before, after), layout)| {
            collect_background_changes(before, after, layout, frame, viewport, damage)
        })
}

fn same_layout(old: Option<&Rc<LayoutNode>>, new: Option<&Rc<LayoutNode>>) -> bool {
    old.zip(new).is_some_and(|(old, new)| Rc::ptr_eq(old, new))
}

struct Geometry {
    items: Vec<Item>,
    batches: Vec<Rect>,
    paths: BTreeMap<Vec<usize>, usize>,
    // Text state and synthetic button presses can change without a pointer event.
    independent: Vec<usize>,
    islands: Vec<usize>,
    index: PaintIndex,
    scale: f32,
}

impl Geometry {
    fn new(viewport: Rect, scale: f32) -> Self {
        Self {
            items: Vec::new(),
            batches: Vec::new(),
            paths: BTreeMap::new(),
            independent: Vec::new(),
            islands: Vec::new(),
            index: PaintIndex::new(viewport),
            scale,
        }
    }

    fn active_viewports(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        frame: PaintFrame<'_>,
    ) -> Vec<usize> {
        if self.islands.is_empty() {
            return Vec::new();
        }
        let input = frame.input;
        if input.scroll_motion_active()
            || input.scroll_bar_captured()
            || input.table_drag_selection_armed()
            || input.table_column_move_claimed()
            || input.table_row_move_claimed()
            || input.table_resize_drag(scene, layout).is_some()
            || input.table_row_resize_drag(scene, layout).is_some()
        {
            return (0..self.islands.len()).collect();
        }
        self.islands
            .iter()
            .enumerate()
            .filter_map(|(position, &index)| {
                let item = &self.items[index];
                let active = input
                    .window_paint_targets()
                    .any(|path| path.starts_with(item.path.as_slice()))
                    || input.pointer_position().is_some_and(|point| {
                        item.extent.contains(super::Point {
                            x: point.x * self.scale,
                            y: point.y * self.scale,
                        })
                    })
                    || [
                        xrack_runtime::Orientation::Horizontal,
                        xrack_runtime::Orientation::Vertical,
                    ]
                    .into_iter()
                    .any(|axis| input.scroll_bar_highlighted(&item.path, axis));
                active.then_some(position)
            })
            .collect()
    }

    fn paint_input(&self, frame: PaintFrame<'_>) -> BTreeMap<usize, PaintInput> {
        let mut states = BTreeMap::new();
        let targeted = frame
            .input
            .window_paint_targets()
            .filter_map(|path| self.paths.get(path));
        for &index in targeted.chain(&self.independent) {
            let item = &self.items[index];
            let state = PaintInput::capture(&item.node, &item.path, frame);
            if state != PaintInput::default() {
                states.insert(index, state);
            }
        }
        states
    }

    fn record_batch(&mut self, run: Option<Rect>) {
        if let Some(run) = run {
            self.batches.push(run);
            self.index.add(run, 0);
        }
    }

    fn capture_item(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        frame: PaintFrame<'_>,
    ) -> Option<Batchable> {
        if !layout.visible
            || node.opacity() == 0.0
            || node.kind == "Shortcut"
            || node.scene_texture_only()
        {
            return Some(Batchable::Primitives(None));
        }
        let viewport = is_paint_viewport(node.kind);
        let island_work = if viewport {
            frame.prepared_layout?;
            // A frozen viewport can be skipped only if all its visible content
            // has bounded, non-autonomous paint.
            if layout.allows_overflow() {
                return None;
            }
            Some(static_viewport_work(node, layout)?)
        } else {
            if !basic_painter(node.kind)
                || layout.scroll.is_some()
                || layout.table_pane_clip().is_some()
            {
                return None;
            }
            None
        };
        let extent = paint_extent(node, layout, frame);
        let bounds = layout.bounds;
        if ![
            extent.x,
            extent.y,
            extent.width,
            extent.height,
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
        ]
        .into_iter()
        .all(f32::is_finite)
        {
            return None;
        }
        let extent = if shadow::nonempty(extent) {
            device_extent(node, layout, extent, frame.scale).intersect(self.index.viewport)
        } else {
            Rect::default()
        };
        let button = xrack_runtime::is_button(node.kind);
        let editor = xrack_runtime::is_text_input(node.kind);
        let styled = node.kind == "StyledText";
        let drop_area = node.kind == "DropArea";
        let tracks_input = button
            || editor
            || styled
            || drop_area
            || matches!(node.kind, "FocusScope" | "SplitHandle" | "Pane");
        let index = self.items.len();
        self.items.push(Item {
            node: node.clone(),
            path: if viewport || tracks_input {
                path.clone()
            } else {
                Vec::new()
            },
            depth: path.len(),
            bounds,
            extent,
            clip: layout.clip,
            enabled: layout.enabled,
            direction: layout.layout_direction,
            direction_override: layout.layout_direction_override(),
            transform: layout.world_transform().coefficients(),
            overflow: layout.allows_overflow(),
        });
        let paintless = paintless_container(node);
        if !path.is_empty() && !paintless && !can_clip_surface(node, layout, frame) {
            self.index.add(
                extent,
                island_work.unwrap_or_else(|| paint_weight(node, button, editor)),
            );
        }
        if viewport {
            self.islands.push(index);
            return Some(Batchable::Barrier);
        }
        if tracks_input {
            self.paths.insert(path.clone(), index);
        }
        if editor || styled || drop_area || button && node.button_press_handle.is_some() {
            self.independent.push(index);
        }
        let mut run = None;
        let mut has_barrier = false;
        for (index, child, item) in layout.children_in_paint_order(node) {
            path.push(index);
            let batchable = self.capture_item(child, item, path, frame)?;
            path.pop();
            match batchable {
                Batchable::Primitives(Some(bounds)) => {
                    run = Some(run.map_or(bounds, |rect| shadow::union(rect, bounds)));
                }
                Batchable::Primitives(None) => {}
                Batchable::Barrier => {
                    has_barrier = true;
                    self.record_batch(run.take());
                }
            }
        }
        self.record_batch(run);
        // Retain a superset of each paint run so culling never changes its atlas
        // or glyph batch membership.
        let batchable = if paintless
            && !has_barrier
            && layout.child_clip_shape().is_none()
            && Renderer::is_unpainted_group(node, layout)
        {
            Batchable::Primitives(run)
        } else if matches!(node.kind, "Text" | "Label" | "Image")
            || node.kind == "Rectangle"
                && super::rectangle::Solid::from_node(node, layout, frame.scale).is_some()
        {
            Batchable::Primitives(shadow::nonempty(extent).then_some(extent))
        } else {
            Batchable::Barrier
        };
        Some(batchable)
    }
}

pub(super) fn is_paint_viewport(kind: &str) -> bool {
    matches!(
        kind,
        "ScrollView"
            | "ListView"
            | "StandardListView"
            | "GridView"
            | "TableView"
            | "StandardTableView"
            | "TreeView"
    )
}

fn basic_painter(kind: &str) -> bool {
    matches!(
        kind,
        "Window"
            | "Empty"
            | "Rectangle"
            | "Text"
            | "Label"
            | "StyledText"
            | "Image"
            | "Path"
            | "Pane"
            | "ButtonContent"
            | "DropArea"
            | "FocusScope"
            | "Row"
            | "HorizontalLayout"
            | "VerticalLayout"
            | "VerticalBox"
            | "HorizontalBox"
            | "FlexboxLayout"
            | "FlowLayout"
            | "GridLayout"
            | "Button"
            | "ToolButton"
            | "RoundButton"
            | "SplitView"
            | "SplitHandle"
    ) || xrack_runtime::is_text_input(kind)
}

fn static_viewport_work(node: &Node, layout: &LayoutNode) -> Option<u32> {
    if !layout.visible || node.opacity() == 0.0 {
        return Some(0);
    }
    if node.kind == "DropArea"
        || node.boolean("pull-to-refresh", false)
        || node.button_press_handle.is_some()
        || xrack_runtime::is_text_input(node.kind)
        || !layout.list_section_headers().is_empty()
        || !(basic_painter(node.kind)
            || is_paint_viewport(node.kind)
            || matches!(
                node.kind,
                "ListRow"
                    | "StandardListItem"
                    | "ItemDelegate"
                    | "TableRow"
                    | "TableCell"
                    | "TableColumn"
                    | "TreeRow"
                    | "TreeCell"
                    | "ScrollBar"
                    | "ScrollBarThumb"
                    | "ScrollBarTrack"
            ))
    {
        return None;
    }
    // An empty content scissor skips the subtree's paint work. Keep the
    // viewport's painter eligibility checks independent of that estimate.
    if !layout.allows_overflow()
        && super::is_clipped_out(layout.bounds, layout.clip)
        && !node
            .properties
            .keys()
            .any(|name| name.starts_with("drop-shadow-"))
    {
        return Some(0);
    }
    let weight = paint_weight(node, xrack_runtime::is_button(node.kind), false);
    layout
        .children_in_paint_order(node)
        .try_fold(weight, |total, (_, node, layout)| {
            static_viewport_work(node, layout).map(|work| total.saturating_add(work))
        })
}

enum Batchable {
    Barrier,
    Primitives(Option<Rect>),
}

struct Item {
    node: Node,
    path: Vec<usize>,
    depth: usize,
    bounds: Rect,
    extent: Rect,
    clip: Rect,
    enabled: bool,
    direction: xrack_runtime::LayoutDirection,
    direction_override: Option<xrack_runtime::LayoutDirection>,
    transform: [f32; 6],
    overflow: bool,
}

impl Item {
    fn same(&self, other: &Self) -> bool {
        self.depth == other.depth
            && self.bounds == other.bounds
            && self.extent == other.extent
            && self.clip == other.clip
            && self.enabled == other.enabled
            && self.direction == other.direction
            && self.direction_override == other.direction_override
            && self.transform == other.transform
            && self.overflow == other.overflow
            && self.node.same_own_paint_inputs(&other.node)
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct PaintInput {
    hovered: bool,
    pressed: bool,
    focused: bool,
    editing: bool,
    focused_link: Option<usize>,
    drop_target: bool,
}

impl PaintInput {
    fn capture(node: &Node, path: &[usize], frame: PaintFrame<'_>) -> Self {
        let button = xrack_runtime::is_button(node.kind);
        let editor = xrack_runtime::is_text_input(node.kind);
        let split_handle = node.kind == "SplitHandle";
        let styled = node.kind == "StyledText";
        let input = frame.input;
        Self {
            hovered: (button || split_handle) && input.is_hovered(path),
            pressed: button
                && (input.is_pressed(path)
                    || node
                        .button_press_handle
                        .as_ref()
                        .is_some_and(xrack_runtime::ButtonPressHandle::is_synthetic_pressed)),
            focused: (editor
                || button
                || split_handle
                || matches!(node.kind, "FocusScope" | "Pane"))
                && input.is_focused(path),
            editing: (editor || styled)
                && (input.editor_active(path) || input.displayed_editor(node, path).is_some()),
            focused_link: if styled {
                input
                    .focused
                    .as_ref()
                    .filter(|target| target.path == path)
                    .and_then(xrack_layout::Target::text_link_index)
            } else {
                None
            },
            drop_target: node.kind == "DropArea" && input.is_drop_target(node, path),
        }
    }
}

const BANDS: usize = 8;

/// Physical bounds indexed by coarse viewport cells. Bounds crossing cells
/// appear in more than one bucket but are visited only once for each query.
struct PaintIndex {
    viewport: Rect,
    entries: Vec<(Rect, u32)>,
    cells: OnceCell<[Vec<usize>; BANDS * BANDS]>,
    work: u64,
}

impl PaintIndex {
    fn new(viewport: Rect) -> Self {
        Self {
            viewport,
            entries: Vec::new(),
            cells: OnceCell::new(),
            work: 0,
        }
    }

    fn cell(&self, x: f32, y: f32) -> (usize, usize) {
        let band = |coordinate: f32, extent: f32| {
            ((coordinate * BANDS as f32 / extent) as usize).min(BANDS - 1)
        };
        (band(x, self.viewport.width), band(y, self.viewport.height))
    }

    fn add(&mut self, extent: Rect, work: u32) {
        if shadow::nonempty(extent) {
            self.entries.push((extent, work));
            self.work += u64::from(work);
        }
    }

    fn enable_grid(&self) {
        // Small or one-off layouts are cheaper to scan than to index.
        if self.entries.len() < BANDS * BANDS {
            return;
        }
        self.cells.get_or_init(|| {
            let mut cells: [Vec<usize>; BANDS * BANDS] = std::array::from_fn(|_| Vec::new());
            for (index, &(extent, _)) in self.entries.iter().enumerate() {
                let (x0, y0) = self.cell(extent.x, extent.y);
                let (x1, y1) = self.cell(
                    extent.x + extent.width - 1.0,
                    extent.y + extent.height - 1.0,
                );
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        cells[y * BANDS + x].push(index);
                    }
                }
            }
            cells
        });
    }

    fn visit(&self, region: Rect, mut visit: impl FnMut(Rect, u32) -> bool) -> bool {
        let Some(cells) = self.cells.get() else {
            return self
                .entries
                .iter()
                .all(|&(extent, work)| !overlap(region, extent) || visit(extent, work));
        };
        let (x0, y0) = self.cell(region.x, region.y);
        let (x1, y1) = self.cell(
            region.x + region.width - 1.0,
            region.y + region.height - 1.0,
        );
        for y in y0..=y1 {
            for x in x0..=x1 {
                for &index in &cells[y * BANDS + x] {
                    let (extent, work) = self.entries[index];
                    let intersection = region.intersect(extent);
                    if shadow::nonempty(intersection)
                        && self.cell(intersection.x, intersection.y) == (x, y)
                        && !visit(extent, work)
                    {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn paint_work(&self, regions: Option<DamageRegions>) -> (u64, u64) {
        let mut dirty = 0;
        if let Some(regions) = regions {
            let regions = regions.rectangles();
            for (index, region) in regions.iter().enumerate() {
                self.visit(region.as_rect(), |extent, work| {
                    if !regions[..index]
                        .iter()
                        .any(|region| overlap(region.as_rect(), extent))
                    {
                        dirty += u64::from(work);
                    }
                    true
                });
            }
        }
        (self.work, dirty)
    }
}

fn paint_weight(node: &Node, button: bool, editor: bool) -> u32 {
    let text = || {
        node.text("text")
            .map_or(0, |text| text.len().min(1024).div_ceil(8) as u32)
    };
    let content = match node.kind {
        "Text" | "Label" => 3 + text(),
        "StyledText" => {
            3 + node.styled_text("text").map_or(0, |document| {
                document.text().len().min(1024).div_ceil(8) as u32
            })
        }
        "Image" => 4,
        _ if button || editor => 8 + text(),
        _ => 1,
    };
    let shadow = node.properties.contains_key("drop-shadow-color");
    content + if shadow { 8 } else { 0 }
}

fn device_extent(node: &Node, layout: &LayoutNode, bounds: Rect, scale: f32) -> Rect {
    let raster = super::retained_layers::device_bounds(bounds, scale);
    let grouped = is_paint_viewport(node.kind)
        || matches!(
            node.kind,
            "Pane" | "Button" | "ToolButton" | "RoundButton" | "SplitHandle"
        )
        || node.kind == "Rectangle"
            && (!node.children.is_empty()
                || super::rectangle::Solid::from_node(node, layout, scale)
                    .is_some_and(|rectangle| !rectangle.uses_mask()));
    if grouped
        && node.opacity() == 1.0
        && !node.boolean("cache-rendering-hint", false)
        && !layout.allows_overflow()
        && layout.world_transform().is_identity()
        && !layout.has_ancestor_clip_chain()
        && layout.table_pane_clip().is_none()
        && !node
            .properties
            .keys()
            .any(|name| name.starts_with("drop-shadow-"))
        && super::pixel_aligned(bounds, scale)
    {
        // These painters clip their whole group with an axis-aligned scissor.
        // At integral edges the first outside pixel has zero coverage.
        raster
    } else {
        // Cover antialiasing and quantization in the general case.
        raster.inset(-2.0)
    }
}

fn has_edge_effect(node: &Node) -> bool {
    node.length("border-width").is_some_and(|width| width > 0.0)
        || node
            .properties
            .keys()
            .any(|name| name.starts_with("drop-shadow-") || name.starts_with("inner-shadow-"))
}

fn window_uses_only_clear(node: &Node, layout: &LayoutNode, scale: f32) -> bool {
    !has_edge_effect(node)
        && (!node.properties.contains_key("background")
            || super::window_clear_background(node, layout, layout.bounds, scale).is_some())
}

fn paintless_container(node: &Node) -> bool {
    matches!(
        node.kind,
        "Rectangle"
            | "Row"
            | "HorizontalLayout"
            | "VerticalLayout"
            | "HorizontalBox"
            | "VerticalBox"
            | "FlexboxLayout"
            | "FlowLayout"
            | "GridLayout"
            | "SplitView"
            | "ButtonContent"
            | "DropArea"
    ) && node.opacity() == 1.0
        && !node.properties.contains_key("background")
        && !has_edge_effect(node)
}

/// Opaque square container surfaces can be replayed through the damage scissor.
pub(super) fn can_clip_surface(node: &Node, layout: &LayoutNode, frame: PaintFrame<'_>) -> bool {
    if !matches!(node.kind, "Rectangle" | "Pane")
        || node.children.is_empty()
        || node.opacity() != 1.0
        || node.boolean("cache-rendering-hint", false)
        || layout.allows_overflow()
        || !layout.world_transform().is_identity()
        || layout.has_ancestor_clip_chain()
        || layout.table_pane_clip().is_some()
        || has_edge_effect(node)
        || xrack_layout::corner_radii(node, layout.bounds) != [0.0; 4]
        || !super::pixel_aligned(layout.bounds.intersect(layout.clip), frame.scale)
    {
        return false;
    }
    match node.properties.get("background") {
        Some(xrack_runtime::Value::Color(color))
        | Some(xrack_runtime::Value::Brush(xrack_runtime::Brush::Solid(color))) => {
            color.0 & 0xff == 0xff
        }
        None if node.kind == "Pane" => frame.theme.role(Role::Surface).0 & 0xff == 0xff,
        _ => false,
    }
}

fn paint_extent(node: &Node, layout: &LayoutNode, frame: PaintFrame<'_>) -> Rect {
    if layout.allows_overflow() {
        return layout.window_bounds(layout.clip);
    }
    let bounds = if node
        .properties
        .keys()
        .any(|name| name.starts_with("drop-shadow-"))
    {
        let scale = frame.scale * layout.world_transform().max_scale();
        shadow::OuterShadow::for_node(node, layout.bounds, scale, frame.theme.role(Role::Border))
            .map_or(layout.bounds, |shadow| {
                shadow::union(layout.bounds, shadow.paint_bounds())
            })
    } else {
        layout.bounds
    };
    // Overflowing children are still inside this parent's paint group.
    // The clip is in the node's local coordinate space.
    layout.window_bounds(layout.clip.intersect(bounds))
}

pub(super) fn intersects_damage(
    node: &Node,
    layout: &LayoutNode,
    frame: PaintFrame<'_>,
    damage: DamageRegions,
) -> bool {
    let extent = paint_extent(node, layout, frame);
    shadow::nonempty(extent) && damage.intersects(device_extent(node, layout, extent, frame.scale))
}

struct WindowBacking {
    texture: wgpu::Texture,
    scale: f32,
    valid: bool,
    blitter: Option<WindowBlitter>,
}

struct WindowBlitter {
    pipeline: wgpu::RenderPipeline,
    source: wgpu::BindGroup,
}

impl WindowBlitter {
    fn new(device: &wgpu::Device, source: &wgpu::Texture) -> Self {
        let format = source.format();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("XRack window image input"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("XRack window image transfer"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("XRack window image transfer"),
            source: wgpu::ShaderSource::Wgsl(include_str!("pixel_blit.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("XRack window image transfer"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let view = source.create_view(&Default::default());
        let source = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack window image input"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        Self { pipeline, source }
    }

    fn copy(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::Texture) {
        let view = target.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("XRack transfer window image"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    // The full-screen triangle replaces the previous image.
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.source, &[]);
        pass.draw(0..3, 0..1);
    }
}

impl WindowBacking {
    fn new(renderer: &Renderer, target: &wgpu::Texture, scale: f32) -> Option<Self> {
        let format = target.format();
        let usage = wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC;
        if !renderer
            .adapter
            .get_texture_format_features(format)
            .allowed_usages
            .contains(usage)
            || !matches!(
                format.sample_type(None, Some(renderer.device.features())),
                Some(wgpu::TextureSampleType::Float { .. })
            )
        {
            return None;
        }
        let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("XRack persistent window image"),
            size: target.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        });
        Some(Self {
            texture,
            scale,
            valid: false,
            blitter: None,
        })
    }

    fn matches(&self, target: &wgpu::Texture, scale: f32) -> bool {
        self.texture.size() == target.size()
            && self.texture.format() == target.format()
            && self.scale == scale
    }

    fn encode_transfer(
        &mut self,
        renderer: &Renderer,
        target: &wgpu::Texture,
    ) -> wgpu::CommandBuffer {
        let mut encoder = renderer
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("XRack transfer window image"),
            });
        if target.usage().contains(wgpu::TextureUsages::COPY_DST) {
            encoder.copy_texture_to_texture(
                self.texture.as_image_copy(),
                target.as_image_copy(),
                target.size(),
            );
        } else {
            self.blitter
                .get_or_insert_with(|| WindowBlitter::new(&renderer.device, &self.texture))
                .copy(&mut encoder, target);
        }
        encoder.finish()
    }
}

const SAMPLE_INTERVAL: u8 = 8;
const RETRY_INTERVAL: u8 = 16;
const MAX_RETRY_INTERVAL: u8 = 128;
const PRIME_AFTER: u8 = 2;
const MIN_SKIPPED_WORK: u64 = 64;

#[derive(Default)]
pub(super) struct WindowDamageState {
    forced: bool,
    backing: Option<WindowBacking>,
    previous: Option<Scene>,
    direct: DirectPaintCost,
    small_frames: u8,
    retry_in: u8,
    retry_interval: u8,
    last_analysis_cost: Option<Duration>,
}

#[derive(Default)]
struct DirectPaintCost {
    recent: [Option<Duration>; 2],
    typical: Duration,
    work: Option<u64>,
    sample_in: u8,
}

impl DirectPaintCost {
    fn start_sample(&mut self) -> Option<Instant> {
        if self.sample_in > 0 {
            self.sample_in -= 1;
            return None;
        }
        self.sample_in = SAMPLE_INTERVAL - 1;
        Some(Instant::now())
    }

    fn observe(&mut self, cost: Duration) {
        // Require repeated expensive direct frames. Startup and a preempted
        // frame must not make an otherwise cheap window allocate a backing.
        self.typical = match self.recent {
            [Some(oldest), Some(last)] => cost.min(oldest).min(last),
            _ => Duration::ZERO,
        };
        self.recent = [self.recent[1], Some(cost)];
        self.work = None;
    }
}

impl WindowDamageState {
    fn invalidate(&mut self) {
        if let Some(backing) = &mut self.backing {
            backing.valid = false;
        }
        self.small_frames = 0;
    }

    fn defer_analysis(&mut self) {
        self.invalidate();
        if !self.forced {
            self.previous = None;
            self.last_analysis_cost = None;
            self.retry_in = self.retry_interval.max(RETRY_INTERVAL);
            self.retry_interval = self.retry_in.saturating_mul(2).min(MAX_RETRY_INTERVAL);
        }
    }

    fn can_analyze(&mut self, transfer_budget: Duration) -> bool {
        if self.forced {
            return true;
        }
        if self.retry_in > 0 {
            self.retry_in -= 1;
            return false;
        }
        self.direct.typical >= transfer_budget.mul_f32(1.5)
    }

    fn profitable(
        &mut self,
        scene: &Scene,
        damage: Option<DamageRegions>,
        transfer_budget: Duration,
        analysis_start: Instant,
    ) -> bool {
        if self.forced {
            return true;
        }
        let (full, dirty) = scene.geometry.index.paint_work(damage);
        let skipped = full.saturating_sub(dirty);
        // Timing alone can overestimate the cost of a small window when the
        // UI thread is busy or the driver is still warming its pipelines.
        if skipped < MIN_SKIPPED_WORK {
            return false;
        }
        let reference = *self.direct.work.get_or_insert(full.max(1));
        let ratio = (skipped as f64 / reference as f64).min(1.0);
        let saved = self.direct.typical.mul_f64(ratio);
        let analysis = analysis_start.elapsed();
        let typical_analysis = self
            .last_analysis_cost
            .replace(analysis)
            .map_or(analysis, |last| last.min(analysis));
        let budget = transfer_budget + typical_analysis;
        // A new backing is still primed only after consecutive
        // profitable plans; an existing backing uses the same estimate.
        saved >= budget
    }

    fn direct_paint(
        &mut self,
        renderer: &mut Renderer,
        scene: &Node,
        layout: &LayoutNode,
        target: &wgpu::Texture,
        frame: PaintFrame<'_>,
    ) -> Result<WindowDamage, String> {
        let start = (!self.forced).then(|| self.direct.start_sample()).flatten();
        renderer.paint_frame(scene, layout, target, frame)?;
        if let Some(start) = start {
            self.direct.observe(start.elapsed());
        }
        Ok(WindowDamage::Full)
    }

    fn paint(
        &mut self,
        renderer: &mut Renderer,
        scene: &Node,
        layout: &LayoutNode,
        target: &wgpu::Texture,
        frame: PaintFrame<'_>,
        transfer_budget: Duration,
    ) -> Result<WindowDamage, String> {
        if self
            .backing
            .as_ref()
            .is_some_and(|backing| !backing.matches(target, frame.scale))
        {
            self.backing = None;
            self.previous = None;
            self.defer_analysis();
            return self.direct_paint(renderer, scene, layout, target, frame);
        }
        let analysis_start = Instant::now();
        renderer.text.set_window(scene);
        let window = Rect::new(
            0.0,
            0.0,
            target.width() as f32 / frame.scale,
            target.height() as f32 / frame.scale,
        );
        let current = (window == layout.bounds)
            .then(|| {
                Scene::capture(
                    renderer,
                    scene,
                    layout,
                    frame,
                    target,
                    self.previous.as_ref(),
                )
            })
            .flatten();
        let Some(current) = current else {
            self.previous = None;
            self.defer_analysis();
            return self.direct_paint(renderer, scene, layout, target, frame);
        };

        let planned = self
            .previous
            .as_ref()
            .map(|previous| WindowDamage::between(previous, &current));
        if planned == Some(WindowDamage::Full) {
            if self.forced {
                self.previous = Some(current);
            }
            self.defer_analysis();
            return self.direct_paint(renderer, scene, layout, target, frame);
        }
        if planned.is_none() && !self.forced {
            // Establish the scene baseline with a direct frame. The first
            // snapshot does not offer evidence that future damage stays local.
            self.previous = Some(current);
            return self.direct_paint(renderer, scene, layout, target, frame);
        }
        let damage = planned.and_then(WindowDamage::regions);
        if !self.profitable(&current, damage, transfer_budget, analysis_start) {
            self.defer_analysis();
            return self.direct_paint(renderer, scene, layout, target, frame);
        }
        let valid = self.backing.as_ref().is_some_and(|backing| backing.valid);
        if !valid && !self.forced {
            self.small_frames += 1;
            if self.small_frames < PRIME_AFTER {
                self.previous = Some(current);
                return self.direct_paint(renderer, scene, layout, target, frame);
            }
        }
        let backing = self
            .backing
            .take()
            .or_else(|| WindowBacking::new(renderer, target, frame.scale));
        let Some(mut backing) = backing else {
            self.defer_analysis();
            return self.direct_paint(renderer, scene, layout, target, frame);
        };
        let transfer = backing.encode_transfer(renderer, target);
        let work = if valid {
            planned.unwrap_or(WindowDamage::Full)
        } else {
            WindowDamage::Full
        };
        if work == WindowDamage::Unchanged {
            if renderer.geometry_cache.enabled || renderer.inspect_scene {
                renderer.scene_changes = renderer
                    .render_scene
                    .synchronize(scene, renderer.scene_generation);
            }
            xrack_runtime::update_profile::submit_gpu();
            xrack_runtime::update_profile::transfer_window_image();
            renderer.queue.submit([transfer]);
        } else {
            renderer.paint_frame_with_transfer(
                scene,
                layout,
                &backing.texture,
                PaintFrame {
                    damage: if valid { damage } else { None },
                    ..frame
                },
                Some(transfer),
            )?;
        }
        backing.valid = true;
        self.retry_interval = RETRY_INTERVAL;
        self.backing = Some(backing);
        self.previous = Some(current);
        Ok(work)
    }
}

impl Renderer {
    /// Bypass the adaptive cost policy for comparison and diagnostics.
    /// All safety checks and direct full-frame fallbacks still apply.
    #[doc(hidden)]
    pub fn set_window_damage_forced(&mut self, forced: bool) {
        if forced != self.window_damage.forced {
            self.window_damage = WindowDamageState {
                forced,
                ..Default::default()
            };
        }
    }

    pub(super) fn paint_window(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        target: &wgpu::Texture,
        frame: PaintFrame<'_>,
    ) -> Result<WindowDamage, String> {
        // Require a conservative savings margin for each window size.
        // CPU savings are the signal so the UI thread never waits for GPU timing.
        let pixels = u64::from(target.width()) * u64::from(target.height());
        let transfer_budget = Duration::from_micros(150 + pixels.div_ceil(32_000));
        if !self.window_damage.can_analyze(transfer_budget) {
            let state = &mut self.window_damage;
            state.previous = None;
            state.invalidate();
            if state.direct.typical < transfer_budget {
                state.backing = None;
            }
            let start = state.direct.start_sample();
            self.paint_frame(scene, layout, target, frame)?;
            if let Some(start) = start {
                self.window_damage.direct.observe(start.elapsed());
            }
            return Ok(WindowDamage::Full);
        }
        let mut damage = std::mem::take(&mut self.window_damage);
        let result = damage.paint(self, scene, layout, target, frame, transfer_budget);
        if result.is_err() {
            damage.previous = None;
            damage.invalidate();
        }
        self.window_damage = damage;
        result
    }
}
