use crate::ff::XrackMediaApi;
use std::{path::PathBuf, sync::OnceLock};

struct Native {
    _library: libloading::Library,
    api: &'static XrackMediaApi,
}

static NATIVE: OnceLock<Result<Native, String>> = OnceLock::new();

fn initialize() -> Result<Native, String> {
    let path = library_path()?;
    #[cfg(all(not(feature = "bazel_bindings"), not(target_os = "windows")))]
    let temporary;
    #[cfg(all(not(feature = "bazel_bindings"), not(target_os = "windows")))]
    let path = if let Some(path) = path {
        path
    } else {
        temporary = tempfile::tempdir().map_err(|error| error.to_string())?;
        let path = temporary.path().join(library_name());
        write_library(std::fs::File::create(&path).map_err(|error| error.to_string())?)?;
        path
    };
    #[cfg(all(not(feature = "bazel_bindings"), target_os = "windows"))]
    let path = path.map_or_else(packaged_windows_library, Ok)?;
    #[cfg(feature = "bazel_bindings")]
    let path = path.ok_or(
        "could not find the XRack media library alongside the executable or in Bazel runfiles",
    )?;
    // The package or the application supplies this library. It exports the
    // versioned table and uses the same C declarations as the generated Rust.
    unsafe {
        let library = libloading::Library::new(&path)
            .map_err(|error| format!("load media library {}: {error}", path.display()))?;
        let entry = library
            .get::<unsafe extern "C" fn() -> *const XrackMediaApi>(b"xrack_media_api_v3\0")
            .map_err(|error| format!("incompatible media library {}: {error}", path.display()))?;
        let api = entry()
            .as_ref()
            .ok_or("media library returned an empty API table")?;
        Ok(Native {
            _library: library,
            api,
        })
    }
}

pub fn load() -> Result<(), &'static str> {
    NATIVE
        .get_or_init(initialize)
        .as_ref()
        .map(|_| ())
        .map_err(String::as_str)
}

pub(crate) fn api() -> &'static XrackMediaApi {
    // Each decoder loads the library before it allocates a C handle.
    NATIVE
        .get()
        .and_then(|native| native.as_ref().ok())
        .expect("loaded media library")
        .api
}

fn library_path() -> Result<Option<PathBuf>, String> {
    if let Some(path) = std::env::var_os("XRACK_MEDIA_LIBRARY") {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err("XRACK_MEDIA_LIBRARY requires an absolute path".into());
        }
        return Ok(Some(path));
    }
    #[cfg(feature = "bazel_bindings")]
    {
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let sibling = executable.with_file_name(library_name());
        if sibling.is_file() {
            return Ok(Some(sibling));
        }
        if let Some(path) = runfiles::Runfiles::create()
            .ok()
            .and_then(|runfiles| runfiles::rlocation!(runfiles, env!("XRACK_MEDIA_RUNFILE")))
        {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

fn library_name() -> String {
    format!(
        "{}xrack_media_ffmpeg{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    )
}

#[cfg(not(feature = "bazel_bindings"))]
fn write_library(mut output: impl std::io::Write) -> Result<(), String> {
    std::io::copy(
        &mut lzma_rust2::XzReader::new(crate::ff::LIBRARY, false),
        &mut output,
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
}

#[cfg(all(not(feature = "bazel_bindings"), target_os = "windows"))]
fn packaged_windows_library() -> Result<PathBuf, String> {
    use sha2::{Digest, Sha256};
    use std::fs;
    // Windows keeps loaded DLLs locked. Use a per-user, content-addressed cache.
    let directory = PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is unset")?)
        .join("XRack")
        .join("media")
        .join(crate::ff::LIBRARY_SHA256);
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = directory.join(library_name());
    if !path.is_file() {
        let mut temporary =
            tempfile::NamedTempFile::new_in(&directory).map_err(|error| error.to_string())?;
        write_library(&mut temporary)?;
        if let Err(error) = temporary.persist_noclobber(&path)
            && error.error.kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(error.to_string());
        }
    }
    let digest = Sha256::digest(fs::read(&path).map_err(|error| error.to_string())?);
    if !digest
        .iter()
        .zip(crate::ff::LIBRARY_SHA256.as_bytes().chunks_exact(2))
        .all(|(byte, hex)| u8::from_str_radix(std::str::from_utf8(hex).unwrap(), 16) == Ok(*byte))
    {
        return Err(format!(
            "incorrect bundled media library {}",
            path.display()
        ));
    }
    Ok(path)
}
