//! Shared file-backed 3D resource preparation for the compiler and applications.

mod heightfield;
mod lightmap_unwrap;
mod mesh;
mod mesh_animation;
mod mesh_attributes;
mod mesh_compression;
mod mesh_draco;
mod mesh_instances;
mod mesh_lights;
mod mesh_lod;
mod mesh_lod_motion;
mod mesh_materials;
mod mesh_morph;
mod mesh_obj;
mod mesh_skin;
mod mesh_tangents;

#[doc(hidden)]
pub use mesh::MeshSource;
#[doc(hidden)]
pub use mesh_lod::generate as generate_mesh_lod;
#[doc(hidden)]
pub use mesh_materials::{pack_mesh_image, prepare_scene_texture};
pub use xrack_runtime::scene3d::MeshImportOptions;

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use xrack_runtime::MeshAsset3D;

/// A prepared mesh and the complete set of canonical files read for it.
pub struct MeshFile {
    pub asset: MeshAsset3D,
    pub inputs: BTreeSet<PathBuf>,
}

/// Read a local mesh from a host-selected path. Paths referenced by the file
/// follow the same relative resolution rules as statically embedded meshes.
pub fn load_mesh_file(
    path: impl AsRef<Path>,
    options: MeshImportOptions,
) -> Result<MeshFile, String> {
    let file = path.as_ref();
    load(file, options).map_err(|error| format!("{}: {error}", file.display()))
}

fn load(path: &Path, options: MeshImportOptions) -> Result<MeshFile, String> {
    let source = MeshSource::from_path(path)?;
    let path = path.canonicalize().map_err(|error| error.to_string())?;
    let (mut asset, inputs) = source.load(&path, None)?;
    if options.lightmap_uv {
        asset = asset.map_material_variants(|mesh| source.generate_lightmap_uv(mesh))?;
    }
    if options.automatic_lod {
        asset = asset
            .map_material_variants(|mesh| Ok(generate_mesh_lod(mesh)))
            .unwrap_or_else(|never: std::convert::Infallible| match never {});
    }
    Ok(MeshFile {
        asset: MeshAsset3D::new(asset),
        inputs,
    })
}

/// Enable declarative mesh imports on a native or headless application host.
#[doc(hidden)]
pub fn install_mesh_importer() {
    xrack_runtime::register_mesh_importer(|path, options| {
        load_mesh_file(path, options).map(|file| file.asset)
    });
}
