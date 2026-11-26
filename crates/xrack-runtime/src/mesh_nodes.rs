use crate::{Point3D, VecModel, Vector3D};
use nalgebra::Matrix4;
use std::{collections::BTreeMap, sync::Arc};

/// A glTF node and its affine transform in the query's coordinate frame.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshNode3D {
    pub source_index: i64,
    pub parent_index: i64,
    pub name: String,
    pub path: String,
    pub origin: Point3D,
    pub axis_x: Vector3D,
    pub axis_y: Vector3D,
    pub axis_z: Vector3D,
    pub has_mesh: bool,
    pub has_camera: bool,
    pub has_light: bool,
}

impl MeshNode3D {
    pub fn valid(&self) -> bool {
        self.source_index >= 0
    }

    pub(crate) fn rest_matrix(&self) -> Matrix4<f32> {
        let axis = |axis: Vector3D| [axis.x as f32, axis.y as f32, axis.z as f32, 0.0];
        let columns = [
            axis(self.axis_x),
            axis(self.axis_y),
            axis(self.axis_z),
            [
                self.origin.x as f32,
                self.origin.y as f32,
                self.origin.z as f32,
                1.0,
            ],
        ];
        Matrix4::from_fn(|row, column| columns[column][row])
    }
}

impl Default for MeshNode3D {
    fn default() -> Self {
        Self {
            source_index: -1,
            parent_index: -1,
            name: String::new(),
            path: String::new(),
            origin: Point3D::default(),
            axis_x: Vector3D::default(),
            axis_y: Vector3D::default(),
            axis_z: Vector3D::default(),
            has_mesh: false,
            has_camera: false,
            has_light: false,
        }
    }
}

/// Shared node data and indices. Geometry, LOD and material selection use their
/// own GPU-facing representations.
#[doc(hidden)]
#[derive(Debug, PartialEq)]
pub struct MeshNodeCatalog {
    nodes: Vec<MeshNode3D>,
    names: BTreeMap<String, Option<usize>>,
    paths: BTreeMap<String, Option<usize>>,
    sources: BTreeMap<i64, usize>,
}

impl MeshNodeCatalog {
    pub fn new(nodes: Vec<MeshNode3D>) -> Arc<Self> {
        let mut names = BTreeMap::new();
        let mut paths = BTreeMap::new();
        let mut sources = BTreeMap::new();
        for (position, node) in nodes.iter().enumerate() {
            sources.insert(node.source_index, position);
            for (value, index) in [(&node.name, &mut names), (&node.path, &mut paths)] {
                if !value.is_empty() {
                    index
                        .entry(value.clone())
                        .and_modify(|found| *found = None)
                        .or_insert(Some(position));
                }
            }
        }
        Arc::new(Self {
            nodes,
            names,
            paths,
            sources,
        })
    }

    pub fn entries(&self) -> &[MeshNode3D] {
        &self.nodes
    }

    pub fn node(&self, selector: &str) -> MeshNode3D {
        self.selector_ordinal(selector)
            .and_then(|index| self.nodes.get(index))
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn selector_ordinal(&self, selector: &str) -> Option<usize> {
        let position = if let Some(index) = selector
            .strip_prefix('#')
            .and_then(|i| i.parse::<i64>().ok())
        {
            self.sources.get(&index)
        } else if selector.starts_with('/') {
            self.paths.get(selector).and_then(Option::as_ref)
        } else {
            self.names.get(selector).and_then(Option::as_ref)
        };
        position.copied()
    }

    pub(crate) fn ordinal(&self, source: i64) -> Option<usize> {
        self.sources.get(&source).copied()
    }

    pub fn node_at(&self, source: i64) -> MeshNode3D {
        self.sources
            .get(&source)
            .and_then(|&index| self.nodes.get(index))
            .cloned()
            .unwrap_or_default()
    }

    pub fn nodes(&self, kind: &str) -> impl Iterator<Item = &MeshNode3D> {
        self.nodes.iter().filter(move |node| match kind {
            "" | "all" => true,
            "mesh" => node.has_mesh,
            "camera" => node.has_camera,
            "light" => node.has_light,
            _ => false,
        })
    }

    pub fn indices(&self, kind: &str) -> VecModel<i64> {
        self.nodes(kind)
            .map(|node| node.source_index)
            .collect::<Vec<_>>()
            .into()
    }

    /// Emit compact data for the AOT asset table.
    pub fn packed(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for node in &self.nodes {
            bytes.extend(node.source_index.to_le_bytes());
            bytes.extend(node.parent_index.to_le_bytes());
            bytes.push(
                u8::from(node.has_mesh)
                    | (u8::from(node.has_camera) << 1)
                    | (u8::from(node.has_light) << 2),
            );
            let origin = [node.origin.x, node.origin.y, node.origin.z];
            for coord in origin.into_iter().chain(
                [node.axis_x, node.axis_y, node.axis_z]
                    .into_iter()
                    .flat_map(|axis| [axis.x, axis.y, axis.z]),
            ) {
                bytes.extend(coord.to_le_bytes());
            }
            for value in [&node.name, &node.path] {
                bytes.extend((value.len() as u64).to_le_bytes());
                bytes.extend(value.as_bytes());
            }
        }
        bytes
    }

    /// Rebuild the same index from the compiler's embedded data.
    pub fn from_packed(mut bytes: &[u8]) -> Option<Arc<Self>> {
        fn take<'a>(bytes: &mut &'a [u8], length: usize) -> Option<&'a [u8]> {
            let (value, tail) = bytes.split_at_checked(length)?;
            *bytes = tail;
            Some(value)
        }
        fn read<const N: usize>(bytes: &mut &[u8]) -> Option<[u8; N]> {
            take(bytes, N)?.try_into().ok()
        }
        fn string(bytes: &mut &[u8]) -> Option<String> {
            let length = u64::from_le_bytes(read(bytes)?).try_into().ok()?;
            std::str::from_utf8(take(bytes, length)?)
                .ok()
                .map(str::to_owned)
        }
        fn vector(bytes: &mut &[u8]) -> Option<[f64; 3]> {
            let mut fields = [0.0; 3];
            for field in &mut fields {
                *field = f64::from_le_bytes(read(bytes)?);
                if !field.is_finite() {
                    return None;
                }
            }
            Some(fields)
        }

        let mut nodes = Vec::new();
        while !bytes.is_empty() {
            let source_index = i64::from_le_bytes(read(&mut bytes)?);
            let parent_index = i64::from_le_bytes(read(&mut bytes)?);
            let [flags] = read(&mut bytes)?;
            if source_index < 0 || parent_index < -1 || flags > 7 {
                return None;
            }
            let [x, y, z] = vector(&mut bytes)?;
            let origin = Point3D::new(x, y, z);
            let axis = |bytes: &mut &[u8]| {
                let [x, y, z] = vector(bytes)?;
                Some(Vector3D::new(x, y, z))
            };
            let axis_x = axis(&mut bytes)?;
            let axis_y = axis(&mut bytes)?;
            let axis_z = axis(&mut bytes)?;
            let name = string(&mut bytes)?;
            let path = string(&mut bytes)?;
            nodes.push(MeshNode3D {
                source_index,
                parent_index,
                name,
                path,
                origin,
                axis_x,
                axis_y,
                axis_z,
                has_mesh: flags & 1 != 0,
                has_camera: flags & 2 != 0,
                has_light: flags & 4 != 0,
            });
        }
        Some(Self::new(nodes))
    }
}
