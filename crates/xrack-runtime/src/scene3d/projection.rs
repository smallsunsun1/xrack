//! Reuse prepared 3D geometry and spatial queries throughout one program.
use std::{
    cell::RefCell,
    collections::VecDeque,
    sync::{Arc, Weak},
};

use crate::{
    Quaternion3D, RecordList, RotationTrack3D, Value, VecModel,
    scene3d::camera::{CameraMatrixCache, SceneCameraSettings},
    scene3d::hierarchy::{
        SceneModel, SceneNode, prepare_model, prepare_record_scene, rest_mesh_objects,
    },
    scene3d::instance_table::{InstanceTable3D, InstanceTableRow},
    scene3d::mesh::animation::{AnimatedMesh, MeshAsset},
    scene3d::mesh::authored::SceneMeshInputs,
    scene3d::particles::bursts::{ParticleBurstDefinition, ParticleBurstSchedule},
    scene3d::particles::surface::{ParticleMeshSurface, ParticleSurfaceCache},
    scene3d::particles::track::{ParticleEmissionAnimation, ParticleEmissionTrack},
    scene3d::{
        MAX_IMPORTED_VERTICES, Mesh3D, ParticleInstanceModel, ParticleModelSource,
        ParticlePointCloud, SceneObject, TextureAsset3D,
    },
};

const CAPACITY: usize = 6;

struct Projection {
    source: (u64, Option<u64>),
    revision: u64,
    schema: Box<str>,
    renderables: bool,
    retain_fragment: bool,
    mesh_inputs: SceneMeshInputs,
    scene: Arc<SceneModel>,
}

struct SceneCombination {
    inputs: [Weak<SceneModel>; 2],
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
    combinations: RefCell<VecDeque<SceneCombination>>,
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
    key: ParticleModelKey,
    meshes: Option<Arc<[Mesh3D]>>,
}

enum ParticleModelKey {
    Asset(String),
    Scene(Arc<[SceneObject]>),
}

impl ParticleModelKey {
    fn matches(&self, source: &ParticleModelSource<'_>) -> bool {
        match (self, source) {
            (Self::Asset(key), ParticleModelSource::Asset(asset)) => key == asset,
            (Self::Scene(objects), ParticleModelSource::Scene(scene)) => {
                Arc::ptr_eq(objects, scene.objects())
                    || objects.as_ref() == scene.objects().as_ref()
            }
            _ => false,
        }
    }
}

