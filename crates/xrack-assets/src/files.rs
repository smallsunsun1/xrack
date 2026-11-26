//! Bounded local file reads shared by resource loaders.
use crate::MAX_ASSET_BYTES;
use std::{
    collections::BTreeSet,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

pub fn local_path(url: &str) -> Result<&Path, String> {
    let relative = Path::new(url);
    if url.is_empty() || relative.is_absolute() || url.contains(':') {
        return Err("expected a local path relative to the .xr file".into());
    }
    Ok(relative)
}

pub fn asset_path(
    base: &Path,
    relative: &Path,
    declared: Option<&BTreeSet<PathBuf>>,
    kind: &str,
) -> Result<PathBuf, String> {
    let path = base.join(relative).canonicalize().map_err(|error| {
        if declared.is_some() {
            format!("{error}; list the {kind} in xr_ui_library(assets = [...])")
        } else {
            error.to_string()
        }
    })?;
    if declared.is_some_and(|inputs| !inputs.contains(&path)) {
        return Err("not declared in xr_ui_library(assets = [...])".into());
    }
    Ok(path)
}

pub fn read_asset(path: &Path, kind: &str) -> Result<Vec<u8>, String> {
    if !std::fs::metadata(path)
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err(format!("{kind} must be a regular file"));
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| error.to_string())?
        .take(MAX_ASSET_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_ASSET_BYTES {
        return Err(format!("{kind} file exceeds 16 MiB"));
    }
    Ok(bytes)
}
