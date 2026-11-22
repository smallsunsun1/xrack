use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

pub const MANIFEST_FILE: &str = "xrack-module.json";
pub const MANIFEST_VERSION: u32 = 1;

/// Paths are relative to the directory containing the manifest.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleManifest {
    pub version: u32,
    pub entry: PathBuf,
    pub mlir: PathBuf,
    pub rust: PathBuf,
    pub components: Vec<String>,
    pub inputs: Vec<PathBuf>,
}

impl ModuleManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != MANIFEST_VERSION {
            return Err(format!("unsupported XRack module version {}", self.version));
        }
        for path in [&self.entry, &self.mlir, &self.rust]
            .into_iter()
            .chain(&self.inputs)
        {
            if !is_relative_file(path) {
                return Err(format!("invalid module path {}", path.display()));
            }
        }
        Ok(())
    }

    /// Resolve an artifact within an opened module, including symlink checks.
    pub fn resolve(root: &Path, path: &Path) -> Result<PathBuf, String> {
        if !is_relative_file(path) {
            return Err(format!("invalid module path {}", path.display()));
        }
        let root = root
            .canonicalize()
            .map_err(|error| format!("{}: {error}", root.display()))?;
        let full = root.join(path);
        let resolved = full
            .canonicalize()
            .map_err(|error| format!("{}: {error}", full.display()))?;
        if !resolved.starts_with(&root) || !resolved.is_file() {
            return Err(format!(
                "{} is not a file inside the module",
                full.display()
            ));
        }
        Ok(resolved)
    }
}

fn is_relative_file(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}
