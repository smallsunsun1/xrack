//! Offscreen captures of window-local 2D sources used by materials and effects.
use std::{
    collections::{BTreeMap, BTreeSet},
    hash::{DefaultHasher, Hash, Hasher},
    sync::{Arc, Weak},
};

use xrack_layout::{Interaction, LayoutNode, Target};
use xrack_runtime::{Node, SceneFeature, Value};

use crate::{PaintContext, PaintFrame, Palette, Renderer, primitive::scene_view_content};

use super::{
    COLOR_FORMAT,
    color_capture::capture_postorder,
    effect_history::Owner,
    retained_view::{ViewOutput, ViewSize, viewport_bytes, viewport_format},
    supersampling::{mip_count, mip_extent, mip_views},
};

const MAX_SOURCES: usize = 8;
const MAX_EDGE: f32 = 2048.0;
const MAX_BYTES: u64 = 16 * 1024 * 1024;

pub(super) type ItemTextureViews = BTreeMap<String, ItemTextureView>;

#[derive(Clone)]
pub(super) struct ItemTextureView {
    pub view: wgpu::TextureView,
    pub color_texture: wgpu::Texture,
    pub linear_premultiplied: bool,
    pub base_view: Option<wgpu::TextureView>,
    pub allocation: u64,
    pub revision: u64,
    readers: Weak<()>,
}

pub(super) struct FrozenItemTextures {
    pub views: Arc<ItemTextureViews>,
    pub revision: u64,
    _readers: Vec<Arc<()>>,
}

impl FrozenItemTextures {
    pub fn new(views: &ItemTextureViews, revision: u64) -> Self {
        Self {
            views: Arc::new(views.clone()),
            revision,
            _readers: views
                .values()
                .filter_map(|view| view.readers.upgrade())
                .collect(),
        }
    }
}

struct ItemImage {
    view: wgpu::TextureView,
    allocation: u64,
    readers: Arc<()>,
}

impl ItemImage {
    fn new(texture: &wgpu::Texture, next_allocation: &mut u64) -> Self {
        *next_allocation = next_allocation.wrapping_add(1).max(1);
        Self {
            view: texture.create_view(&Default::default()),
            allocation: *next_allocation,
            readers: Arc::new(()),
        }
    }

    fn is_pinned(&self) -> bool {
        Arc::strong_count(&self.readers) > 1
    }
}

struct MipChain {
    size: [u32; 2],
    format: wgpu::TextureFormat,
    levels: Vec<wgpu::TextureView>,
    image: ItemImage,
}

impl MipChain {
    fn new(
        device: &wgpu::Device,
        size: [u32; 2],
        format: wgpu::TextureFormat,
        next_allocation: &mut u64,
    ) -> Self {
        let mip_level_count = mip_count(size);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("XRack 3D material item mipmaps"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let levels = mip_views(&texture);
        Self {
            size,
            format,
            levels,
            image: ItemImage::new(&texture, next_allocation),
        }
    }

    fn refresh(&self, renderer: &mut Renderer, source: &wgpu::TextureView) {
        let encoder = renderer.custom_encoder.get_or_insert_with(|| {
            renderer
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("XRack 3D material item mipmaps"),
                })
        });
        let filter = renderer.scene3d.postprocess.area_filter(&renderer.device);
        if self.format == COLOR_FORMAT {
            filter.decode_item(&renderer.device, encoder, (source, &self.levels[0]));
        } else {
            filter.resolve(
                &renderer.device,
                encoder,
                (source, &self.levels[0]),
                self.size,
                self.format,
            );
        }
        filter.resolve_mips(
            &renderer.device,
            encoder,
            &self.levels,
            self.size,
            self.format,
        );
        renderer.scene3d.stats.item_texture_mip_passes += self.levels.len() as u64;
    }
}

fn chain_bytes(size: [u32; 2], format: wgpu::TextureFormat) -> u64 {
    (0..mip_count(size) as usize)
        .map(|level| crate::transient_textures::render_bytes(mip_extent(size, level), format))
        .sum()
}

