use crate::{
    MediaError,
    ff::{XrackAudioDevice, XrackMediaApi, XrackVoiceOpen},
};
use std::{
    ffi::c_void,
    path::PathBuf,
    ptr::NonNull,
    sync::{Arc, OnceLock},
};

pub(crate) struct AudioDevice<T> {
    handle: NonNull<XrackAudioDevice>,
    close: unsafe extern "C" fn(*mut XrackAudioDevice),
    pub(crate) state: Arc<T>,
}

impl<T: Send + Sync> AudioDevice<T> {
    pub(crate) fn open(
        state: Arc<T>,
        render: unsafe extern "C" fn(*mut c_void, *mut f32, u32, u32),
    ) -> Result<Self, MediaError> {
        load().map_err(|error| MediaError(error.into()))?;
        let api = api();
        let (Some(open), Some(close)) = (api.xr_audio_open, api.xr_audio_close) else {
            return Err(MediaError(
                "the media library does not provide audio output".into(),
            ));
        };
        // The callback state stays alive until close has joined the device.
        let handle = unsafe { open(Some(render), Arc::as_ptr(&state).cast_mut().cast()) };
        let handle = NonNull::new(handle)
            .ok_or_else(|| MediaError("no audio output device is available".into()))?;
        Ok(Self {
            handle,
            close,
            state,
        })
    }
}

impl<T> Drop for AudioDevice<T> {
    fn drop(&mut self) {
        unsafe { (self.close)(self.handle.as_ptr()) }
    }
}

struct Native {
    library: libloading::Library,
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
        let required = [
            (stringify!(av_strerror), api.av_strerror.is_some()),
            (
                stringify!(av_channel_layout_default),
                api.av_channel_layout_default.is_some(),
            ),
            (stringify!(av_frame_alloc), api.av_frame_alloc.is_some()),
            (stringify!(av_frame_free), api.av_frame_free.is_some()),
            (stringify!(av_packet_alloc), api.av_packet_alloc.is_some()),
            (stringify!(av_packet_free), api.av_packet_free.is_some()),
            (stringify!(av_packet_unref), api.av_packet_unref.is_some()),
            (
                stringify!(avcodec_alloc_context3),
                api.avcodec_alloc_context3.is_some(),
            ),
            (
                stringify!(avcodec_free_context),
                api.avcodec_free_context.is_some(),
            ),
            (
                stringify!(avcodec_parameters_to_context),
                api.avcodec_parameters_to_context.is_some(),
            ),
            (stringify!(avcodec_open2), api.avcodec_open2.is_some()),
            (
                stringify!(avcodec_send_packet),
                api.avcodec_send_packet.is_some(),
            ),
            (
                stringify!(avcodec_receive_frame),
                api.avcodec_receive_frame.is_some(),
            ),
            (
                stringify!(avcodec_flush_buffers),
                api.avcodec_flush_buffers.is_some(),
            ),
            (
                stringify!(avformat_open_input),
                api.avformat_open_input.is_some(),
            ),
            (
                stringify!(avformat_find_stream_info),
                api.avformat_find_stream_info.is_some(),
            ),
            (
                stringify!(av_find_best_stream),
                api.av_find_best_stream.is_some(),
            ),
            (stringify!(av_read_frame), api.av_read_frame.is_some()),
            (stringify!(av_seek_frame), api.av_seek_frame.is_some()),
            (
                stringify!(avformat_close_input),
                api.avformat_close_input.is_some(),
            ),
            (stringify!(swr_init), api.swr_init.is_some()),
            (
                stringify!(swr_alloc_set_opts2),
                api.swr_alloc_set_opts2.is_some(),
            ),
            (stringify!(swr_free), api.swr_free.is_some()),
            (stringify!(swr_convert), api.swr_convert.is_some()),
            (
                stringify!(swr_get_out_samples),
                api.swr_get_out_samples.is_some(),
            ),
            (stringify!(sws_freeContext), api.sws_freeContext.is_some()),
            (stringify!(sws_scale), api.sws_scale.is_some()),
            (
                stringify!(sws_getCachedContext),
                api.sws_getCachedContext.is_some(),
            ),
            (stringify!(xr_audio_open), api.xr_audio_open.is_some()),
            (stringify!(xr_audio_close), api.xr_audio_close.is_some()),
            (
                stringify!(avformat_alloc_context),
                api.avformat_alloc_context.is_some(),
            ),
            (
                stringify!(avcodec_find_decoder),
                api.avcodec_find_decoder.is_some(),
            ),
            (stringify!(av_dict_set), api.av_dict_set.is_some()),
            (stringify!(av_dict_get), api.av_dict_get.is_some()),
            (stringify!(av_dict_free), api.av_dict_free.is_some()),
            (
                stringify!(avcodec_decode_subtitle2),
                api.avcodec_decode_subtitle2.is_some(),
            ),
            (stringify!(avsubtitle_free), api.avsubtitle_free.is_some()),
            (stringify!(xr_hls_window), api.xr_hls_window.is_some()),
        ];
        if let Some((name, _)) = required.into_iter().find(|&(_, present)| !present) {
            return Err(format!(
                "media library {} is missing {name}",
                path.display()
            ));
        }
        Ok(Native { library, api })
    }
}

pub fn load() -> Result<(), &'static str> {
    NATIVE
        .get_or_init(initialize)
        .as_ref()
        .map(|_| ())
        .map_err(String::as_str)
}

pub(crate) fn voice_open() -> Result<XrackVoiceOpen, MediaError> {
    let native = NATIVE
        .get_or_init(initialize)
        .as_ref()
        .map_err(|error| MediaError(error.clone()))?;
    // The file and the versioned symbol live for the duration of the process.
    unsafe {
        native
            .library
            .get::<XrackVoiceOpen>(b"xrack_voice_open_v1\0")
    }
    .map(|symbol| *symbol)
    .map_err(|_| {
        MediaError("the XRack media library does not provide desktop voice capture".into())
    })
}

#[expect(
    clippy::expect_used,
    reason = "decoder and audio device constructors load the media library before accessing the API"
)]
pub(crate) fn api() -> &'static XrackMediaApi {
    // Each decoder loads the library before it allocates a C handle.
    NATIVE
        .get()
        .and_then(|native| native.as_ref().ok())
        .expect("loaded media library")
        .api
}

#[expect(
    clippy::expect_used,
    reason = "initialize verifies all required entries in the versioned media API table"
)]
pub(crate) fn required_function<F>(function: Option<F>, name: &str) -> F {
    function.expect(name)
}

macro_rules! media_function {
    ($name:ident) => {
        $crate::native::required_function($crate::native::api().$name, stringify!($name))
    };
}
pub(crate) use media_function;

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
        .all(|(byte, hex)| {
            std::str::from_utf8(hex)
                .ok()
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                == Some(*byte)
        })
    {
        return Err(format!(
            "incorrect bundled media library {}",
            path.display()
        ));
    }
    Ok(path)
}
