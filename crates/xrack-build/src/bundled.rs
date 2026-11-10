use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use xrackc_bin_aarch64_apple_darwin::COMPILER;
#[cfg(all(target_os = "windows", target_arch = "aarch64", target_env = "msvc"))]
use xrackc_bin_aarch64_pc_windows_msvc::COMPILER;
#[cfg(all(target_os = "linux", target_arch = "aarch64", target_env = "gnu"))]
use xrackc_bin_aarch64_unknown_linux_gnu::COMPILER;
#[cfg(all(target_os = "windows", target_arch = "x86_64", target_env = "msvc"))]
use xrackc_bin_x86_64_pc_windows_msvc::COMPILER;
#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
use xrackc_bin_x86_64_unknown_linux_gnu::COMPILER;
#[cfg(not(any(
    all(target_os = "macos", target_arch = "aarch64"),
    all(
        target_os = "linux",
        any(target_arch = "aarch64", target_arch = "x86_64"),
        target_env = "gnu"
    ),
    all(
        target_os = "windows",
        any(target_arch = "aarch64", target_arch = "x86_64"),
        target_env = "msvc"
    ),
)))]
const COMPILER: &[u8] = &[];

pub fn materialize(out: &Path) -> io::Result<PathBuf> {
    if COMPILER.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no bundled xrackc for this host or source checkout; set XRACKC to an executable",
        ));
    }
    let path = out.join(format!("xrackc{}", env::consts::EXE_SUFFIX));
    let temporary = out.join("xrackc-extract.tmp");
    {
        let mut file = fs::File::create(&temporary)?;
        io::copy(&mut flate2::read::GzDecoder::new(COMPILER), &mut file)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o755))?;
    }
    fs::rename(&temporary, &path)?;
    Ok(path)
}
