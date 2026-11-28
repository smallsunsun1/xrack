use crate::{LayoutNode, Node, PaintFrame, Palette, Rect, Renderer, Role, TextMeasurer, shadow};
use std::{
    cell::OnceCell,
    collections::BTreeMap,
    rc::Rc,
    time::{Duration, Instant},
};
use xrack_layout::item::{ButtonStyle, ContainerPaint, ItemPaint, paint as item_paint};

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

    pub(crate) fn as_rect(self) -> Rect {
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

    fn extend_or_union(&mut self, other: Self) {
        if self.extend(other) {
            return;
        }
        let extent = self
            .rectangles()
            .iter()
            .chain(other.rectangles())
            .map(|rect| rect.as_rect())
            .reduce(shadow::union)
            .unwrap_or_default();
        *self = Self::default();
        self.insert(extent);
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

struct SolidUpdate {
    region: DamageRect,
    pixels: SolidPixels,
}

enum SolidPixels {
    Single([u8; 4]),
    Pieces(Vec<(DamageRect, [u8; 4])>),
}

fn split_uncovered(piece: Rect, covered: Rect, rest: &mut Vec<Rect>) {
    let right = covered.x + covered.width;
    let bottom = covered.y + covered.height;
    let pieces = [
        Rect::new(piece.x, piece.y, piece.width, covered.y - piece.y),
        Rect::new(
            piece.x,
            bottom,
            piece.width,
            piece.y + piece.height - bottom,
        ),
        Rect::new(piece.x, covered.y, covered.x - piece.x, covered.height),
        Rect::new(
            right,
            covered.y,
            piece.x + piece.width - right,
            covered.height,
        ),
    ];
    rest.extend(pieces.into_iter().filter(|&piece| shadow::nonempty(piece)));
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
        let previous_batches: &[Rect] = if same_geometry {
            &[]
        } else {
            &before.geometry.batches
        };
        Self::resolve_damage(damage, &after.geometry, previous_batches)
    }

    fn resolve_damage(
        mut damage: DamageRegions,
        geometry: &Geometry,
        previous_batches: &[Rect],
    ) -> Self {
        if damage.len == 0 {
            return Self::Unchanged;
        }
        // Bound large texture replays independently of the CPU savings
        // estimate used to choose whether an eligible plan is profitable.
        let maximum_area = geometry.index.viewport.width * geometry.index.viewport.height * 0.7;
        // Preserve complete overlapping paint groups and batches, including
        // their original coverage math. A group crossing two regions merges them.
        for _ in 0..4 {
            let old = damage;
            for region in old.rectangles() {
                let query = region.as_rect();
                if !geometry.index.visit(query, |entry, extent, _| {
                    geometry.replay_index.as_ref().is_some_and(|replay| {
                        replay.children[entry].is_some_and(|child| child.clipped_surface)
                    }) || damage.insert(extent)
                }) {
                    return Self::Full;
                }
                if !previous_batches
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
        previous: &mut Option<Self>,
    ) -> Option<(Self, Option<WindowDamage>)> {
        let input = frame.input;
        if !matches!(item_paint(scene), ItemPaint::Window)
            || scene.opacity() != 1.0
            || !layout.local_transform().is_identity()
            || !window_uses_only_clear(scene, layout, frame.scale)
            || renderer.inspection_outline.is_some()
            || frame
                .transitions
                .is_some_and(crate::StackTransitions::is_active)
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
        if let Some(last) = previous.as_mut()
            && last.style == style
            && last.geometry.scale == frame.scale
            && last.geometry.index.viewport.width == target.width() as f32
            && last.geometry.index.viewport.height == target.height() as f32
            && let Some(layout) = frame.prepared_layout
            && let Some(damage) = last.update_rectangles(scene, layout, frame)
        {
            let current = previous.take()?;
            let planned = WindowDamage::resolve_damage(damage, &current.geometry, &[]);
            return Some((current, Some(planned)));
        }
        let previous = previous.as_ref();
        let mut paint_updates = DamageRegions::default();
        let mut paint_since_geometry = DamageRegions::default();
        let can_reuse = previous.filter(|previous| {
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
        });
        let geometry = if let Some(previous) = can_reuse {
            // This history is replayed only when the index is replaced.
            // Keep the current frame's independently bounded paint updates.
            paint_since_geometry = previous.paint_since_geometry;
            paint_since_geometry.extend_or_union(paint_updates);
            previous.geometry.index.enable_grid();
            previous.geometry.clone()
        } else {
            paint_updates = DamageRegions::default();
            let viewport = Rect::new(0.0, 0.0, target.width() as f32, target.height() as f32);
            let replay = can_replay_children(scene, layout, frame);
            let mut geometry = Geometry::new(viewport, frame.scale, target.format(), replay);
            let previous = previous.filter(|previous| previous.style == style);
            geometry.capture_item(scene, layout, &mut Vec::new(), 0, frame, previous)?;
            xrack_runtime::update_profile::build_window_geometry();
            Rc::new(geometry)
        };
        Some((
            Self {
                root: scene.clone(),
                layout: layout_key,
                input: geometry.paint_input(frame),
                active_viewports: geometry.active_viewports(scene, layout, frame),
                geometry,
                style,
                paint_updates,
                paint_since_geometry,
            },
            None,
        ))
    }

    fn update_rectangles(
        &mut self,
        scene: &Node,
        layout: &Rc<LayoutNode>,
        frame: PaintFrame<'_>,
    ) -> Option<DamageRegions> {
        // Batch records and root children keep their existing paint order.
        // Validate every change before updating the previous frame.
        let geometry = Rc::get_mut(&mut self.geometry)?;
        let replay = geometry.replay_index.as_ref()?;
        let children = &replay.children;
        let before = self.layout.as_ref()?;
        if Rc::ptr_eq(before, layout)
            || !self.root.same_own_paint_inputs(scene)
            || before.children.len() != layout.children.len()
            || !layout.world_transform().is_identity()
            || before.bounds != layout.bounds
            || before.clip != layout.clip
        {
            return None;
        }
        if children.len() != geometry.index.entries.len() {
            return None;
        }
        let changes = layout.window_child_changes_from(before, &self.root, scene);
        if changes.is_none()
            && (!replay.groups[0].opaque
                || children.len() != scene.children.len()
                || children.len() != self.root.children.len()
                || children.len() + 1 != geometry.items.len()
                || !layout
                    .child_paint_order()
                    .eq(children.iter().flatten().map(|child| child.index)))
        {
            return None;
        }
        let projected = changes.into_iter().flatten().filter_map(|(path, _)| {
            let entry = replay.entry_for_path(path);
            if entry.is_none() {
                let clipped = |root: &Node, layout: &LayoutNode| {
                    root.at_path(path)
                        .zip(layout.at_path(path))
                        .is_some_and(|(node, item)| {
                            !shadow::nonempty(paint_extent(node, item, frame))
                        })
                };
                // A leaf wholly outside its ancestor clip has no index record.
                if clipped(&self.root, before) && clipped(scene, layout) {
                    return None;
                }
            }
            Some(entry)
        });
        let scan_count = if changes.is_some() { 0 } else { children.len() };
        let scan = (0..scan_count).map(Some);
        let mut damage = DamageRegions::default();
        let mut updates = Vec::new();
        for entry in projected.chain(scan) {
            let entry = entry?;
            let child = children[entry]?;
            if !child.opaque_fill {
                return None;
            }
            let item = child.item;
            let parent = &replay.groups[child.group].path;
            let old_node = self.root.at_path(parent)?.child_at(child.index)?;
            let new_node = scene.at_path(parent)?.child_at(child.index)?;
            let prior = before.at_path(parent)?.child_at(child.index)?;
            let placed = layout.at_path(parent)?.child_at(child.index)?;
            if prior.visible != placed.visible
                || prior.clip != placed.clip
                || prior.enabled != placed.enabled
            {
                return None;
            }
            if prior.bounds == placed.bounds && old_node.shares_storage_with(new_node) {
                continue;
            }
            if !old_node.same_own_paint_except_position_and_background(new_node)
                || !placed.world_transform().is_identity()
                || can_clip_surface(new_node, placed, frame)
                || updates.len() == MAX_REGIONS
            {
                return None;
            }
            let rectangle = crate::rectangle::Solid::from_node(new_node, placed, frame.scale)?;
            rectangle.opaque_rect()?;
            let logical = paint_extent(new_node, placed, frame);
            let extent = device_extent(new_node, placed, logical, frame.scale)
                .intersect(geometry.index.viewport);
            if !shadow::nonempty(extent)
                || !damage.insert(geometry.index.entries[entry].0)
                || !damage.insert(extent)
            {
                return None;
            }
            updates.push((entry, item, new_node, placed.bounds, extent));
        }
        if updates.is_empty() && changes.is_none() {
            return None;
        }
        let input = geometry.paint_input(frame);
        for &index in self.input.keys().chain(input.keys()) {
            let previous = self.input.get(&index).copied().unwrap_or_default();
            let current = input.get(&index).copied().unwrap_or_default();
            if previous.needs_damage(current) && !damage.insert(geometry.items[index].extent) {
                return None;
            }
        }
        let active_viewports = geometry.active_viewports(scene, layout, frame);
        for &position in self.active_viewports.iter().chain(&active_viewports) {
            if !damage.insert(geometry.items[geometry.islands[position].item].extent) {
                return None;
            }
        }
        geometry.index.enable_grid();
        for (entry, item, node, bounds, extent) in updates {
            geometry.index.reposition(entry, extent);
            let item = &mut geometry.items[item];
            item.node = node.clone();
            item.bounds = bounds;
            item.extent = extent;
        }
        self.root = scene.clone();
        self.layout = Some(layout.clone());
        self.input = input;
        self.active_viewports = active_viewports;
        self.paint_updates = damage;
        Some(damage)
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
            .all(|(position, (old, new))| {
                let before = &self.geometry.items[old.item];
                let current = &after.geometry.items[new.item];
                if before.path != current.path {
                    return false;
                }
                let active = self.active_viewports.binary_search(&position).is_ok()
                    || after.active_viewports.binary_search(&position).is_ok();
                // Scrolling changes the viewport's placement even when
                // its large scene subtree is mostly unchanged.
                let unchanged = !active
                    && (Rc::ptr_eq(&old.work, &new.work)
                        || ((same_layout
                            || self.layout.as_ref().zip(after.layout.as_ref()).is_some_and(
                                |(old, new)| {
                                    old.at_path(&before.path) == new.at_path(&current.path)
                                },
                            ))
                            && (before.node.shares_storage_with(&current.node)
                                || before.node == current.node)));
                unchanged || damage.insert(before.extent) && damage.insert(current.extent)
            })
    }

    fn input_changed(&self, old: usize, after: &Self, new: usize) -> bool {
        let old = self.input.get(&old).copied().unwrap_or_default();
        let new = after.input.get(&new).copied().unwrap_or_default();
        old.needs_damage(new)
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
    if is_paint_viewport(item_paint(before))
        || before.children.len() != after.children.len()
        || after.children.len() != layout.children.len()
    {
        return false;
    }
    if !before.same_own_paint_inputs(after) {
        if !matches!(item_paint(after), ItemPaint::Rectangle)
            || !after.children.is_empty()
            || !before.same_own_paint_except_background(after)
        {
            return false;
        }
        let fill = |node| crate::rectangle::Solid::from_node(node, layout, frame.scale);
        let (Some(old), Some(new)) = (fill(before), fill(after)) else {
            return false;
        };
        // An update behind an ancestor clip cannot change the backing image
        // or the existing spatial index.
        if !old.visible() && !new.visible() {
            return true;
        }
        let opaque = |node: &Node| {
            node.color("background")
                .is_some_and(|color| color.0 & 0xff == 0xff)
        };
        if !old.visible() || !new.visible() || opaque(before) != opaque(after) {
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
    islands: Vec<PaintViewport>,
    index: PaintIndex,
    replay_index: Option<PaintReplayIndex>,
    scale: f32,
}

pub(crate) type PaintReplay<'a> = BTreeMap<&'a [usize], Vec<crate::PaintChild>>;

struct PaintReplayIndex {
    // One entry per spatial index record. Batch bounds have no child.
    children: Vec<Option<IndexedChild>>,
    groups: Vec<ReplayGroup>,
}

struct ReplayGroup {
    path: Vec<usize>,
    opaque: bool,
    first_entry: usize,
}

#[derive(Clone, Copy)]
struct IndexedChild {
    group: usize,
    index: usize,
    item: usize,
    opaque_fill: bool,
    // A panel is selected by its whole bounds, then paints only inside the damage.
    clipped_surface: bool,
}

impl PaintReplayIndex {
    fn entry_for_path(&self, path: &[usize]) -> Option<usize> {
        let (&child, parent) = path.split_last()?;
        let group = self.groups.iter().position(|group| group.path == parent)?;
        let first_entry = self.groups[group].first_entry;
        let matches = |entry: &Option<IndexedChild>| {
            entry.is_some_and(|entry| entry.group == group && entry.index == child)
        };
        let candidate = first_entry.checked_add(child)?;
        if self.children.get(candidate).is_some_and(matches) {
            return Some(candidate);
        }
        self.children[first_entry..]
            .iter()
            .position(matches)
            .map(|offset| first_entry + offset)
    }
}

struct PaintViewport {
    item: usize,
    // Sharing this handle across snapshots certifies that the viewport's
    // node and complete layout are unchanged.
    work: Rc<u32>,
}

impl Geometry {
    fn new(viewport: Rect, scale: f32, format: wgpu::TextureFormat, can_replay: bool) -> Self {
        let replay_index = (can_replay
            && matches!(
                format,
                wgpu::TextureFormat::Rgba8Unorm
                    | wgpu::TextureFormat::Rgba8UnormSrgb
                    | wgpu::TextureFormat::Bgra8Unorm
                    | wgpu::TextureFormat::Bgra8UnormSrgb
            ))
        .then(|| PaintReplayIndex {
            children: Vec::new(),
            groups: vec![ReplayGroup {
                path: Vec::new(),
                opaque: true,
                first_entry: 0,
            }],
        });
        Self {
            items: Vec::new(),
            batches: Vec::new(),
            paths: BTreeMap::new(),
            independent: Vec::new(),
            islands: Vec::new(),
            index: PaintIndex::new(viewport),
            replay_index,
            scale,
        }
    }

    fn patch_key(&self, scene: &Node, regions: DamageRegions) -> Option<PatchKey> {
        const MAX_PATCH_PIXELS: u64 = 32_768;
        const MAX_PATCH_FILLS: usize = 256;
        let [region] = regions.rectangles() else {
            return None;
        };
        if u64::from(region.width) * u64::from(region.height) > MAX_PATCH_PIXELS {
            return None;
        }
        let replay = self.replay_index.as_ref()?;
        let mut fills = Vec::new();
        let supported = self.index.visit(region.as_rect(), |index, _, _| {
            let Some(entry) = replay.children[index] else {
                return true;
            };
            if entry.clipped_surface {
                return true;
            }
            let Some(node) = scene
                .at_path(&replay.groups[entry.group].path)
                .and_then(|node| node.child_at(entry.index))
            else {
                return false;
            };
            match item_paint(node) {
                ItemPaint::Rectangle => {
                    if let Some(color) = node.color("background") {
                        fills.push((index, color.0));
                    }
                    fills.len() <= MAX_PATCH_FILLS
                }
                ItemPaint::Label | ItemPaint::Text => true,
                _ => false,
            }
        });
        if !supported || fills.is_empty() {
            return None;
        }
        fills.sort_unstable_by_key(|&(index, _)| index);
        Some(PatchKey {
            region: *region,
            fills,
        })
    }

    fn solid_updates(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        regions: DamageRegions,
        frame: PaintFrame<'_>,
    ) -> Option<Vec<SolidUpdate>> {
        const MAX_SOLID_UPLOAD_PIXELS: u64 = 4096;
        const MAX_PAINTERS: usize = 64;
        const MAX_PIECES: usize = 16;
        let replay = self.replay_index.as_ref()?;
        let area = regions
            .rectangles()
            .iter()
            .map(|rect| u64::from(rect.width) * u64::from(rect.height))
            .sum::<u64>();
        if area > MAX_SOLID_UPLOAD_PIXELS {
            return None;
        }
        let background = || {
            crate::window_clear_background(scene, layout, layout.bounds, frame.scale)
                .or_else(|| {
                    (!scene.is_item_delegate() && !scene.properties.contains_key("background"))
                        .then_some(frame.theme.window)
                })
                .filter(|color| color.0 & 0xff == 0xff)
                .map(|color| color.0.to_be_bytes())
        };
        let opaque_source = |entry: IndexedChild| {
            let path = &replay.groups[entry.group].path;
            let node = scene.at_path(path)?.child_at(entry.index)?;
            let layout = layout.at_path(path)?.child_at(entry.index)?;
            let (bounds, color) = if entry.opaque_fill {
                crate::rectangle::Solid::from_node(node, layout, self.scale)?.opaque_rect()?
            } else if entry.clipped_surface
                && crate::rectangle::plain_surface(node, layout)
                && !node.is_item_delegate()
                && crate::pixel_aligned(layout.bounds, frame.scale)
                && can_clip_surface(node, layout, frame)
            {
                (
                    layout.bounds.intersect(layout.clip),
                    node.color("background")?.0,
                )
            } else {
                return None;
            };
            Some((
                crate::retained_layers::device_bounds(bounds, self.scale),
                color.to_be_bytes(),
            ))
        };
        regions
            .rectangles()
            .iter()
            .map(|rect| {
                let region = rect.as_rect();
                let mut top = None;
                self.index.visit(region, |index, _, _| {
                    if replay.children[index].is_some() && top.is_none_or(|top| index > top) {
                        top = Some(index);
                    }
                    true
                });
                let Some(top) = top else {
                    return Some(SolidUpdate {
                        region: *rect,
                        pixels: SolidPixels::Single(background()?),
                    });
                };
                let (bounds, color) = opaque_source(replay.children[top]?)?;
                if region.intersect(bounds) == region {
                    return Some(SolidUpdate {
                        region: *rect,
                        pixels: SolidPixels::Single(color),
                    });
                }
                let mut entries = Vec::new();
                let collected = self.index.visit(region, |index, extent, _| {
                    if let Some(child) = replay.children[index] {
                        entries.push((index, extent, child));
                    }
                    entries.len() <= MAX_PAINTERS
                });
                if !collected {
                    return None;
                }
                // Only the topmost visible painter contributes to a filled piece.
                entries.sort_unstable_by_key(|(index, _, _)| std::cmp::Reverse(*index));
                let mut pending = vec![region];
                let mut remainder = Vec::new();
                let mut fills = Vec::new();
                for (_, extent, entry) in entries {
                    if !pending.iter().any(|&piece| overlap(piece, extent)) {
                        continue;
                    }
                    let (bounds, color) = opaque_source(entry)?;
                    for piece in pending.drain(..) {
                        let painted = piece.intersect(bounds);
                        if !shadow::nonempty(painted) {
                            remainder.push(piece);
                            continue;
                        }
                        fills.push((DamageRect::from_rect(painted), color));
                        split_uncovered(piece, painted, &mut remainder);
                    }
                    std::mem::swap(&mut pending, &mut remainder);
                    if fills.len() + pending.len() > MAX_PIECES {
                        return None;
                    }
                    if pending.is_empty() {
                        break;
                    }
                }
                if !pending.is_empty() {
                    let color = background()?;
                    fills.extend(
                        pending
                            .into_iter()
                            .map(|piece| (DamageRect::from_rect(piece), color)),
                    );
                }
                Some(SolidUpdate {
                    region: *rect,
                    pixels: SolidPixels::Pieces(fills),
                })
            })
            .collect()
    }

    fn damaged_children(&self, regions: DamageRegions) -> Option<PaintReplay<'_>> {
        let replay = self.replay_index.as_ref()?;
        if replay.children.len() != self.index.entries.len() {
            return None;
        }
        let mut entries = Vec::new();
        for region in regions.rectangles() {
            self.index.visit(region.as_rect(), |index, _, _| {
                entries.push(index);
                true
            });
        }
        // Grid traversal is in spatial order. Replay the original paint order
        // so overlapping children still occlude each other at the same pixels.
        entries.sort_unstable();
        entries.dedup();
        let mut previous = vec![None; replay.groups.len()];
        let mut selected = PaintReplay::new();
        for entry in entries {
            let Some(child) = replay.children[entry] else {
                continue;
            };
            let group = &replay.groups[child.group];
            // A skipped painter still separates batches in a mixed group.
            let last = &mut previous[child.group];
            let flush_before = !group.opaque && last.is_some_and(|prior| prior + 1 != entry);
            *last = Some(entry);
            selected
                .entry(group.path.as_slice())
                .or_default()
                .push(crate::PaintChild {
                    index: child.index,
                    flush_before,
                });
        }
        Some(selected)
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
            .filter_map(|(position, viewport)| {
                let item = &self.items[viewport.item];
                let active = input
                    .window_paint_targets()
                    .any(|path| path.starts_with(item.path.as_slice()))
                    || input.pointer_position().is_some_and(|point| {
                        item.extent.contains(crate::Point {
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
        if self.paths.is_empty() && self.independent.is_empty() {
            return states;
        }
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
            if let Some(root) = &mut self.replay_index
                && shadow::nonempty(run)
            {
                root.children.push(None);
            }
            self.index.add(run, 0);
        }
    }

    fn capture_item(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        group: usize,
        frame: PaintFrame<'_>,
        previous: Option<&Scene>,
    ) -> Option<Batchable> {
        let paint = item_paint(node);
        if !layout.visible
            || node.opacity() == 0.0
            || matches!(paint, ItemPaint::Shortcut)
            || node.scene_texture_only()
        {
            return Some(Batchable::Primitives(None));
        }
        let viewport = is_paint_viewport(paint);
        let island_work = if viewport {
            frame.prepared_layout?;
            // A frozen viewport can be skipped only if all its visible content
            // has bounded, non-autonomous paint.
            if layout.allows_overflow() {
                return None;
            }
            let retained = previous.and_then(|previous| {
                let viewport = previous.geometry.islands.get(self.islands.len())?;
                let item = &previous.geometry.items[viewport.item];
                (item.path == *path
                    && item.node.shares_storage_with(node)
                    && previous.layout.as_ref()?.at_path(path)? == layout)
                    .then(|| viewport.work.clone())
            });
            Some(match retained {
                Some(work) => work,
                None => Rc::new(static_viewport_work(node, layout)?),
            })
        } else {
            if !basic_painter(paint)
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
        let button = matches!(paint, ItemPaint::Button(_));
        let editor = matches!(paint, ItemPaint::Entry { .. } | ItemPaint::SearchEntry);
        let styled = matches!(paint, ItemPaint::StyledText);
        let drop_area = matches!(paint, ItemPaint::DropArea);
        let tracks_input = button
            || editor
            || styled
            || drop_area
            || matches!(
                paint,
                ItemPaint::FocusScope | ItemPaint::SplitHandle | ItemPaint::Pane
            );
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
        let clip_surface = !path.is_empty() && can_clip_surface(node, layout, frame);
        let clipped_surface = clip_surface && self.replay_index.is_some();
        let indexed = !path.is_empty() && !paintless && (!clip_surface || clipped_surface);
        let rectangle = if matches!(paint, ItemPaint::Rectangle) {
            crate::rectangle::Solid::from_node(node, layout, frame.scale)
        } else {
            None
        };
        let opaque_fill = self.replay_index.is_some()
            && indexed
            && shadow::nonempty(extent)
            && layout.world_transform().is_identity()
            && rectangle.is_some_and(|rectangle| rectangle.opaque_rect().is_some());
        let mut child_group = group;
        if let Some(replay) = &mut self.replay_index
            && let Some(&child) = path.last()
        {
            replay.groups[group].opaque &= opaque_fill;
            if indexed && shadow::nonempty(extent) {
                replay.children.push(Some(IndexedChild {
                    group,
                    index: child,
                    item: index,
                    opaque_fill,
                    clipped_surface,
                }));
            }
            if clipped_surface {
                child_group = replay.groups.len();
                replay.groups.push(ReplayGroup {
                    path: path.clone(),
                    opaque: true,
                    first_entry: replay.children.len(),
                });
            }
        }
        if indexed {
            self.index.add(
                extent,
                island_work
                    .as_deref()
                    .copied()
                    .unwrap_or_else(|| paint_weight(node, button, editor)),
            );
        }
        if let Some(work) = island_work {
            self.islands.push(PaintViewport { item: index, work });
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
            let batchable = self.capture_item(child, item, path, child_group, frame, previous)?;
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
        let batchable = if opaque_fill {
            // Pixel-aligned opaque fills preserve their coverage when replayed alone.
            Batchable::Primitives(None)
        } else if paintless
            && !has_barrier
            && layout.child_clip_shape().is_none()
            && Renderer::is_unpainted_group(node, layout)
        {
            Batchable::Primitives(run)
        } else if matches!(
            paint,
            ItemPaint::Text | ItemPaint::Label | ItemPaint::Image { animated: false }
        ) || rectangle.is_some()
        {
            Batchable::Primitives(shadow::nonempty(extent).then_some(extent))
        } else {
            Batchable::Barrier
        };
        Some(batchable)
    }
}

fn can_replay_children(node: &Node, layout: &LayoutNode, frame: PaintFrame<'_>) -> bool {
    layout
        .children_in_paint_order(node)
        .all(|(_, child, placed)| {
            let opacity = child.opacity();
            let paint = item_paint(child);
            if !placed.visible
                || opacity == 0.0
                || child.scene_texture_only()
                || matches!(paint, ItemPaint::Shortcut)
            {
                return true;
            }
            if matches!(paint, ItemPaint::DropArea) && paintless_container(child) {
                // Its drag outline can appear without an indexed fill.
                return false;
            }
            // Shrinking a batch must not switch fractional coverage or an independent
            // opacity between different paint paths.
            opacity == 1.0
                && if child.children.is_empty() {
                    !matches!(paint, ItemPaint::Rectangle)
                        || placed.world_transform().is_identity()
                            && crate::pixel_aligned(placed.bounds, frame.scale)
                            && crate::pixel_aligned(
                                placed.bounds.intersect(placed.clip),
                                frame.scale,
                            )
                } else {
                    can_clip_surface(child, placed, frame)
                        && can_replay_children(child, placed, frame)
                }
        })
}

pub(crate) fn is_paint_viewport(paint: ItemPaint) -> bool {
    matches!(
        paint,
        ItemPaint::ScrollView | ItemPaint::Rows | ItemPaint::TableView
    )
}

fn basic_painter(paint: ItemPaint) -> bool {
    matches!(
        paint,
        ItemPaint::Window
            | ItemPaint::Rectangle
            | ItemPaint::Text
            | ItemPaint::Label
            | ItemPaint::StyledText
            | ItemPaint::Image { animated: false }
            | ItemPaint::Vector
            | ItemPaint::Pane
            | ItemPaint::DropArea
            | ItemPaint::FocusScope
            | ItemPaint::Container(
                ContainerPaint::Empty
                    | ContainerPaint::Row
                    | ContainerPaint::Horizontal
                    | ContainerPaint::Vertical
                    | ContainerPaint::BoxOrFlow
                    | ContainerPaint::Grid
                    | ContainerPaint::Split
                    | ContainerPaint::ButtonContent
            )
            | ItemPaint::Button(ButtonStyle::Push | ButtonStyle::Tool | ButtonStyle::Round)
            | ItemPaint::SplitHandle
            | ItemPaint::Entry { .. }
            | ItemPaint::SearchEntry
    )
}

fn static_viewport_work(node: &Node, layout: &LayoutNode) -> Option<u32> {
    if !layout.visible || node.opacity() == 0.0 {
        return Some(0);
    }
    let paint = item_paint(node);
    if matches!(paint, ItemPaint::DropArea)
        || node.boolean("pull-to-refresh", false)
        || node.button_press_handle.is_some()
        || matches!(paint, ItemPaint::Entry { .. } | ItemPaint::SearchEntry)
        || !layout.list_section_headers().is_empty()
        || !(basic_painter(paint)
            || is_paint_viewport(paint)
            || matches!(
                paint,
                ItemPaint::ListRow
                    | ItemPaint::StandardListItem
                    | ItemPaint::ViewportPart
                    | ItemPaint::TableRow
                    | ItemPaint::Cell { .. }
                    | ItemPaint::ScrollBar {
                        interactive: Some(_)
                    }
            ))
    {
        return None;
    }
    // An empty content scissor skips the subtree's paint work. Keep the
    // viewport's painter eligibility checks independent of that estimate.
    if !layout.allows_overflow()
        && crate::is_clipped_out(layout.bounds, layout.clip)
        && !node
            .properties
            .keys()
            .any(|name| name.starts_with("drop-shadow-"))
    {
        return Some(0);
    }
    let weight = paint_weight(node, matches!(paint, ItemPaint::Button(_)), false);
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
    fn needs_damage(self, current: Self) -> bool {
        self.editing
            || current.editing
            || self.drop_target
            || current.drop_target
            || self != current
    }

    fn capture(node: &Node, path: &[usize], frame: PaintFrame<'_>) -> Self {
        let paint = item_paint(node);
        let button = matches!(paint, ItemPaint::Button(_));
        let editor = matches!(paint, ItemPaint::Entry { .. } | ItemPaint::SearchEntry);
        let split_handle = matches!(paint, ItemPaint::SplitHandle);
        let styled = matches!(paint, ItemPaint::StyledText);
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
                || matches!(paint, ItemPaint::FocusScope | ItemPaint::Pane))
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
            drop_target: matches!(paint, ItemPaint::DropArea) && input.is_drop_target(node, path),
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

    fn cells_crossed(&self, extent: Rect) -> impl Iterator<Item = usize> + use<> {
        let (x0, y0) = self.cell(extent.x, extent.y);
        let (x1, y1) = self.cell(
            extent.x + extent.width - 1.0,
            extent.y + extent.height - 1.0,
        );
        (y0..=y1).flat_map(move |y| (x0..=x1).map(move |x| y * BANDS + x))
    }

    fn reposition(&mut self, entry: usize, extent: Rect) {
        let previous = self.entries[entry].0;
        if previous == extent {
            return;
        }
        let remove = self.cells_crossed(previous);
        let add = self.cells_crossed(extent);
        if let Some(cells) = self.cells.get_mut() {
            for cell in remove {
                cells[cell].retain(|&index| index != entry);
            }
            for cell in add {
                cells[cell].push(entry);
            }
        }
        self.entries[entry].0 = extent;
    }

    fn enable_grid(&self) {
        // Small or one-off layouts are cheaper to scan than to index.
        if self.entries.len() < BANDS * BANDS {
            return;
        }
        self.cells.get_or_init(|| {
            let mut cells: [Vec<usize>; BANDS * BANDS] = std::array::from_fn(|_| Vec::new());
            for (index, &(extent, _)) in self.entries.iter().enumerate() {
                for cell in self.cells_crossed(extent) {
                    cells[cell].push(index);
                }
            }
            cells
        });
    }

    fn visit(&self, region: Rect, mut visit: impl FnMut(usize, Rect, u32) -> bool) -> bool {
        let Some(cells) = self.cells.get() else {
            return self
                .entries
                .iter()
                .enumerate()
                .all(|(index, &(extent, work))| {
                    !overlap(region, extent) || visit(index, extent, work)
                });
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
                        && !visit(index, extent, work)
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
                self.visit(region.as_rect(), |_, extent, work| {
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
    let content = match item_paint(node) {
        ItemPaint::Text | ItemPaint::Label => 3 + text(),
        ItemPaint::StyledText => {
            3 + node.styled_text("text").map_or(0, |document| {
                document.text().len().min(1024).div_ceil(8) as u32
            })
        }
        ItemPaint::Image { animated: false } => 4,
        _ if button || editor => 8 + text(),
        _ => 1,
    };
    let shadow = node.properties.contains_key("drop-shadow-color");
    content + if shadow { 8 } else { 0 }
}

fn device_extent(node: &Node, layout: &LayoutNode, bounds: Rect, scale: f32) -> Rect {
    let paint = item_paint(node);
    let raster = crate::retained_layers::device_bounds(bounds, scale);
    let grouped = is_paint_viewport(paint)
        || matches!(
            paint,
            ItemPaint::Pane
                | ItemPaint::Button(ButtonStyle::Push | ButtonStyle::Tool | ButtonStyle::Round)
                | ItemPaint::SplitHandle
        )
        || matches!(paint, ItemPaint::Rectangle)
            && (!node.children.is_empty()
                || crate::rectangle::Solid::from_node(node, layout, scale)
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
        && crate::pixel_aligned(bounds, scale)
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
            || crate::window_clear_background(node, layout, layout.bounds, scale).is_some())
}

fn paintless_container(node: &Node) -> bool {
    matches!(
        item_paint(node),
        ItemPaint::Rectangle
            | ItemPaint::DropArea
            | ItemPaint::Container(
                ContainerPaint::Row
                    | ContainerPaint::Horizontal
                    | ContainerPaint::Vertical
                    | ContainerPaint::BoxOrFlow
                    | ContainerPaint::Grid
                    | ContainerPaint::Split
                    | ContainerPaint::ButtonContent
            )
    ) && node.opacity() == 1.0
        && !node.properties.contains_key("background")
        && !has_edge_effect(node)
}

/// Opaque square container surfaces can be replayed through the damage scissor.
pub(crate) fn can_clip_surface(node: &Node, layout: &LayoutNode, frame: PaintFrame<'_>) -> bool {
    let paint = item_paint(node);
    if !matches!(paint, ItemPaint::Rectangle | ItemPaint::Pane)
        || node.children.is_empty()
        || node.opacity() != 1.0
        || node.boolean("cache-rendering-hint", false)
        || layout.allows_overflow()
        || !layout.world_transform().is_identity()
        || layout.has_ancestor_clip_chain()
        || layout.table_pane_clip().is_some()
        || has_edge_effect(node)
        || xrack_layout::corner_radii(node, layout.bounds) != [0.0; 4]
        || !crate::pixel_aligned(layout.bounds.intersect(layout.clip), frame.scale)
    {
        return false;
    }
    match node.properties.get("background") {
        Some(xrack_runtime::Value::Color(color))
        | Some(xrack_runtime::Value::Brush(xrack_runtime::Brush::Solid(color))) => {
            color.0 & 0xff == 0xff
        }
        None if matches!(paint, ItemPaint::Pane) => {
            frame.theme.role(Role::Surface).0 & 0xff == 0xff
        }
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

pub(crate) fn intersects_damage(
    node: &Node,
    layout: &LayoutNode,
    frame: PaintFrame<'_>,
    damage: crate::PaintDamage<'_>,
) -> bool {
    let extent = paint_extent(node, layout, frame);
    shadow::nonempty(extent)
        && damage
            .regions
            .intersects(device_extent(node, layout, extent, frame.scale))
}

#[derive(PartialEq, Eq)]
struct PatchKey {
    region: DamageRect,
    fills: Vec<(usize, u32)>,
}

struct PatchEntry {
    key: PatchKey,
    texture: Option<wgpu::Texture>,
}

struct PatchCopy {
    texture: wgpu::Texture,
    region: DamageRect,
    restore: bool,
}

struct WindowBacking {
    texture: wgpu::Texture,
    scale: f32,
    valid: bool,
    transfer_source: Option<wgpu::BindGroup>,
    upload: Vec<u8>,
    patches: std::collections::VecDeque<PatchEntry>,
}

struct WindowBlitter {
    format: wgpu::TextureFormat,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}

impl WindowBlitter {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
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
        Self {
            format,
            layout,
            pipeline,
        }
    }

    fn bind_source(&self, device: &wgpu::Device, source: &wgpu::Texture) -> wgpu::BindGroup {
        let view = source.create_view(&Default::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack window image input"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        })
    }

    fn copy(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::BindGroup,
        target: &wgpu::Texture,
    ) {
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
        pass.set_bind_group(0, source, &[]);
        pass.draw(0..3, 0..1);
    }
}

impl WindowBacking {
    fn upload_solid_updates(&mut self, queue: &wgpu::Queue, updates: &[SolidUpdate]) {
        let bgra = self.texture.format() == wgpu::TextureFormat::Bgra8Unorm;
        let texture_color = |mut color: [u8; 4]| {
            if bgra {
                color.swap(0, 2);
            }
            color
        };
        let fill_pixels = |bytes: &mut [u8], color: [u8; 4]| {
            for pixel in bytes.chunks_exact_mut(4) {
                pixel.copy_from_slice(&color);
            }
        };
        for update in updates {
            let rect = update.region;
            let stride = rect.width as usize * 4;
            let size = stride * rect.height as usize;
            self.upload.resize(size, 0);
            match &update.pixels {
                SolidPixels::Single(color) => fill_pixels(&mut self.upload, texture_color(*color)),
                SolidPixels::Pieces(fills) => {
                    for &(fill, color) in fills {
                        let color = texture_color(color);
                        let left = (fill.x - rect.x) as usize * 4;
                        let width = fill.width as usize * 4;
                        let first_row = (fill.y - rect.y) as usize;
                        for row in first_row..first_row + fill.height as usize {
                            let start = row * stride + left;
                            fill_pixels(&mut self.upload[start..start + width], color);
                        }
                    }
                }
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: rect.x,
                        y: rect.y,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &self.upload,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(rect.width * 4),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: rect.width,
                    height: rect.height,
                    depth_or_array_layers: 1,
                },
            );
            xrack_runtime::update_profile::texture_upload(size);
        }
    }

    fn new(renderer: &Renderer, target: &wgpu::Texture, scale: f32) -> Option<Self> {
        let format = target.format();
        let allowed = renderer
            .adapter
            .get_texture_format_features(format)
            .allowed_usages;
        let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC;
        // Non-sRGB formats store the fill's original bytes without conversion.
        if matches!(
            format,
            wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm
        ) && allowed.contains(wgpu::TextureUsages::COPY_DST)
        {
            usage |= wgpu::TextureUsages::COPY_DST;
        }
        if !allowed.contains(usage)
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
            transfer_source: None,
            upload: Vec::new(),
            patches: Default::default(),
        })
    }

    fn matches(&self, target: &wgpu::Texture, scale: f32) -> bool {
        self.texture.size() == target.size()
            && self.texture.format() == target.format()
            && self.scale == scale
    }

    fn prepare_patch(&mut self, device: &wgpu::Device, key: PatchKey) -> Option<PatchCopy> {
        const PATCH_COUNT: usize = 8;
        let Some(index) = self.patches.iter().position(|entry| entry.key == key) else {
            // Observe the state twice before reserving GPU memory.
            if self.patches.len() == PATCH_COUNT {
                self.patches.pop_front();
            }
            self.patches.push_back(PatchEntry { key, texture: None });
            return None;
        };
        let mut entry = self.patches.remove(index)?;
        let region = entry.key.region;
        let restore = entry.texture.is_some();
        let texture = entry
            .texture
            .get_or_insert_with(|| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("XRack repeat damage"),
                    size: wgpu::Extent3d {
                        width: region.width,
                        height: region.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: self.texture.format(),
                    usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                })
            })
            .clone();
        self.patches.push_back(entry);
        Some(PatchCopy {
            texture,
            region,
            restore,
        })
    }

    fn encode_transfer(
        &mut self,
        renderer: &Renderer,
        target: &wgpu::Texture,
        transfer_pipeline: &mut Option<WindowBlitter>,
        patch: Option<&PatchCopy>,
    ) -> wgpu::CommandBuffer {
        let mut encoder = renderer
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("XRack transfer window image"),
            });
        if let Some(patch) = patch {
            let region = patch.region;
            let window = wgpu::TexelCopyTextureInfo {
                origin: wgpu::Origin3d {
                    x: region.x,
                    y: region.y,
                    z: 0,
                },
                ..self.texture.as_image_copy()
            };
            let patch_source = patch.texture.as_image_copy();
            let (source, destination) = if patch.restore {
                (patch_source, window)
            } else {
                (window, patch_source)
            };
            encoder.copy_texture_to_texture(source, destination, patch.texture.size());
        }
        if target.usage().contains(wgpu::TextureUsages::COPY_DST) {
            encoder.copy_texture_to_texture(
                self.texture.as_image_copy(),
                target.as_image_copy(),
                target.size(),
            );
        } else {
            let format = target.format();
            if transfer_pipeline
                .as_ref()
                .is_some_and(|blitter| blitter.format != format)
            {
                *transfer_pipeline = None;
            }
            let blitter = transfer_pipeline
                .get_or_insert_with(|| WindowBlitter::new(&renderer.device, format));
            let source = self
                .transfer_source
                .get_or_insert_with(|| blitter.bind_source(&renderer.device, &self.texture));
            blitter.copy(&mut encoder, source, target);
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
pub(crate) struct WindowDamageState {
    forced: bool,
    backing: Option<WindowBacking>,
    // The pipeline depends on the target format. A resize only needs a new
    // backing texture and source binding.
    transfer_pipeline: Option<WindowBlitter>,
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
            .then(|| Scene::capture(renderer, scene, layout, frame, target, &mut self.previous))
            .flatten();
        let Some((current, projected)) = current else {
            self.previous = None;
            self.defer_analysis();
            return self.direct_paint(renderer, scene, layout, target, frame);
        };

        let planned = projected.or_else(|| {
            self.previous
                .as_ref()
                .map(|previous| WindowDamage::between(previous, &current))
        });
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
        // A shared index proves that only eligible background values changed.
        // Position updates consume the previous snapshot before we get here.
        let same_geometry = self
            .previous
            .as_ref()
            .is_some_and(|previous| Rc::ptr_eq(&previous.geometry, &current.geometry));
        if !same_geometry {
            backing.patches.clear();
        }
        let work = if valid {
            planned.unwrap_or(WindowDamage::Full)
        } else {
            WindowDamage::Full
        };
        let can_update_pixels = valid
            && backing
                .texture
                .usage()
                .contains(wgpu::TextureUsages::COPY_DST)
            && !renderer.needs_redraw
            && !renderer.geometry_cache.enabled
            && !renderer.inspect_scene
            && current.input.is_empty()
            && current.geometry.islands.is_empty()
            && renderer.scene_item_textures.is_idle();
        let texture_sources_idle = || {
            !scene.contains_scene_feature(xrack_runtime::SceneFeature::SceneTextureSource)
                && !scene.contains_scene_feature(xrack_runtime::SceneFeature::SceneImageTexture)
        };
        let solid_updates = if can_update_pixels
            && let Some(regions) = damage
            && let Some(updates) = current
                .geometry
                .solid_updates(scene, layout, regions, frame)
            && texture_sources_idle()
        {
            Some(updates)
        } else {
            None
        };
        let patch = if can_update_pixels
            && same_geometry
            && solid_updates.is_none()
            && renderer.display_batching
            && let Some(regions) = damage
            && let Some(key) = current.geometry.patch_key(scene, regions)
            && texture_sources_idle()
        {
            backing.prepare_patch(&renderer.device, key)
        } else {
            None
        };
        let transfer = backing.encode_transfer(
            renderer,
            target,
            &mut self.transfer_pipeline,
            patch.as_ref(),
        );
        if work == WindowDamage::Unchanged
            || solid_updates.is_some()
            || patch.as_ref().is_some_and(|patch| patch.restore)
        {
            if let Some(updates) = solid_updates {
                backing.upload_solid_updates(&renderer.queue, &updates);
            }
            if renderer.geometry_cache.enabled || renderer.inspect_scene {
                renderer.scene_changes = renderer
                    .render_scene
                    .synchronize(scene, renderer.scene_generation);
            }
            xrack_runtime::update_profile::submit_gpu();
            xrack_runtime::update_profile::transfer_window_image();
            renderer.queue.submit([transfer]);
        } else {
            let damage = if valid { damage } else { None };
            let replay = damage.and_then(|regions| current.geometry.damaged_children(regions));
            renderer.paint_frame_with_transfer(
                scene,
                layout,
                &backing.texture,
                PaintFrame {
                    damage: damage.map(|regions| crate::PaintDamage {
                        regions,
                        replay: replay.as_ref(),
                    }),
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

    pub(crate) fn paint_window(
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
