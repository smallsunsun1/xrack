use block2::RcBlock;
use objc2::{rc::Retained, rc::autoreleasepool};
use objc2_foundation::{NSFileCoordinator, NSFileCoordinatorReadingOptions, NSString, NSURL};
use std::{
    cell::{Cell, RefCell},
    io,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    path::{Component, Path, PathBuf},
    ptr::NonNull,
};

/// A directory granted by the iOS Files picker for this application session.
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

            // SAFETY: The original picker URL retains the security scope. The guard
            // balances each successful acquisition, including error paths.
            if !unsafe { self.url.startAccessingSecurityScopedResource() } {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Files did not grant access to the selected directory",
                ));
            }
            let _scope = SecurityScope(&self.url);

            let read = Cell::new(Some(read));
            let result = RefCell::new(None);
            let accessor = RcBlock::new(|url: NonNull<NSURL>| {
                let Some(read) = read.take() else {
                    return;
                };
                // SAFETY: The supplied URL is valid for the synchronous accessor.
                let path = unsafe { url.as_ref().path() };
                let value = catch_unwind(AssertUnwindSafe(|| {
                    let path =
                        path.ok_or_else(|| io::Error::other("Files supplied no local path"))?;
                    read(&PathBuf::from(path.to_string()))
                }));
                result.replace(Some(value));
            });
            let mut error = None;
            // SAFETY: The coordinator invokes this accessor synchronously. Its
            // captures and the security scope remain alive throughout the call.
            unsafe {
                NSFileCoordinator::new().coordinateReadingItemAtURL_options_error_byAccessor(
                    &url,
                    NSFileCoordinatorReadingOptions::empty(),
                    Some(&mut error),
                    &accessor,
                );
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
        })
    }
}

struct SecurityScope<'a>(&'a NSURL);

impl Drop for SecurityScope<'_> {
    fn drop(&mut self) {
        // SAFETY: This guard exists only after starting access on the same URL.
        unsafe { self.0.stopAccessingSecurityScopedResource() };
    }
}
