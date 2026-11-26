//! Reuse prepared 3D geometry and spatial queries throughout one program.
use std::{cell::RefCell, collections::VecDeque, sync::Arc};

use crate::{
    Quaternion3D, RecordList, RotationTrack3D, Value, VecModel,
    scene_animation::{AnimatedMesh, MeshAsset},
    scene_authored_mesh::SceneMeshInputs,
    scene_camera::{CameraMatrixCache, SceneCameraSettings},
    scene_hierarchy::{
        SceneModel, SceneNode, prepare_model, prepare_record_scene, rest_mesh_objects,
    },
    scene_instance_table::{InstanceTable3D, InstanceTableRow},
    scene_particle_bursts::{ParticleBurstDefinition, ParticleBurstSchedule},
    scene_particle_mesh::{ParticleMeshSurface, ParticleSurfaceCache},
    scene_particle_track::{ParticleEmissionAnimation, ParticleEmissionTrack},
    scene3d::{MAX_IMPORTED_VERTICES, Mesh3D, MeshImage},
};

const CAPACITY: usize = 6;

struct Projection {
    source: (u64, Option<u64>),
    revision: u64,
    schema: Box<str>,
    renderables: bool,
    mesh_inputs: SceneMeshInputs,
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
    camera_matrices: CameraMatrixCache,
    instance_tables: ModelProjectionCache<InstanceTable3D>,
    rotation_tracks: ModelProjectionCache<RotationTrack3D>,
    emission_tracks: ModelProjectionCache<Option<ParticleEmissionTrack>>,
    particle_bursts: ModelProjectionCache<Option<ParticleBurstSchedule>>,
    animation_tracks: RefCell<VecDeque<AnimationTrackProjection>>,
    particle_surfaces: ParticleSurfaceCache,
    particle_models: RefCell<VecDeque<CachedParticleModel>>,
}

#[derive(Default)]
struct ModelProjectionCache<V>(RefCell<VecDeque<CachedModelProjection<V>>>);

struct CachedModelProjection<V> {
    source: (u64, Option<u64>),
    revision: u64,
    value: V,
}

impl<V: Clone> ModelProjectionCache<V> {
    const fn new() -> Self {
        Self(RefCell::new(VecDeque::new()))
    }

    fn get<T>(&self, model: &VecModel<T>, project: impl FnOnce() -> V) -> V {
        if !model.is_shared() {
            return project();
        }
        let (_, revision) = model.contents_stamp();
        let source = model.row_cache_identity();
        {
            let mut cache = self.0.borrow_mut();
            if let Some(index) = cache
                .iter()
                .position(|entry| entry.source == source && entry.revision == revision)
                && let Some(entry) = cache.remove(index)
            {
                let value = entry.value.clone();
                cache.push_front(entry);
                return value;
            }
        }
        let value = project();
        let mut cache = self.0.borrow_mut();
        cache.retain(|entry| entry.source != source);
        cache.truncate(CAPACITY - 1);
        cache.push_front(CachedModelProjection {
            source,
            revision,
            value: value.clone(),
        });
        value
    }
}

struct AnimationTrackProjection {
    rig: Arc<AnimatedMesh>,
    settings: ParticleEmissionAnimation,
    track: Option<ParticleEmissionTrack>,
}

struct CachedParticleModel {
    key: String,
    meshes: Option<Arc<[Mesh3D]>>,
}

impl SceneProjectionCache {
    pub const fn new() -> Self {
        Self {
            projections: RefCell::new(VecDeque::new()),
            camera_matrices: CameraMatrixCache::new(),
            instance_tables: ModelProjectionCache::new(),
            rotation_tracks: ModelProjectionCache::new(),
            emission_tracks: ModelProjectionCache::new(),
            particle_bursts: ModelProjectionCache::new(),
            animation_tracks: RefCell::new(VecDeque::new()),
            particle_surfaces: ParticleSurfaceCache::new(),
            particle_models: RefCell::new(VecDeque::new()),
        }
    }

