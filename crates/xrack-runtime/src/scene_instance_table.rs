//! Immutable instance-table resources shared by the 3D backends.

use std::sync::Arc;

use crate::{
    Color, Instance3D, Point3D, Quaternion3D, RecordValue, Value, VecModel, Vector3D,
    scene_hierarchy::SceneTransform,
    scene_lighting::linear_color,
    scene_random_instances::{
        InstancePlacement, InstanceTint, RandomInstances, SceneRandomInstances,
    },
};

/// The typed projection of an authored instance-table record.
#[doc(hidden)]
pub struct InstanceTableRow {
    pub position: Point3D,
    pub scale: Vector3D,
    pub rotation: Vector3D,
    pub orientation: Option<Quaternion3D>,
    pub color: Color,
    pub data: [f64; 4],
}

impl Default for InstanceTableRow {
    fn default() -> Self {
        Self {
            position: Point3D::default(),
            scale: Vector3D::new(1.0, 1.0, 1.0),
            rotation: Vector3D::default(),
            orientation: None,
            color: Color(0xffffffff),
            data: [0.0; 4],
        }
    }
}

impl InstanceTableRow {
    pub(crate) fn from_record(row: &RecordValue) -> Self {
        let mut result = Self::default();
        if let Some(Value::Point3D(position)) = row.field("position") {
            result.position = *position;
        }
        if let Some(Value::Vector3D(scale)) = row.field("scale") {
            result.scale = *scale;
        }
        if let Some(Value::Vector3D(rotation)) = row.field("rotation") {
            result.rotation = *rotation;
        }
        if let Some(Value::Quaternion3D(orientation)) = row.field("orientation") {
            result.orientation = Some(*orientation);
        }
        if let Some(Value::Color(color)) = row.field("color") {
            result.color = *color;
        }
        result.data = ["data-x", "data-y", "data-z", "data-w"].map(|name| match row.field(name) {
            Some(Value::Float(value)) => *value,
            _ => 0.0,
        });
        result
    }

    fn placement(self) -> Option<InstancePlacement> {
        let scale = self.scale;
        let rotation = self.rotation;
        let position = self.position.narrow()?;
        let scale = [scale.x as f32, scale.y as f32, scale.z as f32];
        let (transform, _) = if let Some(orientation) = self.orientation {
            SceneTransform::local_with_rotation(
                position,
                scale,
                [0.0; 3],
                orientation.rotation_f32(),
            )?
        } else {
            SceneTransform::local(
                position,
                scale,
                [rotation.y as f32, rotation.x as f32, rotation.z as f32],
                [0.0; 3],
            )?
        };
        let data = self.data.map(|value| value as f32);
        let [r, g, b] = linear_color(self.color);
        data.into_iter()
            .all(f32::is_finite)
            .then_some(InstancePlacement {
                transform,
                tint: InstanceTint::Linear([r, g, b, f32::from(self.color.0 as u8) / 255.0]),
                data: Some(data),
            })
    }
}

/// An immutable table of decoded or procedurally generated placements.
#[derive(Clone, Debug, Default)]
pub struct InstanceTable3D(Option<InstanceSource>);

#[derive(Clone, Debug)]
enum InstanceSource {
    Rows(Arc<[InstancePlacement]>),
    Random(Arc<RandomInstances>),
}

impl PartialEq for InstanceTable3D {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(InstanceSource::Rows(a)), Some(InstanceSource::Rows(b))) => {
                Arc::ptr_eq(a, b) || a == b
            }
            (Some(InstanceSource::Random(a)), Some(InstanceSource::Random(b))) => {
                Arc::ptr_eq(a, b) || a == b
            }
            (None, None) => true,
            _ => false,
        }
    }
}

impl InstanceTable3D {
    /// Prepare a seeded table. Placements are generated only when requested.
    /// A disabled generator returns an unloaded table.
    #[doc(hidden)]
    pub fn from_random(settings: SceneRandomInstances) -> Self {
        if settings.enabled == Some(false) {
            return Self::default();
        }
        Self(Some(InstanceSource::Random(Arc::new(
            settings.prepare(crate::scene3d::MAX_MESH_INSTANCES),
        ))))
    }

    /// Read a source row in table coordinates using its zero-based index.
    /// An unloaded table or an index outside the table returns an invalid value.
    pub fn instance(&self, index: i64) -> Instance3D {
        let Some(row) = usize::try_from(index).ok().and_then(|i| self.sample(i)) else {
            return Instance3D::default();
        };
        Instance3D::from_placement(
            String::new(),
            [-1, -1, index],
            row.tint.apply(Color(0xffffffff)),
            row.transform,
            row.data.unwrap_or_default(),
        )
    }

