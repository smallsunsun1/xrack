//! FFmpeg-backed local media playback.
#[cfg(feature = "ffmpeg")]
mod decoder;
#[cfg(feature = "ffmpeg")]
mod native;
#[cfg(feature = "ffmpeg")]
mod player;
#[cfg(feature = "ffmpeg")]
mod subtitles;

#[cfg(feature = "ffmpeg")]
pub use decoder::{MediaError, VideoDecoder, VideoFrame};
#[cfg(feature = "ffmpeg")]
pub use native::load as check_native_backend;
#[cfg(feature = "ffmpeg")]
pub use player::install;

#[cfg(feature = "bazel_bindings")]
use xrack_ffmpeg_sys as ff;

#[cfg(all(
    feature = "ffmpeg",
    not(feature = "bazel_bindings"),
    target_os = "macos",
    target_arch = "aarch64"
))]
use xrack_media_bin_aarch64_apple_darwin as ff;
#[cfg(all(
    feature = "ffmpeg",
    not(feature = "bazel_bindings"),
    target_os = "windows",
    target_arch = "aarch64",
    target_env = "msvc"
))]
use xrack_media_bin_aarch64_pc_windows_msvc as ff;
#[cfg(all(
    feature = "ffmpeg",
    not(feature = "bazel_bindings"),
    target_os = "linux",
    target_arch = "aarch64",
    target_env = "gnu"
))]
use xrack_media_bin_aarch64_unknown_linux_gnu as ff;
#[cfg(all(
    feature = "ffmpeg",
    not(feature = "bazel_bindings"),
    target_os = "windows",
    target_arch = "x86_64",
    target_env = "msvc"
))]
use xrack_media_bin_x86_64_pc_windows_msvc as ff;
#[cfg(all(
    feature = "ffmpeg",
    not(feature = "bazel_bindings"),
    target_os = "linux",
    target_arch = "x86_64",
    target_env = "gnu"
))]
use xrack_media_bin_x86_64_unknown_linux_gnu as ff;
