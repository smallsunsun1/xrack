//! Compose resolved scene roots without reprojecting their authored rows.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, OnceLock, Weak},
};

use super::{
    LodGroup, MAX_NODES, MAX_SCENE_LIGHTS, NodeFrame, SceneCamera, SceneGeometry, SceneModel,
    SceneObject, SelectedCamera,
};
use crate::PhysicsWorld3D;

#[derive(Debug)]
struct ScenePart {
    model: Arc<SceneModel>,
    row_offset: u32,
    scope_offset: u16,
}

impl ScenePart {
    fn objects<'a>(
        &'a self,
        geometry: &'a SceneGeometry,
    ) -> impl Iterator<Item = SceneObject> + 'a {
        geometry.iter().map(|source| {
            let mut object = source.clone();
            if let Some(row) = object.scene_row() {
                object = object.with_source_row(row + self.row_offset);
            }
            if let Some(order) = &mut object.instance_order {
                order.group[0] += self.row_offset;
            }
            if object.lighting_index != 0 {
                object.lighting_index += self.scope_offset;
            }
            object
        })
    }
}

#[derive(Debug)]
struct ViewGeometry {
    inputs: Box<[Weak<SceneGeometry>]>,
    combined: Arc<SceneGeometry>,
}

/// Separate namespaces for each authored root, with one row and scope space
/// for the renderer and spatial queries.
#[derive(Debug)]
pub(super) struct SceneComposition {
    parts: Box<[ScenePart]>,
    has_lods: bool,
    pub(super) has_colliders: bool,
    views: Mutex<VecDeque<ViewGeometry>>,
    physics: Mutex<Option<Arc<SceneModel>>>,
}

impl PartialEq for SceneComposition {
    fn eq(&self, other: &Self) -> bool {
        self.parts.len() == other.parts.len()
            && self.parts.iter().zip(&other.parts).all(|(left, right)| {
                Arc::ptr_eq(&left.model, &right.model) || left.model == right.model
            })
    }
}

impl SceneComposition {
    pub(super) fn node_key(&self, row: u32) -> Option<(crate::RowKey, &str)> {
        let part = self
            .parts
            .iter()
            .rev()
            .find(|part| part.row_offset <= row)?;
        part.model.node_key(row - part.row_offset)
    }

    pub(super) fn match_node_key(
        &self,
        key: crate::RowKey,
        namespace: &str,
        offset: u32,
        result: &mut Option<u32>,
    ) -> bool {
        self.parts.iter().all(|part| {
            part.model
                .match_node_key(key, namespace, offset + part.row_offset, result)
        })
    }

    pub(super) fn same_node_sources(&self, other: &Self) -> bool {
        self.parts.len() == other.parts.len()
            && self.parts.iter().zip(&other.parts).all(|(left, right)| {
                left.row_offset == right.row_offset && left.model.same_node_sources(&right.model)
            })
    }

    pub(super) fn camera(&self, id: &str) -> Option<SelectedCamera<'_>> {
        self.parts.iter().find_map(|part| {
            let (name, entry) = part.model.cameras.get_key_value(id)?;
            Some(SelectedCamera {
                entry,
                name: Some(name),
                row: entry.row + part.row_offset,
            })
        })
    }

    pub(super) fn frame_at_row(&self, row: u32) -> Option<(&SceneModel, &NodeFrame, u16)> {
        let part = self
            .parts
            .iter()
            .rev()
            .find(|part| part.row_offset <= row)?;
        let (scene, frame, offset) = part.model.frame_at_row(row - part.row_offset)?;
        Some((scene, frame, offset + part.scope_offset))
    }

    pub(super) fn frame(&self, id: &str) -> Option<(&SceneModel, &NodeFrame, u16)> {
        self.parts.iter().find_map(|part| {
            Some((
                part.model.as_ref(),
                part.model.frames.get(id)?,
                part.scope_offset,
            ))
        })
    }

    pub(super) fn lod_group(&self, mut index: usize) -> Option<&LodGroup> {
        for part in &self.parts {
            if let Some(group) = part.model.lod_groups.get(index) {
                return Some(group);
            }
            index -= part.model.lod_groups.len();
        }
        None
    }

    pub(super) fn collisions(&self) -> Arc<SceneGeometry> {
        let objects = self
            .parts
            .iter()
            .flat_map(|part| part.objects(part.model.collision_geometry()))
            .take(MAX_NODES)
            .collect();
        SceneGeometry::new(objects, 0)
    }

    pub(super) fn render_geometry(
        &self,
        primary: &Arc<SceneGeometry>,
        camera: &SceneCamera,
        fade_width: f64,
    ) -> Arc<SceneGeometry> {
        if !self.has_lods {
            return primary.clone();
        }

        let mut geometries: Vec<_> = self
            .parts
            .iter()
            .map(|part| part.model.render_geometry(camera, fade_width))
            .collect();
        // A fade needs both variants. Use hard choices when concatenation
        // would cut through a coarse/fine pair at the instance limit.
        if geometries.iter().any(|geometry| geometry.lod_fade)
            && geometries
                .iter()
                .map(|geometry| geometry.len())
                .sum::<usize>()
                > MAX_NODES
        {
            for (part, geometry) in self.parts.iter().zip(&mut geometries) {
                *geometry = part.model.render_geometry(camera, 0.0);
            }
        }
        if self
            .parts
            .iter()
            .zip(&geometries)
            .all(|(part, geometry)| Arc::ptr_eq(&part.model.geometry, geometry))
        {
            return primary.clone();
        }

        let mut views = self
            .views
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let cached = views.iter().position(|view| {
            view.inputs
                .iter()
                .zip(&geometries)
                .all(|(before, after)| before.as_ptr() == Arc::as_ptr(after))
        });
        let entry = cached
            .and_then(|index| views.remove(index))
            .unwrap_or_else(|| {
                let objects = self
                    .parts
                    .iter()
                    .zip(&geometries)
                    .flat_map(|(part, geometry)| part.objects(geometry))
                    .take(MAX_NODES)
                    .collect();
                let count = geometries.iter().map(|geometry| geometry.lod_objects).sum();
                ViewGeometry {
                    inputs: geometries.iter().map(Arc::downgrade).collect(),
                    combined: SceneGeometry::new(objects, count),
                }
            });
        let combined = entry.combined.clone();
        views.truncate(3);
        views.push_front(entry);
        combined
    }

    pub(super) fn with_physics(&self, world: &PhysicsWorld3D) -> Option<Arc<SceneModel>> {
        let mut changed = false;
        let parts: Vec<_> = self
            .parts
            .iter()
            .map(|part| {
                let next = part.model.with_physics(world);
                changed |= next.is_some();
                next.unwrap_or_else(|| part.model.clone())
            })
            .collect();
        if !changed {
            return None;
        }
        let mut cache = self
            .physics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(previous) = cache.as_ref()
            && let Some(composition) = previous.composition.as_ref()
            && composition.parts.len() == parts.len()
            && composition
                .parts
                .iter()
                .zip(&parts)
                .all(|(before, after)| Arc::ptr_eq(&before.model, after))
        {
            return Some(previous.clone());
        }
        let result = SceneModel::from_parts(parts);
        *cache = Some(result.clone());
        Some(result)
    }
}

