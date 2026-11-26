//! Warm the existing scene GPU caches without drawing the prepared model.
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

use xrack_layout::LayoutNode;
use xrack_runtime::{
    Node, SceneFeature, Value,
    scene3d::{Mesh3D, SceneModel, ScenePreloadCursor, ScenePreloadStep},
};

use super::{SceneGpu, SceneRenderer, item_textures::visit};

const UPLOAD_STEPS_PER_FRAME: usize = 2;
const ROWS_PER_FRAME: usize = 512;
const LOD_POSES_PER_FRAME: usize = 1;

#[derive(Default)]
pub(super) struct Preloads {
    jobs: HashMap<usize, Job>,
}

struct Job {
    model: Weak<SceneModel>,
    meshes: ScenePreloadCursor,
    requested: bool,
}

impl SceneRenderer {
    pub(crate) fn preload_resources(
        &mut self,
        root: &Node,
        layout: &LayoutNode,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> bool {
        let feature = SceneFeature::ScenePreload;
        if !root.contains_scene_feature(feature) {
            self.preloads.jobs.clear();
            return false;
        }

        let mut work = false;
        let mut uploads = UPLOAD_STEPS_PER_FRAME;
        let mut rows = ROWS_PER_FRAME;
        let mut lod_poses = LOD_POSES_PER_FRAME;
        visit(root, layout, &mut Vec::new(), feature, &mut |node, _, _| {
            let Some(Value::SceneObjects(model)) = node.properties.get("preload-model") else {
                return;
            };
            let job = self
                .preloads
                .jobs
                .entry(Arc::as_ptr(model) as usize)
                .or_insert_with(|| Job {
                    model: Arc::downgrade(model),
                    meshes: ScenePreloadCursor::default(),
                    requested: false,
                });
            if std::mem::replace(&mut job.requested, true) {
                return;
            }

            while uploads != 0 && rows != 0 {
                let mesh = match job.meshes.advance(model, &mut lod_poses) {
                    Some(ScenePreloadStep::Mesh(mesh)) => Some(mesh),
                    Some(ScenePreloadStep::Skipped) => None,
                    Some(ScenePreloadStep::Deferred) | None => break,
                };
                rows -= 1;
                let Some(mesh @ Mesh3D::Imported(data)) = mesh else {
                    continue;
                };
                let scene = self.scene.get_or_insert_with(|| {
                    SceneGpu::new(
                        device,
                        queue,
                        &self.item_textures,
                        (
                            self.postprocess.effects.images.clone(),
                            self.postprocess.effects.projections.clone(),
                            self.native_anisotropy,
                        ),
                    )
                });
                let key = (Arc::as_ptr(data) as usize, false);
                if scene.meshes.imported.contains_key(&key) {
                    continue;
                }
                // Captures and live item textures are resolved by the consumer
                // when it draws. Warm the static buffers and texture data here.
                scene.meshes.materials.captures.clear();
                scene.meshes.materials.items.clone_from(&self.item_textures);
                let images = &self.postprocess.effects.images;
                let image_uploads = images.borrow().uploads;
                let meshes = scene.meshes.uploads;
                scene.meshes.get(device, queue, mesh);
                // A rebuilt SceneModel can have new material bindings with
                // the same vertex streams and images. Budget only transfers.
                if meshes != scene.meshes.uploads || image_uploads != images.borrow().uploads {
                    uploads -= 1;
                    self.stats.preloaded_meshes += 1;
                }
            }
            work |= job.meshes.pending(model);
        });
        self.preloads
            .jobs
            .retain(|_, job| std::mem::take(&mut job.requested) && job.model.strong_count() != 0);
        work
    }
}