#[derive(Default)]
pub(crate) struct ItemTextures {
    current: BTreeMap<String, Capture>,
    viewports: BTreeMap<String, ViewportCapture>,
    reset: bool,
    revision: u64,
    next_allocation: u64,
}

struct ViewportCapture {
    path: Vec<usize>,
    output: ViewOutput,
    allocation: u64,
    view: wgpu::TextureView,
    mipmaps: Option<MipChain>,
}

impl ViewportCapture {
    fn material_view(&self) -> ItemTextureView {
        let (view, allocation, readers) = self.mipmaps.as_ref().map_or_else(
            || {
                (
                    self.view.clone(),
                    self.allocation,
                    self.output.readers.clone(),
                )
            },
            |mipmaps| {
                (
                    mipmaps.image.view.clone(),
                    mipmaps.image.allocation,
                    Arc::downgrade(&mipmaps.image.readers),
                )
            },
        );
        ItemTextureView {
            view,
            color_texture: self.output.texture.clone(),
            allocation,
            readers,
            linear_premultiplied: self.mipmaps.is_some()
                || self.output.texture.format() != wgpu::TextureFormat::Rgba8Unorm,
            base_view: self
                .mipmaps
                .as_ref()
                .and_then(|chain| chain.levels.first().cloned()),
            revision: self.output.revision,
        }
    }
}

struct Capture {
    source: Node,
    layout: LayoutNode,
    scale: f32,
    palette: Palette,
    feedback: Feedback,
    texture: wgpu::Texture,
    image: ItemImage,
    mipmaps: Option<MipChain>,
    revision: u64,
    item_revision: u64,
}

impl Capture {
    fn material_view(&self) -> ItemTextureView {
        let image = self
            .mipmaps
            .as_ref()
            .map_or(&self.image, |mipmaps| &mipmaps.image);
        ItemTextureView {
            view: image.view.clone(),
            color_texture: self.texture.clone(),
            linear_premultiplied: self.mipmaps.is_some(),
            base_view: self
                .mipmaps
                .as_ref()
                .and_then(|chain| chain.levels.first().cloned()),
            allocation: image.allocation,
            revision: self.revision,
            readers: Arc::downgrade(&image.readers),
        }
    }
}

#[derive(PartialEq)]
struct Feedback {
    hovered: Option<Target>,
    pressed: Option<Target>,
    focused: Option<Target>,
    range: Option<f64>,
}

impl Feedback {
    fn within(input: &Interaction, path: &[usize]) -> Self {
        let below = |target: &Option<Target>| {
            target
                .as_ref()
                .filter(|target| target.path.starts_with(path))
                .cloned()
        };
        Self {
            hovered: below(&input.hovered),
            pressed: below(&input.pressed),
            focused: below(&input.focused),
            range: input
                .slider_preview_position()
                .and_then(|(target, value)| target.starts_with(path).then_some(value)),
        }
    }
}

struct Source<'a> {
    node: &'a Node,
    layout: &'a LayoutNode,
    path: Vec<usize>,
}

impl ItemTextures {
    pub(crate) fn clear(&mut self) {
        self.reset |= !self.current.is_empty() || !self.viewports.is_empty();
        self.current.clear();
        self.viewports.clear();
    }

    pub(crate) fn viewport_output(&self, node: &Node, path: &[usize]) -> Option<wgpu::Texture> {
        let output = self.viewports.get(node.scene_texture_source()?)?;
        (output.path == path).then(|| output.output.texture.clone())
    }

    fn view(&self, name: &str) -> Option<ItemTextureView> {
        self.current
            .get(name)
            .map(Capture::material_view)
            .or_else(|| self.viewports.get(name).map(ViewportCapture::material_view))
    }

    fn views(&self) -> ItemTextureViews {
        self.current
            .iter()
            .map(|(name, capture)| (name.clone(), capture.material_view()))
            .chain(
                self.viewports
                    .iter()
                    .map(|(name, capture)| (name.clone(), capture.material_view())),
            )
            .collect()
    }
}