impl SceneProjectionCache {
    pub const fn new() -> Self {
        Self {
            projections: RefCell::new(VecDeque::new()),
            combinations: RefCell::new(VecDeque::new()),
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

    pub fn combine_scenes(
        &self,
        shared: &Arc<SceneModel>,
        local: &Arc<SceneModel>,
    ) -> Arc<SceneModel> {
        let mut combinations = self.combinations.borrow_mut();
        let index = combinations
            .iter()
            .position(|entry| {
                entry.inputs[0].as_ptr() == Arc::as_ptr(shared)
                    && entry.inputs[1].as_ptr() == Arc::as_ptr(local)
            })
            .or_else(|| {
                // Inline record lists can produce an equivalent local snapshot
                // with a fresh identity. Compare a stable input by identity first.
                combinations.iter().position(|entry| {
                    let (previous, current) = if entry.inputs[0].as_ptr() == Arc::as_ptr(shared) {
                        (&entry.inputs[1], local)
                    } else if entry.inputs[1].as_ptr() == Arc::as_ptr(local) {
                        (&entry.inputs[0], shared)
                    } else {
                        return false;
                    };
                    previous
                        .upgrade()
                        .is_some_and(|scene| scene == *current && scene.same_node_sources(current))
                })
            });
        if let Some(entry) = index.and_then(|index| combinations.remove(index)) {
            let scene = entry.scene.clone();
            combinations.push_front(entry);
            crate::update_profile::scene_projection(true);
            return scene;
        }
        let scene = SceneModel::combine(shared, local);
        crate::update_profile::scene_projection(false);
        combinations.truncate(CAPACITY - 1);
        combinations.push_front(SceneCombination {
            inputs: [Arc::downgrade(shared), Arc::downgrade(local)],
            scene: scene.clone(),
        });
        scene
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

    pub fn particle_points(
        &self,
        key: &str,
        scale: [f32; 3],
        shuffle: Option<u32>,
        imported: impl FnOnce(&str) -> Option<ParticlePointCloud>,
    ) -> Option<Arc<ParticleMeshSurface>> {
        self.particle_surfaces.points(key, scale, shuffle, imported)
    }

    pub fn particle_model(
        &self,
        source: ParticleModelSource<'_>,
        imported: impl FnOnce(&str) -> Option<MeshAsset>,
    ) -> Option<Arc<[Mesh3D]>> {
        {
            let mut models = self.particle_models.borrow_mut();
            if let Some(index) = models.iter().position(|entry| entry.key.matches(&source))
                && let Some(entry) = models.remove(index)
            {
                let meshes = entry.meshes.clone();
                models.push_front(entry);
                return meshes;
            }
        }
        let (key, meshes) = match source {
            ParticleModelSource::Asset(key) => {
                let meshes = MeshAsset::resolve(key, imported).and_then(|asset| {
                    let asset = asset.select_material_variant("");
                    if let MeshAsset::Static(mesh) = asset {
                        return Some(Arc::from([mesh]));
                    }
                    bake_particle_meshes(&rest_mesh_objects(&asset), |_, _| None)
                });
                (ParticleModelKey::Asset(key.into()), meshes)
            }
            ParticleModelSource::Scene(scene) => {
                let objects = scene.objects();
                let previous: Vec<_> = self
                    .particle_models
                    .borrow()
                    .iter()
                    .filter_map(|entry| {
                        let ParticleModelKey::Scene(source) = &entry.key else {
                            return None;
                        };
                        Some((source.clone(), entry.meshes.as_ref()?.clone()))
                    })
                    .collect();
                let meshes = bake_particle_meshes(objects, |index, object| {
                    previous.iter().find_map(|(source, baked)| {
                        (source.get(index)? == object).then(|| baked[index].clone())
                    })
                });
                (ParticleModelKey::Scene(objects.clone()), meshes)
            }
        };
        let mut models = self.particle_models.borrow_mut();
        models.truncate(CAPACITY - 1);
        models.push_front(CachedParticleModel {
            key,
            meshes: meshes.clone(),
        });
        meshes
    }

    pub fn particle_instance_models<T>(
        &self,
        model: &VecModel<T>,
        mut project: impl FnMut(&T) -> Option<(String, Arc<SceneModel>)>,
    ) -> Arc<[ParticleInstanceModel]> {
        let mut links = Vec::new();
        model.visit_row_data(super::MAX_PARTICLE_EMITTERS * 2, |_, _, value| {
            if let Some((emitter, scene)) = project(value)
                && !emitter.is_empty()
                && let Some(meshes) =
                    self.particle_model(ParticleModelSource::Scene(&scene), |_| None)
            {
                links.push(ParticleInstanceModel { emitter, meshes });
            }
        });
        links.into()
    }

    pub fn record_particle_instance_models(
        &self,
        model: &RecordList,
    ) -> Arc<[ParticleInstanceModel]> {
        self.particle_instance_models(model.model(), |row| {
            let (Some(Value::String(emitter)), Some(Value::SceneObjects(scene))) =
                (row.field("emitter"), row.field("model"))
            else {
                return None;
            };
            Some((emitter.clone(), scene.clone()))
        })
    }

    pub fn model_nodes<T: Clone>(
        &self,
        model: VecModel<T>,
        schema: &str,
        project: impl Fn(T, &SceneMeshInputs) -> Option<SceneNode>,
    ) -> Arc<SceneModel> {
        self.project_nodes(model, schema, project, false)
    }

    pub fn model_fragment<T: Clone>(
        &self,
        model: VecModel<T>,
        schema: &str,
        project: impl Fn(T, &SceneMeshInputs) -> Option<SceneNode>,
    ) -> Arc<SceneModel> {
        self.project_nodes(model, schema, project, true)
    }

    fn project_nodes<T: Clone>(
        &self,
        model: VecModel<T>,
        schema: &str,
        project: impl Fn(T, &SceneMeshInputs) -> Option<SceneNode>,
        retain_fragment: bool,
    ) -> Arc<SceneModel> {
        self.get(
            &model,
            schema,
            (true, retain_fragment),
            |inputs, observe_rows| {
                prepare_model(
                    &model,
                    |row| project(row, inputs),
                    true,
                    (observe_rows, retain_fragment),
                )
            },
        )
    }

    pub fn model_cameras<T: Clone>(
        &self,
        model: VecModel<T>,
        schema: &str,
        project: impl Fn(T, &SceneMeshInputs) -> Option<SceneNode>,
    ) -> Arc<SceneModel> {
        self.get(&model, schema, (false, false), |inputs, observe_rows| {
            prepare_model(
                &model,
                |row| project(row, inputs),
                false,
                (observe_rows, false),
            )
        })
    }

    /// The interpreter passes its evaluated argument; other handles to a
    /// live record list keep its rows observable.
    pub fn record_objects(
        &self,
        model: &RecordList,
        imported: impl Fn(&str) -> Option<MeshAsset>,
        textures: impl Fn(&str) -> Option<TextureAsset3D>,
        shaders: impl Fn(&str) -> Option<Arc<str>>,
        text_fonts: super::SceneTextFonts<'_>,
        retain_fragment: bool,
    ) -> Arc<SceneModel> {
        self.get(
            model.model(),
            model.name(),
            (true, retain_fragment),
            |inputs, observe_rows| {
                prepare_record_scene(
                    model,
                    imported,
                    (textures, shaders),
                    (true, observe_rows, retain_fragment),
                    inputs,
                    &self.camera_matrices,
                    text_fonts,
                )
            },
        )
    }

    pub fn record_cameras(
        &self,
        model: &RecordList,
        imported: impl Fn(&str) -> Option<MeshAsset>,
    ) -> Arc<SceneModel> {
        self.get(
            model.model(),
            model.name(),
            (false, false),
            |inputs, observe_rows| {
                prepare_record_scene(
                    model,
                    imported,
                    (|_| None, |_| None),
                    (false, observe_rows, false),
                    inputs,
                    &self.camera_matrices,
                    Default::default(),
                )
            },
        )
    }

    fn get<T>(
        &self,
        model: &VecModel<T>,
        schema: &str,
        (renderables, retain_fragment): (bool, bool),
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
                    && projection.retain_fragment == retain_fragment
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
                    || projection.retain_fragment != retain_fragment
                    || (projection.revision == revision && projection.renderables && !renderables)
            });
            projections.truncate(CAPACITY - 1);
            projections.push_front(Projection {
                source,
                revision,
                schema: schema.into(),
                renderables,
                retain_fragment,
                mesh_inputs,
                scene: scene.clone(),
            });
        }
        scene
    }
}

fn bake_particle_meshes(
    objects: &[SceneObject],
    reuse: impl Fn(usize, &SceneObject) -> Option<Mesh3D>,
) -> Option<Arc<[Mesh3D]>> {
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
        .enumerate()
        .map(|(index, object)| reuse(index, object).or_else(|| object.particle_prototype()))
        .collect()
}
