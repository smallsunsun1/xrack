use block2::RcBlock;
use objc2::{rc::Retained, rc::autoreleasepool, runtime::Bool};
use objc2_foundation::{
    NSData, NSFileCoordinator, NSFileCoordinatorReadingOptions, NSFileCoordinatorWritingOptions,
    NSString, NSURL, NSURLBookmarkCreationOptions, NSURLBookmarkResolutionOptions,
};
use std::{
    cell::{Cell, RefCell},
    fs::File,
    io,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    path::{Component, Path, PathBuf},
    ptr::NonNull,
};
use tempfile::NamedTempFile;

/// A directory granted by the iOS Files picker.
pub struct IosDirectory {
    url: Retained<NSURL>,
}

impl IosDirectory {
    pub(crate) fn new(url: Retained<NSURL>) -> Result<Self, &'static str> {
        // SAFETY: UIKit supplied an immutable URL for the selected directory.
        if unsafe { url.isFileURL() } {
            Ok(Self { url })
        } else {
            Err("The picker returned an invalid directory URL.")
        }
    }

    /// Create an opaque iOS bookmark that the application can keep in private storage.
    /// Call from a worker thread. The bookmark may require renewed authorization later.
    pub fn bookmark_data(&self) -> io::Result<Vec<u8>> {
        autoreleasepool(|_| {
            let _scope = self.require_access()?;
            // SAFETY: Use the picker URL while its security scope is active.
            unsafe {
                self.url
                    .bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
                        NSURLBookmarkCreationOptions::NSURLBookmarkCreationMinimalBookmark,
                        None,
                        None,
                    )
            }
            .map(|data| data.bytes().to_vec())
            .map_err(|error| io::Error::other(error.localizedDescription().to_string()))
        })
    }

    /// Restore a directory from bytes created by [`Self::bookmark_data`].
    /// Call from a worker thread. Resolution does not display a chooser. A stale
    /// bookmark or unavailable directory requires the user to choose the folder again.
    pub fn from_bookmark(data: &[u8]) -> io::Result<Self> {
        autoreleasepool(|_| {
            let data = NSData::with_bytes(data);
            let mut stale = Bool::NO;
            let options = NSURLBookmarkResolutionOptions::NSURLBookmarkResolutionWithoutUI
                | NSURLBookmarkResolutionOptions::NSURLBookmarkResolutionWithoutImplicitStartAccessing;
            // SAFETY: The immutable data and out parameter stay valid for this call.
            let url = unsafe {
                NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
                    &data, options, None, &mut stale,
                )
            }
            .map_err(|error| io::Error::other(error.localizedDescription().to_string()))?;
            if stale.as_bool() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "The folder bookmark is stale",
                ));
            }
            let directory = Self::new(url)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            directory.with_read_access("", |path| {
                if path.metadata()?.is_dir() {
                    Ok(())
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "The bookmark does not refer to a directory",
                    ))
                }
            })?;
            Ok(directory)
        })
    }

    /// Coordinate a read of the directory or a relative item inside it.
    ///
    /// Pass an empty path to enumerate the selected directory. Finish all filesystem
    /// I/O in the callback, while the system grant and coordinated path are valid.
    /// Call from a worker thread because a document provider may block the read.
    pub fn with_read_access<T>(
        &self,
        relative: impl AsRef<Path>,
        read: impl FnOnce(&Path) -> io::Result<T>,
    ) -> io::Result<T> {
        autoreleasepool(|_| {
            let mut url = self.url.clone();
            for part in relative.as_ref().components() {
                let part = match part {
                    Component::CurDir => continue,
                    Component::Normal(part) => part.to_str(),
                    _ => None,
                }
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "Expected a relative UTF-8 directory item",
                    )
                })?;
                // SAFETY: Append a validated component to the retained URL.
                url = unsafe { url.URLByAppendingPathComponent(&NSString::from_str(part)) }
                    .ok_or_else(|| io::Error::other("Could not resolve the directory item"))?;
            }

            let _scope = self.require_access()?;
            coordinated_access(&url, Coordination::Read, |url| {
                // SAFETY: The supplied URL is valid for the synchronous accessor.
                let path = unsafe { url.path() }
                    .ok_or_else(|| io::Error::other("Files supplied no local path"))?;
                read(&PathBuf::from(path.to_string()))
            })
        })
    }

    fn require_access(&self) -> io::Result<SecurityScope<'_>> {
        SecurityScope::start(&self.url).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Files did not grant access to the selected directory",
            )
        })
    }

    pub(crate) fn copy_files(&self, sources: &[PathBuf]) -> io::Result<()> {
        autoreleasepool(|_| {
            // A picker URL may already be writable inside the app's sandbox.
            let _scope = SecurityScope::start(&self.url);
            coordinated_access(&self.url, Coordination::Write, |url| {
                // SAFETY: The coordinator borrows the URL for this synchronous accessor.
                let path = unsafe { url.path() }
                    .ok_or_else(|| io::Error::other("Files supplied no local directory path"))?;
                let directory = PathBuf::from(path.to_string());
                // Stage all copies before publishing any filenames in the selected directory.
                let copies = sources
                    .iter()
                    .map(|source| {
                        let mut input = File::open(source)?;
                        if !input.metadata()?.is_file() {
                            return Err(io::Error::other("The source must be a file"));
                        }
                        let mut copy = tempfile::Builder::new()
                            .prefix(".xrack-export-")
                            .tempfile_in(&directory)?;
                        io::copy(&mut input, copy.as_file_mut())?;
                        copy.as_file().sync_all()?;
                        Ok((source, copy))
                    })
                    .collect::<io::Result<Vec<_>>>()?;
                for (saved, (source, copy)) in copies.into_iter().enumerate() {
                    publish_copy(&directory, source, copy).map_err(|error| {
                        io::Error::new(
                            error.kind(),
                            format!("Saved {saved} of {} files. {error}", sources.len()),
                        )
                    })?;
                }
                Ok(())
            })
        })
    }
}