    pub fn custom_camera(
        &self,
        rows: [[f64; 4]; 4],
        clip_space: &str,
    ) -> Option<SceneCameraSettings> {
        self.camera_matrices.get(rows, clip_space)
    }

    pub fn instance_table<T>(
        &self,
        model: &VecModel<T>,
        project: impl FnMut(&T) -> InstanceTableRow,
    ) -> InstanceTable3D {
        self.instance_tables
            .get(model, || InstanceTable3D::from_model(model, project))
    }

    pub fn record_instance_table(&self, model: &RecordList) -> InstanceTable3D {
        self.instance_table(model.model(), InstanceTableRow::from_record)
    }

    pub fn rotation_track<T>(
        &self,
        model: &VecModel<T>,
        project: impl FnMut(&T) -> Option<(f64, Quaternion3D)>,
    ) -> RotationTrack3D {
        self.rotation_tracks
            .get(model, || RotationTrack3D::from_model(model, project))
    }

    pub fn record_rotation_track(&self, model: &RecordList) -> RotationTrack3D {
        self.rotation_track(model.model(), |row| {
            let (Value::Float(time), Value::Quaternion3D(orientation)) =
                (row.field("time")?, row.field("orientation")?)
            else {
                return None;
            };
            Some((*time, *orientation))
        })
    }

    pub fn particle_burst_schedule<T>(
        &self,
        rows: &VecModel<T>,
        project: impl FnMut(&T) -> Option<ParticleBurstDefinition>,
    ) -> Option<ParticleBurstSchedule> {
        self.particle_bursts
            .get(rows, || ParticleBurstSchedule::from_model(rows, project))
    }

    pub(crate) fn record_particle_bursts(
        &self,
        rows: &RecordList,
    ) -> Option<ParticleBurstSchedule> {
        self.particle_burst_schedule(rows.model(), ParticleBurstDefinition::from_record)
    }

    pub fn particle_emission_track<T>(
        &self,
        rows: &VecModel<T>,
        project: impl FnMut(&T) -> Option<(f64, crate::Point3D, Quaternion3D)>,
    ) -> Option<ParticleEmissionTrack> {
        self.emission_tracks
            .get(rows, || ParticleEmissionTrack::from_model(rows, project))
    }

    pub fn record_emission_track(&self, rows: &RecordList) -> Option<ParticleEmissionTrack> {
        self.particle_emission_track(rows.model(), |row| {
            let (Value::Float(time), Value::Point3D(position)) =
                (row.field("time")?, row.field("position")?)
            else {
                return None;
            };
            let rotation = match row.field("orientation") {
                Some(Value::Quaternion3D(value)) => *value,
                None => Quaternion3D::IDENTITY,
                _ => return None,
            };
            Some((*time, *position, rotation))
        })
    }

    pub fn particle_animation_track(
        &self,
        asset: &MeshAsset,
        settings: ParticleEmissionAnimation,
    ) -> Option<ParticleEmissionTrack> {
        let rig = asset.animated_primary()?;
        let mut tracks = self.animation_tracks.borrow_mut();
        if let Some(index) = tracks
            .iter()
            .position(|entry| Arc::ptr_eq(rig, &entry.rig) && entry.settings == settings)
            && let Some(entry) = tracks.remove(index)
        {
            let track = entry.track.clone();
            tracks.push_front(entry);
            return track;
        }
        let track = rig.emission_track(&settings);
        tracks.truncate(CAPACITY - 1);
        tracks.push_front(AnimationTrackProjection {
            rig: rig.clone(),
            settings,
            track: track.clone(),
        });
        track
    }

    pub fn particle_surface(
        &self,
        key: &str,
        scale: [f32; 3],
        imported: impl FnOnce(&str) -> Option<MeshAsset>,
    ) -> Option<Arc<ParticleMeshSurface>> {
        self.particle_surfaces.surface(key, scale, imported)
    }

