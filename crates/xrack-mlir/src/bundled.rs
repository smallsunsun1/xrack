#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use xrack_mlir_bin_aarch64_apple_darwin::*;
#[cfg(all(target_os = "windows", target_arch = "aarch64", target_env = "msvc"))]
pub use xrack_mlir_bin_aarch64_pc_windows_msvc::*;
#[cfg(all(target_os = "linux", target_arch = "aarch64", target_env = "gnu"))]
pub use xrack_mlir_bin_aarch64_unknown_linux_gnu::*;
#[cfg(all(target_os = "windows", target_arch = "x86_64", target_env = "msvc"))]
pub use xrack_mlir_bin_x86_64_pc_windows_msvc::*;
#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
pub use xrack_mlir_bin_x86_64_unknown_linux_gnu::*;

#[cfg(not(any(
    all(target_os = "macos", target_arch = "aarch64"),
    all(
        target_os = "linux",
        any(target_arch = "aarch64", target_arch = "x86_64"),
        target_env = "gnu",
    ),
    all(
        target_os = "windows",
        any(target_arch = "aarch64", target_arch = "x86_64"),
        target_env = "msvc",
    ),
)))]
compile_error!("XRack has no bundled MLIR library for this target");
