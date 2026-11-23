//! Reuse prepared 3D geometry and spatial queries throughout one program.
use std::{cell::RefCell, collections::VecDeque, sync::Arc};

use crate::{
    RecordList, VecModel,
    scene_animation::MeshAsset,
    scene_hierarchy::{
        SceneModel, SceneNode, model_cameras, model_nodes, record_cameras, record_objects,
    },
    scene_particle_mesh::{ParticleMeshSurface, ParticleSurfaceCache},
};

const CAPACITY: usize = 6;

struct Projection {
    source: (u64, Option<u64>),
    revision: u64,
    schema: Box<str>,
    renderables: bool,
    scene: Arc<SceneModel>,
}

/// Short-lived projections for a single compiled module or interpreted program.
///
/// Generated code uses one stable scene projector per record schema and one
/// immutable mesh resolver per cache. No model handles are kept alive here.
#[doc(hidden)]
#[derive(Default)]
pub struct SceneProjectionCache {
    projections: RefCell<VecDeque<Projection>>,
    particle_surfaces: ParticleSurfaceCache,
}

impl SceneProjectionCache {
    pub const fn new() -> Self {
        Self {
            projections: RefCell::new(VecDeque::new()),
            particle_surfaces: ParticleSurfaceCache::new(),
        }
    }

    pub fn particle_surface(
        &self,
        key: &str,
        scale: [f32; 3],
        imported: impl FnOnce(&str) -> Option<MeshAsset>,
    ) -> Option<Arc<ParticleMeshSurface>> {
        self.particle_surfaces.surface(key, scale, imported)
    }

    pub fn model_nodes<T: Clone>(
        &self,
        model: &VecModel<T>,
        schema: &str,
        project: impl Fn(T) -> Option<SceneNode>,
    ) -> Arc<SceneModel> {
        self.get(model, schema, true, || model_nodes(model, project))
    }

    pub fn model_cameras<T: Clone>(
        &self,
        model: &VecModel<T>,
        schema: &str,
        project: impl Fn(T) -> Option<SceneNode>,
    ) -> Arc<SceneModel> {
        self.get(model, schema, false, || model_cameras(model, project))
    }

    pub fn record_objects(
        &self,
        model: &RecordList,
        imported: impl Fn(&str) -> Option<MeshAsset>,
    ) -> Arc<SceneModel> {
        self.get(model.model(), model.name(), true, || {
            record_objects(model, imported)
        })
    }

    pub fn record_cameras(
        &self,
        model: &RecordList,
        imported: impl Fn(&str) -> Option<MeshAsset>,
    ) -> Arc<SceneModel> {
        self.get(model.model(), model.name(), false, || {
            record_cameras(model, imported)
        })
    }

    fn get<T>(
        &self,
        model: &VecModel<T>,
        schema: &str,
        renderables: bool,
        build: impl FnOnce() -> Arc<SceneModel>,
    ) -> Arc<SceneModel> {
        if !model.is_shared() {
            crate::update_profile::scene_projection(false);
            return build();
        }
        // Register the model dependency even when this binding gets a cache
        // hit prepared earlier by a different binding or a ray query.
        let (_, revision) = model.contents_stamp();
        let source = model.row_cache_identity();
        {
            let mut projections = self.projections.borrow_mut();
            let existing = projections.iter().position(|projection| {
                projection.source == source
                    && projection.revision == revision
                    && projection.schema.as_ref() == schema
                    && (projection.renderables || !renderables)
            });
            if let Some(projection) = existing.and_then(|index| projections.remove(index)) {
                let scene = projection.scene.clone();
                projections.push_front(projection);
                crate::update_profile::scene_projection(true);
                return scene;
            }
        }
        crate::update_profile::scene_projection(false);
        let scene = build();
        if model.edit_stamp().1 == revision {
            let mut projections = self.projections.borrow_mut();
            // A complete scene can serve camera-only queries as well.
            projections.retain(|projection| {
                projection.source != source
                    || projection.schema.as_ref() != schema
                    || (projection.revision == revision && projection.renderables && !renderables)
            });
            projections.truncate(CAPACITY - 1);
            projections.push_front(Projection {
                source,
                revision,
                schema: schema.into(),
                renderables,
                scene: scene.clone(),
            });
        }
        scene
    }
}