fn item_references(node: &Node, augment_slots: usize) -> BTreeSet<&str> {
    let mut names = BTreeSet::new();
    if let Some(Value::SceneEffects(effects)) = node.properties.get("scene-effects") {
        names.extend(effects.item_sources());
    }
    if let Some(Value::SceneCaptures(captures)) = node.properties.get("scene-captures") {
        for style in captures
            .values()
            .filter_map(|capture| capture.override_material.as_ref())
        {
            let material = style.material();
            names.extend(material.base_item.as_ref().map(|item| item.source()));
            if let Some(shader) = &material.shader {
                names.extend(shader.item_sources());
            }
        }
        if captures
            .values()
            .filter_map(|capture| capture.augment.as_ref())
            .any(|augment| {
                augment
                    .item_sources(augment.textures.len())
                    .next()
                    .is_some()
            })
        {
            let mut pending = Vec::new();
            if let Some(Value::SceneEffects(effects)) = node.properties.get("scene-effects") {
                pending.extend(effects.capture_sources());
            }
            if let Some(Value::SceneObjects(model)) = node.properties.get("model") {
                for object in model.iter() {
                    if let xrack_runtime::scene3d::Mesh3D::Imported(mesh) = &object.mesh {
                        pending.extend(mesh.capture_sources());
                    }
                }
            }
            for shader in captures
                .values()
                .filter_map(|capture| capture.override_material.as_ref())
                .filter_map(|style| style.material().shader.as_ref())
            {
                pending
                    .extend(shader.capture_sources(xrack_runtime::material_shader::MAX_TEXTURES));
            }
            let mut seen = BTreeSet::new();
            while let Some(read) = pending.pop() {
                if read.attachment.material_data_slot().is_none()
                    || !seen.insert(read.name.as_ref())
                {
                    continue;
                }
                if let Some(augment) = captures
                    .get(read.name.as_ref())
                    .and_then(|capture| capture.augment.as_ref())
                {
                    names.extend(augment.item_sources(augment_slots));
                    pending.extend(augment.capture_sources(augment_slots));
                }
            }
        }
    }
    if let Some(Value::SceneObjects(model)) = node.properties.get("model") {
        for object in model.iter() {
            if let xrack_runtime::scene3d::Mesh3D::Imported(mesh) = &object.mesh {
                names.extend(mesh.item_sources().iter().map(String::as_str));
            }
        }
    }
    names
}

fn collect_visible_references<'a>(
    node: &'a Node,
    layout: &'a LayoutNode,
    names: &mut BTreeSet<&'a str>,
    augment_slots: usize,
) {
    if !layout.visible
        || node.scene_texture_only()
        || !(node.contains_scene_feature(SceneFeature::SceneViewport)
            || node.contains_scene_feature(SceneFeature::SceneImageTexture))
    {
        return;
    }
    names.extend(node.image_texture_input());
    if node.kind == "SceneView3D" {
        names.extend(item_references(node, augment_slots));
    }
    for (_, child, placed) in layout.children_in_paint_order(node) {
        collect_visible_references(child, placed, names, augment_slots);
    }
}

fn source_references<'a>(
    node: &'a Node,
    layout: &'a LayoutNode,
    augment_slots: usize,
) -> BTreeSet<&'a str> {
    if node.kind == "SceneView3D" {
        return item_references(node, augment_slots);
    }
    let mut names = BTreeSet::new();
    for (_, child, placed) in layout.children_in_paint_order(node) {
        collect_visible_references(child, placed, &mut names, augment_slots);
    }
    names
}