fn publish_copy(directory: &Path, source: &Path, mut copy: NamedTempFile) -> io::Result<()> {
    let name = source
        .file_name()
        .ok_or_else(|| io::Error::other("The source has no filename"))?;
    let mut target = directory.join(name);
    let mut suffix = 2;
    loop {
        match copy.persist_noclobber(&target) {
            Ok(_) => return Ok(()),
            Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => {
                copy = error.file;
                let mut name = source.file_stem().unwrap_or(name).to_os_string();
                name.push(format!(" ({suffix})"));
                if let Some(extension) = source.extension() {
                    name.push(".");
                    name.push(extension);
                }
                target.set_file_name(name);
                suffix += 1;
            }
            Err(error) => return Err(error.error),
        }
    }
}

pub(crate) fn with_file_url_access<T>(
    url: &NSURL,
    read: impl FnOnce(&NSURL) -> Option<T>,
) -> Option<T> {
    // SAFETY: The file provider keeps the immutable URL valid for this call.
    if !unsafe { url.isFileURL() } {
        return None;
    }
    // A provider may already supply a readable URL in the app's own sandbox.
    let _scope = SecurityScope::start(url);
    coordinated_access(url, Coordination::Read, |url| {
        read(url).ok_or_else(|| io::Error::other("Could not read the dropped file"))
    })
    .ok()
}

enum Coordination {
    Read,
    Write,
}

fn coordinated_access<T>(
    url: &NSURL,
    coordination: Coordination,
    access: impl FnOnce(&NSURL) -> io::Result<T>,
) -> io::Result<T> {
    let access = Cell::new(Some(access));
    let result = RefCell::new(None);
    let accessor = RcBlock::new(|url: NonNull<NSURL>| {
        if let Some(access) = access.take() {
            // SAFETY: The coordinator borrows the file URL for this callback.
            let url = unsafe { url.as_ref() };
            result.replace(Some(catch_unwind(AssertUnwindSafe(|| access(url)))));
        }
    });
    let mut error = None;
    // SAFETY: The coordinator calls the accessor synchronously. The closure
    // and security scope stay alive until the call returns.
    unsafe {
        let coordinator = NSFileCoordinator::new();
        match coordination {
            Coordination::Read => coordinator.coordinateReadingItemAtURL_options_error_byAccessor(
                url,
                NSFileCoordinatorReadingOptions::empty(),
                Some(&mut error),
                &accessor,
            ),
            Coordination::Write => coordinator.coordinateWritingItemAtURL_options_error_byAccessor(
                url,
                NSFileCoordinatorWritingOptions::empty(),
                Some(&mut error),
                &accessor,
            ),
        }
    }
    drop(accessor);
    match result.into_inner() {
        Some(Ok(value)) => value,
        Some(Err(panic)) => resume_unwind(panic),
        None => Err(io::Error::other(error.map_or_else(
            || "Files did not provide access to the selected item".to_owned(),
            |error| error.localizedDescription().to_string(),
        ))),
    }
}

struct SecurityScope<'a>(&'a NSURL);

impl<'a> SecurityScope<'a> {
    fn start(url: &'a NSURL) -> Option<Self> {
        // SAFETY: The guard balances each successful acquisition on the same URL.
        unsafe { url.startAccessingSecurityScopedResource() }.then(|| Self(url))
    }
}

impl Drop for SecurityScope<'_> {
    fn drop(&mut self) {
        // SAFETY: This guard exists only after starting access on the same URL.
        unsafe { self.0.stopAccessingSecurityScopedResource() };
    }
}