impl SceneModel {
    pub(super) fn from_parts(models: impl IntoIterator<Item = Arc<Self>>) -> Arc<Self> {
        let mut parts = Vec::new();
        let mut source_rows = 0u32;
        let mut scope_span = 0u16;
        for model in models {
            if model.source_rows == 0 {
                continue;
            }
            let Some(next_rows) = source_rows.checked_add(model.source_rows) else {
                break;
            };
            let Some(next_scope) = scope_span.checked_add(model.scope_span) else {
                break;
            };
            parts.push(ScenePart {
                model,
                row_offset: source_rows,
                scope_offset: scope_span,
            });
            source_rows = next_rows;
            scope_span = next_scope;
        }
        if let [part] = parts.as_slice() {
            return part.model.clone();
        }
        let objects = parts
            .iter()
            .flat_map(|part| part.objects(&part.model.geometry))
            .take(MAX_NODES)
            .collect();
        let imported_lights: Vec<_> = parts
            .iter()
            .flat_map(|part| {
                part.model
                    .imported_lights
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .map(|light| {
                        let mut light = *light;
                        light.scope = light.scope.shifted(part.scope_offset);
                        light
                    })
            })
            .take(MAX_SCENE_LIGHTS)
            .collect();
        let has_colliders = parts.iter().any(|part| part.model.has_collision_shapes());
        let has_lods = parts.iter().any(|part| !part.model.lod_groups.is_empty());
        let mut first_camera = parts.iter().find_map(|part| {
            let mut camera = part.model.first_camera.clone()?;
            camera.entry.row += part.row_offset;
            Some(camera)
        });
        if let Some(camera) = &mut first_camera
            && let Some(name) = &camera.name
        {
            let row = parts
                .iter()
                .find_map(|part| Some(part.model.cameras.get(name)?.row + part.row_offset));
            if row != Some(camera.entry.row) {
                camera.name = None;
            }
        }
        Arc::new(Self {
            geometry: SceneGeometry::new(objects, 0),
            scene_particle_surfaces: Default::default(),
            collision_rows: Vec::new(),
            collision_geometry: OnceLock::new(),
            cameras: Default::default(),
            first_camera,
            imported_lights: (!imported_lights.is_empty()).then(|| Arc::from(imported_lights)),
            frames: Default::default(),
            lod_groups: Vec::new(),
            lod_selection: super::SceneLodSelection::Primary,
            physics: None,
            source_rows,
            row_keys: Default::default(),
            node_namespace: Default::default(),
            fragment: None,
            scope_span,
            composition: Some(Box::new(SceneComposition {
                parts: parts.into_boxed_slice(),
                has_lods,
                has_colliders,
                views: Mutex::default(),
                physics: Mutex::default(),
            })),
        })
    }

    pub(crate) fn combine(shared: &Arc<Self>, local: &Arc<Self>) -> Arc<Self> {
        if shared.source_rows == 0 {
            return local.clone();
        }
        if local.source_rows == 0 {
            return shared.clone();
        }
        let mut roots = Vec::new();
        for model in [shared, local] {
            if let Some(composition) = &model.composition {
                roots.extend(composition.parts.iter().map(|part| part.model.clone()));
            } else {
                roots.push(model.clone());
            }
        }
        Self::from_parts(roots)
    }
}