fn prepare_order<'a>(
    sources: BTreeMap<&'a str, Option<Source<'a>>>,
    mut wanted: BTreeSet<&'a str>,
    augment_slots: usize,
) -> Vec<(&'a str, Source<'a>)> {
    let mut pending: Vec<_> = wanted.iter().copied().collect();
    while let Some(name) = pending.pop() {
        if let Some(Some(source)) = sources.get(name) {
            for dependency in source_references(source.node, source.layout, augment_slots) {
                if wanted.insert(dependency) {
                    pending.push(dependency);
                }
            }
        }
    }
    let mut sources: Vec<_> = sources
        .into_iter()
        .filter(|(name, _)| wanted.contains(name))
        .filter_map(|(name, source)| Some((name, Some(source?))))
        .collect();
    let indices: BTreeMap<_, _> = sources
        .iter()
        .enumerate()
        .map(|(index, (name, _))| (*name, index))
        .collect();
    let dependencies: Vec<Vec<_>> = sources
        .iter()
        .map(|(_, source)| {
            source
                .as_ref()
                .map(|source| {
                    source_references(source.node, source.layout, augment_slots)
                        .into_iter()
                        .filter_map(|name| indices.get(name).copied())
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect();
    capture_postorder(sources.len(), |source, index| {
        dependencies[source].get(index).copied()
    })
    .into_iter()
    .filter_map(|index| {
        let (name, source) = &mut sources[index];
        Some((*name, source.take()?))
    })
    .collect()
}

pub(super) fn visit<'a>(
    node: &'a Node,
    layout: &'a LayoutNode,
    path: &mut Vec<usize>,
    feature: SceneFeature,
    apply: &mut impl FnMut(&'a Node, &'a LayoutNode, &[usize]),
) {
    if !layout.visible || !node.contains_scene_feature(feature) {
        return;
    }
    apply(node, layout, path);
    for (index, child, placed) in layout.children_in_paint_order(node) {
        path.push(index);
        visit(child, placed, path, feature, apply);
        path.pop();
    }
}

impl Renderer {
    pub(crate) fn prepare_scene_item_textures(
        &mut self,
        root: &Node,
        layout: &LayoutNode,
        frame: PaintFrame<'_>,
    ) -> Result<bool, String> {
        let mut cache = std::mem::take(&mut self.scene_item_textures);
        let inline = self.inline_target.take();
        let result = self.capture_scene_item_textures(root, layout, frame, &mut cache);
        self.inline_target = inline;
        self.scene_item_textures = cache;
        result
    }

    fn capture_scene_item_textures(
        &mut self,
        root: &Node,
        layout: &LayoutNode,
        frame: PaintFrame<'_>,
        cache: &mut ItemTextures,
    ) -> Result<bool, String> {
        use SceneFeature::SceneTextureSource;

        let augment_slots = super::material::augment_texture_slots(
            &self.device,
            super::material::BASE_MATERIAL_TEXTURES,
        );
        let mut wanted = BTreeSet::new();
        if root.contains_scene_feature(SceneTextureSource) {
            collect_visible_references(root, layout, &mut wanted, augment_slots);
        }
        let mut sources = BTreeMap::new();
        if !wanted.is_empty() {
            visit(
                root,
                layout,
                &mut Vec::new(),
                SceneTextureSource,
                &mut |node, layout, path| {
                    if let Some(name) = node.scene_texture_source() {
                        let source = Source {
                            node,
                            layout,
                            path: path.to_vec(),
                        };
                        sources
                            .entry(name)
                            .and_modify(|value| *value = None)
                            .or_insert(Some(source));
                    }
                },
            );
        }
        let mut previous = std::mem::take(&mut cache.current);
        let changed = std::mem::take(&mut cache.reset);
        let result = self.update_scene_item_textures(
            root,
            frame,
            cache,
            &mut previous,
            prepare_order(sources, wanted, augment_slots),
            changed,
        );
        if result.is_err() {
            cache.reset = true;
        }
        result
    }

    fn update_scene_item_textures(
        &mut self,
        root: &Node,
        frame: PaintFrame<'_>,
        cache: &mut ItemTextures,
        previous: &mut BTreeMap<String, Capture>,
        sources: Vec<(&str, Source<'_>)>,
        mut changed: bool,
    ) -> Result<bool, String> {
        let mut budget = MAX_BYTES;
        let mut previous_viewports = std::mem::take(&mut cache.viewports);
        let mut staged_views = false;
        let source_frame = PaintFrame {
            damage: None,
            transitions: None,
            cleared_background: None,
            indicator_phase: None,
            prepared_layout: None,
            ..frame
        };
        // Reserve each ordinary capture first. An optional mip chain must
        // not evict a different named panel.
        let mut accepted = Vec::new();
        for (name, source) in sources {
            if accepted.len() == MAX_SOURCES {
                break;
            }
            let size = if source.node.kind == "SceneView3D" {
                let (_, content, _) = scene_view_content(source.node, source.layout.bounds);
                let scale = source.layout.world_transform().max_scale() * frame.scale;
                if content.width <= 0.0
                    || content.height <= 0.0
                    || !scale.is_finite()
                    || scale <= 0.0
                {
                    continue;
                }
                super::texture_extent(
                    source.node,
                    [content.width, content.height].map(|edge| (edge * scale).ceil() as u32),
                    self.device.limits().max_texture_dimension_2d,
                )
            } else {
                if source
                    .node
                    .contains_scene_feature(SceneFeature::SceneViewport)
                {
                    continue;
                }
                [source.layout.bounds.width, source.layout.bounds.height]
                    .map(|edge| ((edge * frame.scale).ceil() + 1.0) as u32)
            };
            if !size
                .iter()
                .all(|&edge| (2..=MAX_EDGE as u32).contains(&edge))
            {
                continue;
            }
            let (format, bytes) = if source.node.kind == "SceneView3D" {
                let format = viewport_format(&self.device, source.node);
                (format, viewport_bytes(size, format))
            } else {
                (COLOR_FORMAT, crate::transient_textures::bytes(size))
            };
            if bytes <= budget {
                accepted.push((name, source, size, format));
                budget -= bytes;
            }
        }
        let augment_slots = super::material::augment_texture_slots(
            &self.device,
            super::material::BASE_MATERIAL_TEXTURES,
        );
        for (
            name,
            Source {
                node,
                layout,
                mut path,
            },
            size,
            format,
        ) in accepted
        {
            let mut wants_mips = false;
            if node.boolean("texture-mipmaps", false) {
                let bytes = chain_bytes(size, format);
                wants_mips = bytes <= budget;
                if wants_mips {
                    budget -= bytes;
                }
            }
            let references = source_references(node, layout, augment_slots);
            let mut item_revision = 0;
            if node.kind == "SceneView3D" || !references.is_empty() {
                // Publish only producers already recorded in this frame. A
                // back edge, including a self reference, sees a missing input.
                staged_views = true;
                let dependencies: ItemTextureViews = references
                    .into_iter()
                    .filter_map(|name| Some((name.to_owned(), cache.view(name)?)))
                    .collect();
                let mut hash = DefaultHasher::new();
                for (name, image) in &dependencies {
                    (name, image.allocation, image.revision).hash(&mut hash);
                }
                item_revision = if dependencies.is_empty() {
                    0
                } else {
                    hash.finish().max(1)
                };
                self.scene3d.set_item_textures(item_revision, dependencies);
            }
            if node.kind == "SceneView3D" {
                let old = previous_viewports.remove(name);
                let Some(output) = self.capture_viewport_output(
                    root,
                    node,
                    layout,
                    &path,
                    (size, format),
                    frame.scale,
                )?
                else {
                    changed |= old.is_some();
                    continue;
                };
                let mut capture = match old {
                    Some(old)
                        if old.output.revision == output.revision
                            && old.output.texture == output.texture
                            && old.mipmaps.is_some() == wants_mips =>
                    {
                        self.scene3d.stats.item_texture_reuses += 1;
                        old
                    }
                    old => {
                        changed = true;
                        self.scene3d.stats.item_texture_captures += 1;
                        let (existing_view, old_mipmaps) = old.map_or((None, None), |old| {
                            (
                                (old.output.texture == output.texture)
                                    .then_some((old.allocation, old.view)),
                                old.mipmaps,
                            )
                        });
                        let (allocation, view) = existing_view.unwrap_or_else(|| {
                            cache.next_allocation = cache.next_allocation.wrapping_add(1).max(1);
                            (
                                cache.next_allocation,
                                output.texture.create_view(&Default::default()),
                            )
                        });
                        let mipmaps = wants_mips.then(|| {
                            let chain = old_mipmaps
                                .filter(|chain| {
                                    chain.size == size
                                        && chain.format == format
                                        && !chain.image.is_pinned()
                                })
                                .unwrap_or_else(|| {
                                    self.scene3d.stats.item_texture_mip_allocations += 1;
                                    MipChain::new(
                                        &self.device,
                                        size,
                                        format,
                                        &mut cache.next_allocation,
                                    )
                                });
                            chain.refresh(self, &view);
                            chain
                        });
                        ViewportCapture {
                            path: Vec::new(),
                            output,
                            allocation,
                            view,
                            mipmaps,
                        }
                    }
                };
                capture.path = path;
                cache.viewports.insert(name.to_owned(), capture);
                continue;
            }
            let feedback = Feedback::within(frame.input, &path);
            let capture = match previous.remove(name) {
                Some(old)
                    if old.scale == frame.scale
                        && old.palette == frame.theme
                        && old.feedback == feedback
                        && old.item_revision == item_revision
                        && old.mipmaps.is_some() == wants_mips
                        && old.source == *node
                        && old.layout == *layout =>
                {
                    self.scene3d.stats.item_texture_reuses += 1;
                    old
                }
                old => {
                    let Some(texture) = self.capture_scene_item(
                        layout.bounds,
                        frame.scale,
                        old.as_ref()
                            .filter(|capture| !capture.image.is_pinned())
                            .map(|capture| &capture.texture),
                        |renderer| {
                            renderer.paint_subtree(node, layout, &mut path, source_frame, false)
                        },
                    )?
                    else {
                        changed |= old.is_some();
                        continue;
                    };
                    changed = true;
                    let (image, old_mipmaps) = old.map_or((None, None), |entry| {
                        (
                            (entry.texture == texture).then_some(entry.image),
                            entry.mipmaps,
                        )
                    });
                    let image = image.unwrap_or_else(|| {
                        self.scene3d.stats.item_texture_allocations += 1;
                        ItemImage::new(&texture, &mut cache.next_allocation)
                    });
                    let mipmaps = wants_mips.then(|| {
                        let size = [texture.width(), texture.height()];
                        let chain = old_mipmaps
                            .filter(|chain| {
                                chain.size == size
                                    && chain.format == format
                                    && !chain.image.is_pinned()
                            })
                            .unwrap_or_else(|| {
                                self.scene3d.stats.item_texture_mip_allocations += 1;
                                MipChain::new(
                                    &self.device,
                                    size,
                                    format,
                                    &mut cache.next_allocation,
                                )
                            });
                        chain.refresh(self, &image.view);
                        chain
                    });
                    self.scene3d.stats.item_texture_captures += 1;
                    Capture {
                        source: node.clone(),
                        layout: layout.clone(),
                        scale: frame.scale,
                        palette: frame.theme,
                        feedback,
                        texture,
                        image,
                        mipmaps,
                        revision: cache.revision.wrapping_add(1).max(1),
                        item_revision,
                    }
                }
            };
            cache.current.insert(name.to_owned(), capture);
        }
        changed |= !previous.is_empty() || !previous_viewports.is_empty();
        if changed {
            cache.revision = cache.revision.wrapping_add(1).max(1);
        }
        if changed || staged_views {
            self.scene3d
                .set_item_textures(cache.revision, cache.views());
        }
        Ok(changed)
    }

    fn capture_viewport_output(
        &mut self,
        root: &Node,
        node: &Node,
        layout: &LayoutNode,
        path: &[usize],
        (pixels, format): ([u32; 2], wgpu::TextureFormat),
        scale: f32,
    ) -> Result<Option<ViewOutput>, String> {
        let mut scope = [None, None];
        let mut ancestor = root;
        for &slot in path {
            let Some(child) = ancestor.child_at(slot) else {
                return Ok(None);
            };
            ancestor = child;
            scope[0] = child.routing_row_key().or(scope[0]);
            scope[1] = child.dynamic_mount_key().or(scope[1]);
        }
        let (camera, content, scissor) = scene_view_content(node, layout.bounds);
        let logical = [content.width, content.height];
        self.render_scene_output(
            node,
            PaintContext {
                bounds: content,
                scale,
                data: "",
                revision: 0,
            },
            camera,
            Owner {
                path,
                generation: self.scene_generation,
                scope,
                id: node.element_id(),
                logical_size: logical,
                revision: node.int("scene-effect-history-revision").unwrap_or(0),
            },
            ViewSize {
                pixels,
                logical,
                scissor,
                format,
            },
            scale * layout.world_transform().max_scale(),
            true,
        )
    }
}
