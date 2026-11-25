//! Offscreen captures of window-local 2D sources used by materials and effects.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Weak},
};

use xrack_layout::{Interaction, LayoutNode, Target};
use xrack_runtime::{Node, SceneFeature, Value};

use crate::{PaintFrame, Palette, Renderer};

const MAX_SOURCES: usize = 8;
const MAX_EDGE: f32 = 2048.0;
const MAX_BYTES: u64 = 16 * 1024 * 1024;

pub(super) type ItemTextureViews = BTreeMap<String, ItemTextureView>;

#[derive(Clone)]
pub(super) struct ItemTextureView {
    pub view: wgpu::TextureView,
    pub linear_premultiplied: bool,
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
    levels: Vec<wgpu::TextureView>,
    image: ItemImage,
}

impl MipChain {
    fn new(device: &wgpu::Device, size: [u32; 2], next_allocation: &mut u64) -> Self {
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
            format: super::COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let levels = (0..mip_level_count)
            .map(|base_mip_level| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        Self {
            size,
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
        filter.decode_item(&renderer.device, encoder, (source, &self.levels[0]));
        for (previous, levels) in self.levels.windows(2).enumerate() {
            let size = mip_extent(self.size, previous + 1);
            filter.resolve(
                &renderer.device,
                encoder,
                (&levels[0], &levels[1]),
                size,
                super::COLOR_FORMAT,
            );
        }
        renderer.scene3d.stats.item_texture_mip_passes += self.levels.len() as u64;
    }
}

fn mip_count(size: [u32; 2]) -> u32 {
    size[0].max(size[1]).ilog2() + 1
}

fn mip_extent(size: [u32; 2], level: usize) -> [u32; 2] {
    size.map(|side| (side >> level).max(1))
}

fn texture_bytes(size: [u32; 2]) -> u64 {
    size.into_iter().map(u64::from).product::<u64>() * 4
}

fn chain_bytes(size: [u32; 2]) -> u64 {
    (0..mip_count(size) as usize)
        .map(|level| texture_bytes(mip_extent(size, level)))
        .sum()
}

#[derive(Default)]
pub(crate) struct ItemTextures {
    current: BTreeMap<String, Capture>,
    reset: bool,
    revision: u64,
    next_allocation: u64,
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
}

impl Capture {
    fn material_view(&self) -> ItemTextureView {
        let image = self
            .mipmaps
            .as_ref()
            .map_or(&self.image, |mipmaps| &mipmaps.image);
        ItemTextureView {
            view: image.view.clone(),
            linear_premultiplied: self.mipmaps.is_some(),
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
        self.reset |= !self.current.is_empty();
        self.current.clear();
    }
}

fn visit<'a>(
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
        use SceneFeature::{SceneTextureSource, SceneViewport};

        let mut wanted = BTreeSet::new();
        if root.contains_scene_feature(SceneTextureSource) {
            visit(
                root,
                layout,
                &mut Vec::new(),
                SceneViewport,
                &mut |node, _, _| {
                    if let Some(Value::SceneEffects(effects)) = node.properties.get("scene-effects")
                    {
                        wanted.extend(effects.item_sources());
                    }
                    if let Some(Value::SceneCaptures(captures)) =
                        node.properties.get("scene-captures")
                    {
                        for style in captures
                            .values()
                            .filter_map(|capture| capture.override_material.as_ref())
                        {
                            let material = style.material();
                            wanted.extend(material.base_item.as_deref());
                            if let Some(shader) = &material.shader {
                                wanted.extend(shader.item_sources());
                            }
                        }
                    }
                    if let Some(Value::SceneObjects(model)) = node.properties.get("model") {
                        for object in model.iter() {
                            for material in object.mesh.materials() {
                                wanted.extend(material.base_item.as_deref());
                                if let Some(shader) = &material.shader {
                                    wanted.extend(shader.item_sources());
                                }
                            }
                        }
                    }
                },
            );
        }
        let mut sources = BTreeMap::new();
        if !wanted.is_empty() {
            visit(
                root,
                layout,
                &mut Vec::new(),
                SceneTextureSource,
                &mut |node, layout, path| {
                    if node.kind == "Rectangle"
                        && let Some(name) = node
                            .text("texture-source")
                            .filter(|name| wanted.contains(name))
                    {
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
        let result = self.update_scene_item_textures(frame, cache, &mut previous, sources, changed);
        if result.is_err() {
            cache.reset = true;
        }
        result
    }

    fn update_scene_item_textures(
        &mut self,
        frame: PaintFrame<'_>,
        cache: &mut ItemTextures,
        previous: &mut BTreeMap<String, Capture>,
        sources: BTreeMap<&str, Option<Source<'_>>>,
        mut changed: bool,
    ) -> Result<bool, String> {
        let mut budget = MAX_BYTES;
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
            let Some(source) = source else {
                continue;
            };
            if accepted.len() == MAX_SOURCES {
                break;
            }
            let extent = [source.layout.bounds.width, source.layout.bounds.height]
                .map(|edge| (edge * frame.scale).ceil() + 1.0);
            if source
                .node
                .contains_scene_feature(SceneFeature::SceneViewport)
                || !extent.iter().all(|&edge| (2.0..=MAX_EDGE).contains(&edge))
            {
                continue;
            }
            let size = extent.map(|edge| edge as u32);
            let bytes = texture_bytes(size);
            if bytes <= budget {
                accepted.push((name, source, size));
                budget -= bytes;
            }
        }
        for (
            name,
            Source {
                node,
                layout,
                mut path,
            },
            size,
        ) in accepted
        {
            let mut wants_mips = false;
            if node.boolean("texture-mipmaps", false) {
                let bytes = chain_bytes(size);
                wants_mips = bytes <= budget;
                if wants_mips {
                    budget -= bytes;
                }
            }
            let feedback = Feedback::within(frame.input, &path);
            let capture = match previous.remove(name) {
                Some(old)
                    if old.scale == frame.scale
                        && old.palette == frame.theme
                        && old.feedback == feedback
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
                            .filter(|chain| chain.size == size && !chain.image.is_pinned())
                            .unwrap_or_else(|| {
                                self.scene3d.stats.item_texture_mip_allocations += 1;
                                MipChain::new(&self.device, size, &mut cache.next_allocation)
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
                    }
                }
            };
            cache.current.insert(name.to_owned(), capture);
        }
        changed |= !previous.is_empty();
        if changed {
            cache.revision = cache.revision.wrapping_add(1).max(1);
            self.scene3d.set_item_textures(
                cache.revision,
                cache
                    .current
                    .iter()
                    .map(|(key, capture)| (key.clone(), capture.material_view()))
                    .collect(),
            );
        }
        Ok(changed)
    }
}