    pub(crate) fn from_model<T>(
        model: &VecModel<T>,
        mut project: impl FnMut(&T) -> InstanceTableRow,
    ) -> Self {
        let count = model.len();
        if count > crate::scene3d::MAX_MESH_INSTANCES {
            return Self::default();
        }
        let mut placements = Vec::with_capacity(count);
        model.visit_row_data(count, |index, _, row| {
            if placements.len() == index {
                placements.extend(project(row).placement());
            }
        });
        if placements.len() == count {
            Self(Some(InstanceSource::Rows(placements.into())))
        } else {
            Self::default()
        }
    }

    /// Decode a resource from 20 little-endian floats per row, without a file header.
    #[doc(hidden)]
    pub fn from_packed(bytes: &[u8]) -> Option<Self> {
        let (rows, remainder) = bytes.as_chunks::<80>();
        if !remainder.is_empty() || rows.len() > crate::scene3d::MAX_MESH_INSTANCES {
            return None;
        }
        Self::from_rows(rows.iter().map(|row| {
            let (words, _) = row.as_chunks::<4>();
            std::array::from_fn(|part| {
                std::array::from_fn(|lane| f32::from_le_bytes(words[part * 4 + lane]))
            })
        }))
    }

    /// Decode three affine rows, a linear RGBA tint and four custom channels.
    #[doc(hidden)]
    pub fn from_rows(rows: impl ExactSizeIterator<Item = [[f32; 4]; 5]>) -> Option<Self> {
        if rows.len() > crate::scene3d::MAX_MESH_INSTANCES {
            return None;
        }
        let placements = rows
            .map(|[x, y, z, tint, data]| {
                let transform = SceneTransform::from_instance_rows([x, y, z])?;
                if !tint.iter().all(|value| (0.0..=1.0).contains(value))
                    || !data.iter().all(|value| value.is_finite())
                {
                    return None;
                }
                Some(InstancePlacement {
                    transform,
                    tint: InstanceTint::Linear(tint),
                    data: Some(data),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self(Some(InstanceSource::Rows(placements.into()))))
    }

    #[doc(hidden)]
    pub fn packed_words(&self) -> impl Iterator<Item = f32> + '_ {
        (0..self.len())
            .filter_map(|index| self.sample(index))
            .flat_map(|entry| {
                entry
                    .transform
                    .rows()
                    .into_iter()
                    .chain([entry.tint.linear(), entry.data.unwrap_or_default()])
                    .flatten()
            })
    }

    pub fn len(&self) -> usize {
        match &self.0 {
            Some(InstanceSource::Rows(rows)) => rows.len(),
            Some(InstanceSource::Random(random)) => random.len(),
            None => 0,
        }
    }

    #[doc(hidden)]
    pub fn count(&self) -> i64 {
        self.len() as i64
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_loaded(&self) -> bool {
        self.0.is_some()
    }

    fn sample(&self, index: usize) -> Option<InstancePlacement> {
        match self.0.as_ref()? {
            InstanceSource::Rows(rows) => rows.get(index).copied(),
            InstanceSource::Random(random) if index < random.len() => random.sample(index),
            _ => None,
        }
    }

    fn transform(&self, index: usize) -> Option<SceneTransform> {
        match self.0.as_ref()? {
            InstanceSource::Rows(rows) => Some(rows.get(index)?.transform),
            InstanceSource::Random(random) if index < random.len() => random.transform(index),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct InstancePlacements {
    table: InstanceTable3D,
    limit: usize,
}

impl InstancePlacements {
    pub fn new(
        table: InstanceTable3D,
        random: Option<SceneRandomInstances>,
        override_count: i64,
        scene_limit: usize,
    ) -> Option<Self> {
        let table = if table.is_loaded() {
            table
        } else {
            InstanceTable3D::from_random(random?)
        };
        let limit = table
            .len()
            .min(scene_limit)
            .min(usize::try_from(override_count).unwrap_or(usize::MAX));
        table.is_loaded().then_some(Self { table, limit })
    }

    pub fn len(&self) -> usize {
        self.limit
    }

    pub fn sample(&self, index: usize) -> Option<InstancePlacement> {
        self.table.sample(index)
    }

    pub fn transform(&self, index: usize) -> Option<SceneTransform> {
        self.table.transform(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_projection::SceneProjectionCache;

    #[test]
    fn model_projection_reuses_one_revision_and_keeps_older_tables_immutable() {
        let model = VecModel::from(vec![1.0, 2.0]);
        let shared = model.clone();
        let cache = SceneProjectionCache::new();
        let mut projected = 0;
        let mut row = |x: &f64| {
            projected += 1;
            InstanceTableRow {
                position: Point3D::new(*x, 0.0, 0.0),
                ..InstanceTableRow::default()
            }
        };
        let first = cache.instance_table(&model, &mut row);
        let same = cache.instance_table(&shared, &mut row);
        assert_eq!(first, same);
        assert!(model.set_row(1, 3.0));
        let after = cache.instance_table(&shared, &mut row);
        assert_eq!(projected, 4);
        assert_eq!(first.packed_words().nth(23), Some(2.0));
        assert_eq!(after.packed_words().nth(23), Some(3.0));
        assert_eq!(after.packed_words().next(), Some(1.0));
    }
}
