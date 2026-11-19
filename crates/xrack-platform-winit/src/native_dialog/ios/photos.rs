use super::{ModalQueue, present_panel, presenter, until_canceled};
use block2::RcBlock;
use objc2::{
    ClassType, DeclaredClass, ProtocolType, class, declare_class, extern_class, extern_protocol,
    msg_send, msg_send_id,
    mutability::{IsMainThreadOnly, MainThreadOnly},
    rc::Retained,
    runtime::{NSObject, NSObjectProtocol, ProtocolObject},
};
use objc2_foundation::{MainThreadMarker, NSArray, NSError, NSItemProvider, NSProgress, NSURL};
use objc2_ui_kit::{UIModalPresentationStyle, UIViewController};
use objc2_uniform_type_identifiers::{UTType, UTTypeImage};
use std::{
    cell::Cell,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tempfile::TempDir;
use tokio::sync::oneshot;
use winit::window::Window;

mod thumbnail;
pub use thumbnail::IosPhotoThumbnail;

// PhotosUI's iOS picker is outside the generated bindings for objc2 0.5.
#[link(name = "PhotosUI", kind = "framework")]
unsafe extern "C" {}

extern_class!(
    struct PHPickerViewController;
    unsafe impl ClassType for PHPickerViewController {
        type Super = UIViewController;
        type Mutability = MainThreadOnly;
    }
);

extern_protocol!(
    unsafe trait PHPickerViewControllerDelegate: NSObjectProtocol + IsMainThreadOnly {
        #[method(picker:didFinishPicking:)]
        unsafe fn picker_did_finish_picking(
            &self,
            picker: &PHPickerViewController,
            results: &NSArray<NSObject>,
        );
    }

    unsafe impl ProtocolType for dyn PHPickerViewControllerDelegate {}
);

/// App-local copies of images explicitly selected in the iOS photo picker.
pub struct IosPhotoSelection {
    paths: Vec<PathBuf>,
    _directory: Arc<TempDir>,
}

impl IosPhotoSelection {
    /// Paths in selection order. They are available while this selection is held.
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Decode a bounded, orientation-correct preview using iOS ImageIO.
    /// This operation performs file I/O and can run on a background thread.
    pub fn thumbnail(&self, index: usize, maximum_edge: u32) -> Result<IosPhotoThumbnail, String> {
        let path = self
            .paths
            .get(index)
            .ok_or("The photo index is out of range.")?;
        thumbnail::load(path, maximum_edge)
    }
}

pub(crate) struct PhotoRequest {
    pub(crate) limit: isize,
    pub(crate) reply: oneshot::Sender<Result<Option<IosPhotoSelection>, String>>,
}

impl PhotoRequest {
    pub(crate) async fn present(mut self, window: Arc<Window>, queue: ModalQueue) {
        let Some(permit) = until_canceled(self.reply.closed(), queue.lock_owned()).await else {
            return;
        };
        let selection = select_photo_providers(&window, self.limit, &mut self.reply).await;
        drop(permit);
        let result = match selection {
            Ok(providers) => until_canceled(self.reply.closed(), import_photos(providers)).await,
            Err(error) => Some(Err(error)),
        };
        if let Some(result) = result {
            let _ = self.reply.send(result);
        }
    }
}

async fn select_photo_providers(
    window: &Window,
    limit: isize,
    reply: &mut oneshot::Sender<Result<Option<IosPhotoSelection>, String>>,
) -> Result<Vec<Retained<NSItemProvider>>, String> {
    let (mtm, host) = presenter(window)?;
    let (send, receive) = oneshot::channel();
    let delegate = PhotoDelegate::new(mtm, send);
    // SAFETY: These are public PhotosUI selectors. Limit, representation and
    // selection mode are NSInteger values as declared in PHPicker.h.
    let picker: Retained<PHPickerViewController> = unsafe {
        let config: Retained<NSObject> = msg_send_id![class!(PHPickerConfiguration), new];
        let filter: Retained<NSObject> = msg_send_id![class!(PHPickerFilter), imagesFilter];
        let _: () = msg_send![&config, setFilter: &*filter];
        let _: () = msg_send![&config, setSelectionLimit: limit];
        // PHPickerConfigurationAssetRepresentationModeCurrent preserves the source format.
        let _: () = msg_send![&config, setPreferredAssetRepresentationMode: 1_isize];
        // PHPickerConfigurationSelectionOrdered preserves the user's pick order.
        if limit > 1 {
            let _: () = msg_send![&config, setSelection: 1_isize];
        }
        let picker: Retained<PHPickerViewController> =
            msg_send_id![mtm.alloc::<PHPickerViewController>(), initWithConfiguration: &*config];
        let protocol: &ProtocolObject<dyn PHPickerViewControllerDelegate> =
            ProtocolObject::from_ref(&*delegate);
        let _: () = msg_send![&picker, setDelegate: protocol];
        picker.setModalPresentationStyle(UIModalPresentationStyle::UIModalPresentationFullScreen);
        picker
    };
    let providers = present_panel(&host, &picker, until_canceled(reply.closed(), receive)).await;
    drop(delegate);
    match providers {
        Some(result) => result.map_err(|_| "The system did not return a photo selection.".into()),
        None => Ok(Vec::new()),
    }
}

async fn import_photos(
    providers: Vec<Retained<NSItemProvider>>,
) -> Result<Option<IosPhotoSelection>, String> {
    if providers.is_empty() {
        return Ok(None);
    }
    let directory = Arc::new(
        tempfile::Builder::new()
            .prefix("xrack-photos-")
            .tempdir()
            .map_err(|error| format!("Cannot create the photo import directory: {error}"))?,
    );
    let mut paths = Vec::with_capacity(providers.len());
    for (index, provider) in providers.iter().enumerate() {
        paths.push(load_photo(provider, directory.clone(), index + 1).await?);
    }
    Ok(Some(IosPhotoSelection {
        paths,
        _directory: directory,
    }))
}

declare_class!(
    struct PhotoDelegate;

    unsafe impl ClassType for PhotoDelegate {
        type Super = NSObject;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "XRackPhotoPickerDelegate";
    }

    impl DeclaredClass for PhotoDelegate {
        type Ivars = Cell<Option<oneshot::Sender<Vec<Retained<NSItemProvider>>>>>;
    }

    unsafe impl NSObjectProtocol for PhotoDelegate {}

    unsafe impl PHPickerViewControllerDelegate for PhotoDelegate {
        #[method(picker:didFinishPicking:)]
        unsafe fn picker_did_finish_picking(
            &self,
            _: &PHPickerViewController,
            results: &NSArray<NSObject>,
        ) {
            if let Some(send) = self.ivars().take() {
                let providers = results.iter_retained().map(|result| {
                    // SAFETY: PHPicker supplies PHPickerResult objects in this array.
                    unsafe { msg_send_id![&*result, itemProvider] }
                }).collect();
                let _ = send.send(providers);
            }
        }
    }
);

impl PhotoDelegate {
    fn new(
        mtm: MainThreadMarker,
        send: oneshot::Sender<Vec<Retained<NSItemProvider>>>,
    ) -> Retained<Self> {
        let delegate = mtm.alloc::<Self>().set_ivars(Cell::new(Some(send)));
        // SAFETY: NSObject initializes the delegate after its ivars have been set.
        unsafe { msg_send_id![super(delegate), init] }
    }
}

async fn load_photo(
    provider: &NSItemProvider,
    directory: Arc<TempDir>,
    number: usize,
) -> Result<PathBuf, String> {
    // SAFETY: The provider and declared image representations are owned by the picker result.
    let (identifier, extension) = unsafe {
        provider
            .registeredTypeIdentifiers()
            .iter_retained()
            .find_map(|identifier| {
                let ty = UTType::typeWithIdentifier(&identifier)?;
                ty.conformsToType(UTTypeImage)
                    .then(|| (identifier, ty.preferredFilenameExtension()))
            })
    }
    .ok_or("The selected asset has no image representation.")?;
    let extension = extension
        .as_deref()
        .map(ToString::to_string)
        .filter(|value| {
            !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
        });
    let destination = directory.path().join(extension.map_or_else(
        || format!("photo-{number}"),
        |ext| format!("photo-{number}.{ext}"),
    ));
    let (send, receive) = oneshot::channel();
    let send = Mutex::new(Some(send));
    let callback = RcBlock::new(move |url: *mut NSURL, error: *mut NSError| {
        let _directory = &directory;
        if let Ok(mut send) = send.lock()
            && let Some(send) = send.take()
        {
            if send.is_closed() {
                return;
            }
            // SAFETY: The provider retains the URL and error until this callback returns.
            let result = unsafe {
                if let Some(error) = error.as_ref() {
                    Err(error.localizedDescription().to_string())
                } else {
                    url.as_ref()
                        .ok_or_else(|| "Photo provider did not return a file.".into())
                        .and_then(|url| copy_photo(url, &destination))
                }
            };
            let _ = send.send(result);
        }
    });
    // SAFETY: The callback retains only thread-safe Rust values. The photo is
    // copied before its provider-owned file URL expires.
    let progress = Progress(unsafe {
        provider.loadFileRepresentationForTypeIdentifier_completionHandler(&identifier, &callback)
    });
    let result = receive
        .await
        .map_err(|_| "The photo provider closed without returning an image.")?;
    drop(progress);
    result
}

fn copy_photo(url: &NSURL, target: &Path) -> Result<PathBuf, String> {
    // SAFETY: The immutable URL is live for the provider callback.
    let source = unsafe {
        if !url.isFileURL()
            || url
                .host()
                .is_some_and(|host| !host.is_empty() && host.to_string() != "localhost")
        {
            return Err("The photo provider did not return a local file.".into());
        }
        PathBuf::from(
            url.path()
                .ok_or("The selected photo has no file path.")?
                .to_string(),
        )
    };
    if !source.is_absolute()
        || !fs::symlink_metadata(&source).is_ok_and(|metadata| metadata.is_file())
    {
        return Err("The selected photo is not a readable file.".into());
    }
    fs::copy(source, target).map_err(|error| format!("Cannot copy the selected photo: {error}"))?;
    Ok(target.to_path_buf())
}

struct Progress(Retained<NSProgress>);

impl Drop for Progress {
    fn drop(&mut self) {
        // SAFETY: NSProgress supports cancellation from the caller's thread.
        unsafe {
            if !self.0.isFinished() {
                self.0.cancel();
            }
        }
    }
}