    pub fn particle_model(
        &self,
        key: &str,
        imported: impl FnOnce(&str) -> Option<MeshAsset>,
    ) -> Option<Arc<[Mesh3D]>> {
        {
            let mut models = self.particle_models.borrow_mut();
            if let Some(index) = models.iter().position(|model| model.key == key)
                && let Some(model) = models.remove(index)
            {
                let result = model.meshes.clone();
                models.push_front(model);
                return result;
            }
        }
        let meshes = MeshAsset::resolve(key, imported).and_then(|asset| {
            let asset = asset.select_material_variant("");
            if let MeshAsset::Static(mesh) = asset {
                return Some(Arc::from([mesh]));
            }
            let objects = rest_mesh_objects(&asset);
            if objects.is_empty()
                || objects
                    .iter()
                    .map(|object| object.mesh.vertices().len())
                    .sum::<usize>()
                    > MAX_IMPORTED_VERTICES
            {
                return None;
            }
            objects
                .iter()
                .map(|object| object.particle_prototype())
                .collect::<Option<Arc<[_]>>>()
        });
        let mut models = self.particle_models.borrow_mut();
        models.truncate(CAPACITY - 1);
        models.push_front(CachedParticleModel {
            key: key.into(),
            meshes: meshes.clone(),
        });
        meshes
    }

    pub fn model_nodes<T: Clone>(
        &self,
        model: VecModel<T>,
        schema: &str,
        project: impl Fn(T, &SceneMeshInputs) -> Option<SceneNode>,
    ) -> Arc<SceneModel> {
        self.get(&model, schema, true, |inputs, observe_rows| {
            prepare_model(&model, |row| project(row, inputs), true, observe_rows)
        })
    }

    pub fn model_cameras<T: Clone>(
        &self,
        model: VecModel<T>,
        schema: &str,
        project: impl Fn(T, &SceneMeshInputs) -> Option<SceneNode>,
    ) -> Arc<SceneModel> {
        self.get(&model, schema, false, |inputs, observe_rows| {
            prepare_model(&model, |row| project(row, inputs), false, observe_rows)
        })
    }

    /// The interpreter passes its evaluated argument; other handles to a
    /// live record list keep its rows observable.
    pub fn record_objects(
        &self,
        model: &RecordList,
        imported: impl Fn(&str) -> Option<MeshAsset>,
        textures: impl Fn(&str) -> Option<Arc<MeshImage>>,
        shaders: impl Fn(&str) -> Option<Arc<str>>,
    ) -> Arc<SceneModel> {
        self.get(model.model(), model.name(), true, |inputs, observe_rows| {
            prepare_record_scene(
                model,
                imported,
                (textures, shaders),
                (true, observe_rows),
                inputs,
                &self.camera_matrices,
            )
        })
    }

    pub fn record_cameras(
        &self,
        model: &RecordList,
        imported: impl Fn(&str) -> Option<MeshAsset>,
    ) -> Arc<SceneModel> {
        self.get(
            model.model(),
            model.name(),
            false,
            |inputs, observe_rows| {
                prepare_record_scene(
                    model,
                    imported,
                    (|_| None, |_| None),
                    (false, observe_rows),
                    inputs,
                    &self.camera_matrices,
                )
            },
        )
    }

    fn get<T>(
        &self,
        model: &VecModel<T>,
        schema: &str,
        renderables: bool,
        build: impl FnOnce(&SceneMeshInputs, bool) -> Arc<SceneModel>,
    ) -> Arc<SceneModel> {
        if !model.is_shared() {
            crate::update_profile::scene_projection(false);
            // Its dependency signals would be dropped with the evaluated
            // argument, immediately invalidating the prepared scene.
            return build(&SceneMeshInputs::default(), false);
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
                    && projection.mesh_inputs.current()
            });
            if let Some(projection) = existing.and_then(|index| projections.remove(index)) {
                let scene = projection.scene.clone();
                projections.push_front(projection);
                crate::update_profile::scene_projection(true);
                return scene;
            }
        }
        crate::update_profile::scene_projection(false);
        let mesh_inputs = SceneMeshInputs::default();
        let scene = build(&mesh_inputs, true);
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
                mesh_inputs,
                scene: scene.clone(),
            });
        }
        scene
    }
}
