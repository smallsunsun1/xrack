use crate::IosDirectory;
use crate::native_dialog::until_canceled;
use block2::RcBlock;
use objc2::{
    ClassType, DeclaredClass, declare_class, msg_send_id,
    mutability::MainThreadOnly,
    rc::Retained,
    runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject},
};
use objc2_foundation::{
    CGPoint, CGRect, CGSize, MainThreadMarker, NSArray, NSBundle, NSError, NSString, NSURL,
};
use objc2_ui_kit::{
    UIActivityType, UIActivityViewController, UIAlertAction, UIAlertActionStyle, UIAlertController,
    UIAlertControllerStyle, UIDocumentPickerDelegate, UIDocumentPickerViewController,
    UIModalPresentationStyle, UIViewController, UIViewControllerTransitionCoordinator,
    UIViewControllerTransitionCoordinatorContext,
};
use objc2_uniform_type_identifiers::{UTType, UTTypeFolder, UTTypeItem};
use std::{
    cell::Cell,
    future::Future,
    io,
    path::{Path, PathBuf},
    ptr::NonNull,
    rc::Rc,
    sync::Arc,
};
use tempfile::TempDir;
use tokio::sync::{Mutex, OwnedMutexGuard, oneshot};
use winit::window::Window;
use xrack_runtime::{
    FileDialogMode, FileDialogOptions, MessageButtons, MessageDialogOptions, NativeDialogOptions,
    NativeDialogPresentation, NativeDialogResult, StandardButtonKind,
};

mod color;
mod photos;
pub(crate) use color::ColorRequest;
pub(crate) use photos::PhotoRequest;
pub use photos::{IosPhotoSelection, IosPhotoThumbnail};

// Rust requests and declarative dialogs share the presenting view controller.
pub(crate) type ModalQueue = Arc<Mutex<()>>;

pub(crate) enum Transfer {
    ShareText(String),
    ShareFiles(Vec<PathBuf>),
    ExportFiles(Vec<PathBuf>),
}

pub(crate) struct TransferRequest {
    pub(crate) operation: Transfer,
    pub(crate) reply: oneshot::Sender<Result<bool, String>>,
}

impl TransferRequest {
    pub(crate) async fn present(mut self, window: Arc<Window>, queue: ModalQueue) {
        let Some(permit) = until_canceled(self.reply.closed(), queue.lock_owned()).await else {
            return;
        };
        let result = match self.operation {
            Transfer::ShareText(text) => {
                let text = Retained::into_super(NSString::from_str(&text));
                share(
                    &window,
                    &NSArray::from_vec(vec![Retained::into_super(text)]),
                    self.reply.closed(),
                )
                .await
            }
            Transfer::ShareFiles(paths) => match file_urls(&paths) {
                Ok(urls) => {
                    let items = urls
                        .into_iter()
                        .map(|url| Retained::into_super(Retained::into_super(url)))
                        .collect();
                    share(&window, &NSArray::from_vec(items), self.reply.closed()).await
                }
                Err(error) => Err(error),
            },
            Transfer::ExportFiles(paths) => {
                export_files(&window, paths, permit, &mut self.reply).await
            }
        };
        let _ = self.reply.send(result);
    }
}

pub(crate) struct DirectoryRequest {
    pub(crate) reply: oneshot::Sender<Result<Option<IosDirectory>, String>>,
}

impl DirectoryRequest {
    pub(crate) async fn present(mut self, window: Arc<Window>, queue: ModalQueue) {
        let Some(_permit) = until_canceled(self.reply.closed(), queue.lock_owned()).await else {
            return;
        };
        let result = select_directory(&window, self.reply.closed())
            .await
            .map_err(str::to_owned);
        let _ = self.reply.send(result);
    }
}

async fn select_directory(
    window: &Window,
    canceled: impl Future<Output = ()>,
) -> Result<Option<IosDirectory>, &'static str> {
    let Some(directories) = select_folder_urls(window, "", false, canceled).await? else {
        return Ok(None);
    };
    directories
        .into_iter()
        .next()
        .map(IosDirectory::new)
        .transpose()
}

async fn select_folder_urls(
    window: &Window,
    directory: &str,
    multiple: bool,
    canceled: impl Future<Output = ()>,
) -> Result<Option<Vec<Retained<NSURL>>>, &'static str> {
    let (mtm, host) = presenter(window)?;
    // SAFETY: The controller is created on the UIKit main thread. In-place access
    // preserves the security-scoped URL supplied by the document provider.
    let picker = unsafe {
        let picker = UIDocumentPickerViewController::initForOpeningContentTypes_asCopy(
            mtm.alloc(),
            &NSArray::from_slice(&[UTTypeFolder]),
            false,
        );
        configure_picker(&picker, directory, multiple);
        picker
    };
    present_picker(mtm, &host, &picker, canceled).await
}

fn source_path(path: &Path) -> Result<&str, String> {
    if !path.is_absolute() {
        return Err("The system operation requires an absolute path.".into());
    }
    if !path
        .metadata()
        .map_err(|error| format!("Cannot read source file: {error}"))?
        .is_file()
    {
        return Err("The source must be a file.".into());
    }
    path.to_str()
        .ok_or_else(|| "The source path must be UTF-8.".into())
}

fn file_url(path: &Path) -> Result<Retained<NSURL>, String> {
    let path = source_path(path)?;
    // SAFETY: Construct a file URL from an immutable absolute path.
    Ok(unsafe { NSURL::fileURLWithPath_isDirectory(&NSString::from_str(path), false) })
}

fn file_urls(paths: &[PathBuf]) -> Result<Vec<Retained<NSURL>>, String> {
    paths.iter().map(|path| file_url(path)).collect()
}

async fn share(
    window: &Window,
    items: &NSArray,
    canceled: impl Future<Output = ()>,
) -> Result<bool, String> {
    let (mtm, host) = presenter(window)?;
    let anchor = host
        .view()
        .ok_or("The application window has no presentation view.")?;
    let (send, receive) = oneshot::channel();
    let send = Cell::new(Some(send));
    let completed = RcBlock::new(
        move |_: *mut UIActivityType, completed: Bool, _: *mut NSArray, error: *mut NSError| {
            if let Some(send) = send.take() {
                // SAFETY: UIKit keeps the optional error alive for this callback.
                let result = unsafe { error.as_ref() }.map_or_else(
                    || Ok(completed.as_bool()),
                    |error| Err(error.localizedDescription().to_string()),
                );
                let _ = send.send(result);
            }
        },
    );
    // SAFETY: The share sheet is configured and presented on the UIKit main thread.
    let sheet = unsafe {
        let sheet = UIActivityViewController::initWithActivityItems_applicationActivities(
            mtm.alloc(),
            items,
            None,
        );
        if let Some(popover) = sheet.popoverPresentationController() {
            let bounds = anchor.bounds();
            popover.setSourceView(Some(&anchor));
            popover.setSourceRect(CGRect::new(
                CGPoint::new(
                    bounds.origin.x + bounds.size.width / 2.0,
                    bounds.origin.y + bounds.size.height / 2.0,
                ),
                CGSize::new(1.0, 1.0),
            ));
        }
        sheet.setCompletionWithItemsHandler(std::ptr::from_ref(&*completed).cast_mut());
        sheet
    };
    match present_panel(&host, &sheet, until_canceled(canceled, receive)).await {
        Some(result) => result.map_err(|_| "The share sheet closed without returning a result.")?,
        None => Ok(false),
    }
}

async fn export_files(
    window: &Window,
    paths: Vec<PathBuf>,
    permit: OwnedMutexGuard<()>,
    reply: &mut oneshot::Sender<Result<bool, String>>,
) -> Result<bool, String> {
    for path in &paths {
        source_path(path)?;
    }
    // Exporting through the document picker can leave a copy without a result URL.
    // Select the destination first so the result comes from the coordinated copy.
    let Some(directory) = select_directory(window, reply.closed()).await? else {
        return Ok(false);
    };
    drop(permit);
    if reply.is_closed() {
        return Ok(false);
    }
    let (send, receive) = oneshot::channel();
    std::thread::Builder::new()
        .name("xrack-ios-export".into())
        .spawn(move || {
            let _ = send.send(directory.copy_files(&paths));
        })
        .map_err(|error| format!("Cannot start the Files export: {error}"))?;
    receive
        .await
        .map_err(|_| "The Files export worker stopped before reporting a result.")?
        .map_err(|error| format!("Cannot save to Files: {error}"))?;
    Ok(true)
}

pub(crate) async fn present(
    window: &Window,
    request: &NativeDialogPresentation,
    queue: ModalQueue,
) -> IosDialogResult {
    let Some(permit) = until_canceled(request.wait_until_abandoned(), queue.lock_owned()).await
    else {
        return IosDialogResult::Ready(NativeDialogResult::Abandoned);
    };
    let result = match request.options.as_ref() {
        NativeDialogOptions::File(options) => {
            let selection = open_file(window, options, request).await;
            drop(permit);
            match selection {
                Ok(FileSelection::Folders(urls)) => copy_selected_folders(request, urls).await,
                Ok(FileSelection::Ready(result)) => Ok(IosDialogResult::Ready(result)),
                Err(error) => Err(error.to_owned()),
            }
        }
        NativeDialogOptions::Message(options) => {
            open_message(window, options, request.wait_until_abandoned())
                .await
                .map(IosDialogResult::Ready)
                .map_err(str::to_owned)
        }
    };
    result.unwrap_or_else(|message| IosDialogResult::Ready(NativeDialogResult::Failed(message)))
}

pub(crate) enum IosDialogResult {
    Ready(NativeDialogResult),
    ImportedFolders(ImportedFolders),
}

pub(crate) struct ImportedFolders {
    pub(crate) paths: Vec<String>,
    pub(crate) directory: TempDir,
}

enum FileSelection {
    Ready(NativeDialogResult),
    Folders(Vec<Retained<NSURL>>),
}

#[derive(Clone, Copy)]
enum FolderChoice {
    Add,
    Import,
    Cancel,
}

async fn choose_folders(
    window: &Window,
    options: &FileDialogOptions,
    request: &NativeDialogPresentation,
) -> Result<FileSelection, &'static str> {
    let multiple = options.mode == FileDialogMode::SelectFolders;
    let mut chosen = Vec::new();
    loop {
        let Some(urls) = select_folder_urls(
            window,
            &options.directory,
            multiple,
            request.wait_until_abandoned(),
        )
        .await?
        else {
            return Ok(FileSelection::Ready(NativeDialogResult::Abandoned));
        };
        if urls.is_empty() {
            return Ok(FileSelection::Ready(NativeDialogResult::Canceled));
        }
        if !multiple {
            return Ok(FileSelection::Folders(urls));
        }
        for url in urls {
            if !chosen.contains(&url) {
                chosen.push(url);
            }
        }
        // In folder-only views, Files may show only Open for the current folder.
        // A confirmation panel lets the user collect a batch across those views.
        let names = chosen
            .iter()
            .filter_map(|url| {
                // SAFETY: Each immutable URL is retained from the delegate callback.
                unsafe { url.lastPathComponent() }.map(|name| name.to_string())
            })
            .collect::<Vec<_>>()
            .join("\n");
        let choice = choose_alert(
            window,
            "Selected folders",
            &names,
            &[
                (
                    "Import selected folders",
                    FolderChoice::Import,
                    UIAlertActionStyle::Default,
                ),
                (
                    "Add another folder",
                    FolderChoice::Add,
                    UIAlertActionStyle::Default,
                ),
                ("Cancel", FolderChoice::Cancel, UIAlertActionStyle::Cancel),
            ],
            request.wait_until_abandoned(),
        )
        .await?;
        match choice {
            Some(FolderChoice::Add) => {}
            Some(FolderChoice::Import) => return Ok(FileSelection::Folders(chosen)),
            Some(FolderChoice::Cancel) => {
                return Ok(FileSelection::Ready(NativeDialogResult::Canceled));
            }
            None => return Ok(FileSelection::Ready(NativeDialogResult::Abandoned)),
        }
    }
}

async fn copy_selected_folders(
    request: &NativeDialogPresentation,
    urls: Vec<Retained<NSURL>>,
) -> Result<IosDialogResult, String> {
    if request.is_abandoned() {
        return Ok(IosDialogResult::Ready(NativeDialogResult::Abandoned));
    }
    let (send, receive) = oneshot::channel();
    std::thread::Builder::new()
        .name("xrack-ios-folder-import".into())
        .spawn(move || {
            let result = copy_folders(urls, || send.is_closed());
            let _ = send.send(result);
        })
        .map_err(|error| format!("Cannot start the Files import: {error}"))?;
    let Some(result) = until_canceled(request.wait_until_abandoned(), receive).await else {
        return Ok(IosDialogResult::Ready(NativeDialogResult::Abandoned));
    };
    result
        .map_err(|_| "The Files import worker stopped before reporting a result.".to_owned())?
        .map(IosDialogResult::ImportedFolders)
        .map_err(|error| format!("Cannot import the selected folders: {error}"))
}

fn copy_folders(
    urls: Vec<Retained<NSURL>>,
    canceled: impl Fn() -> bool,
) -> io::Result<ImportedFolders> {
    let directory = tempfile::Builder::new().prefix("xrack-import-").tempdir()?;
    let mut paths = Vec::with_capacity(urls.len());
    for (index, url) in urls.into_iter().enumerate() {
        if canceled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Folder selection canceled",
            ));
        }
        let source = IosDirectory::new(url).map_err(io::Error::other)?;
        let staging = directory.path().join(index.to_string());
        std::fs::create_dir(&staging)?;
        let path = source.import_copy(&staging)?;
        paths.push(
            path.into_os_string()
                .into_string()
                .map_err(|_| io::Error::other("The copied folder path is not UTF-8"))?,
        );
    }
    Ok(ImportedFolders { paths, directory })
}

fn presenter(
    window: &Window,
) -> Result<(MainThreadMarker, Retained<UIViewController>), &'static str> {
    let mtm = MainThreadMarker::new().ok_or("System dialogs require the UIKit main thread.")?;
    let host = crate::ios::view(window)
        .and_then(|view| view.window())
        .and_then(|window| window.rootViewController())
        .ok_or("The application window has no UIKit root view controller.")?;
    // SAFETY: The application window and its controller belong to the UIKit main thread.
    if unsafe { host.presentedViewController().is_some() } {
        return Err("Another system dialog is still visible.");
    }
    Ok((mtm, host))
}

async fn open_file(
    window: &Window,
    options: &FileDialogOptions,
    request: &NativeDialogPresentation,
) -> Result<FileSelection, &'static str> {
    let multiple = match options.mode {
        FileDialogMode::OpenFile => false,
        FileDialogMode::OpenFiles => true,
        FileDialogMode::SelectFolder | FileDialogMode::SelectFolders => {
            return choose_folders(window, options, request).await;
        }
        FileDialogMode::SaveFile => {
            return Err(
                "The iOS Files picker imports existing files. This dialog cannot choose a path for a new file.",
            );
        }
    };
    let filters = options.filters()?;
    let (mtm, host) = presenter(window)?;

    // SAFETY: The controller, delegate and window are confined to the UIKit main thread.
    let picker = unsafe {
        let types = if filters.is_empty() || filters.iter().any(|(_, list)| list.contains(&"*")) {
            NSArray::from_slice(&[UTTypeItem])
        } else {
            let types = filters
                .iter()
                .flat_map(|(_, list)| list)
                .map(|ext| UTType::typeWithFilenameExtension(&NSString::from_str(ext)))
                .collect::<Option<Vec<_>>>()
                .ok_or("The system cannot identify a file type from the specified extension.")?;
            NSArray::from_vec(types)
        };
        // File imports receive a local copy owned by the application.
        let picker = UIDocumentPickerViewController::initForOpeningContentTypes_asCopy(
            mtm.alloc(),
            &types,
            true,
        );
        configure_picker(&picker, &options.directory, multiple);
        picker
    };
    let Some(urls) = present_picker(mtm, &host, &picker, request.wait_until_abandoned()).await?
    else {
        return Ok(FileSelection::Ready(NativeDialogResult::Abandoned));
    };
    if urls.is_empty() {
        return Ok(FileSelection::Ready(NativeDialogResult::Canceled));
    }
    let paths = urls
        .iter()
        .map(|url| {
            // SAFETY: The retained URLs come from the document picker in copy mode.
            unsafe { url.isFileURL().then(|| url.path()).flatten() }.map(|path| path.to_string())
        })
        .collect::<Option<Vec<_>>>()
        .ok_or("The picker returned an invalid file URL.")?;
    Ok(FileSelection::Ready(NativeDialogResult::Files(paths)))
}

fn configure_picker(picker: &UIDocumentPickerViewController, directory: &str, multiple: bool) {
    // SAFETY: Both the picker and its starting directory are configured on the main thread.
    unsafe {
        picker.setAllowsMultipleSelection(multiple);
        if !directory.is_empty() {
            let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(directory), true);
            picker.setDirectoryURL(Some(&url));
        }
    }
}

async fn present_picker(
    mtm: MainThreadMarker,
    host: &UIViewController,
    picker: &UIDocumentPickerViewController,
    canceled: impl Future<Output = ()>,
) -> Result<Option<Vec<Retained<NSURL>>>, &'static str> {
    let (result, finished) = oneshot::channel();
    let delegate = PickerDelegate::new(mtm, result);
    // SAFETY: The controller, delegate and window are confined to the UIKit main thread.
    unsafe {
        // Full-screen presentation uses the picker's Cancel button, including on iPad.
        picker.setModalPresentationStyle(UIModalPresentationStyle::UIModalPresentationFullScreen);
        picker.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    }
    let result = present_panel(host, picker, until_canceled(canceled, finished)).await;
    // UIKit keeps the delegate weakly. Hold it across the wait.
    drop(delegate);
    result
        .transpose()
        .map_err(|_| "The system did not return a file selection.")
}

async fn open_message(
    window: &Window,
    options: &MessageDialogOptions,
    canceled: impl Future<Output = ()>,
) -> Result<NativeDialogResult, &'static str> {
    let (title, message) = if options.title.is_empty() {
        (options.text.as_str(), options.informative_text.clone())
    } else {
        let message = [options.text.as_str(), options.informative_text.as_str()]
            .into_iter()
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        (options.title.as_str(), message)
    };
    let ok = ("OK", StandardButtonKind::Ok, UIAlertActionStyle::Default);
    let yes = ("Yes", StandardButtonKind::Yes, UIAlertActionStyle::Default);
    let no = ("No", StandardButtonKind::No, UIAlertActionStyle::Default);
    let cancel = (
        "Cancel",
        StandardButtonKind::Cancel,
        UIAlertActionStyle::Cancel,
    );
    let buttons: &[_] = match options.buttons {
        MessageButtons::Ok => &[ok],
        MessageButtons::OkCancel => &[ok, cancel],
        MessageButtons::YesNo => &[yes, no],
        MessageButtons::YesNoCancel => &[yes, no, cancel],
    };
    Ok(choose_alert(window, title, &message, buttons, canceled)
        .await?
        .map_or(NativeDialogResult::Abandoned, NativeDialogResult::Button))
}

async fn choose_alert<T: Copy + 'static>(
    window: &Window,
    title: &str,
    message: &str,
    buttons: &[(&str, T, UIAlertActionStyle)],
    canceled: impl Future<Output = ()>,
) -> Result<Option<T>, &'static str> {
    let (mtm, host) = presenter(window)?;
    let title = (!title.is_empty()).then(|| NSString::from_str(title));
    let message = (!message.is_empty()).then(|| NSString::from_str(message));
    let (send, receive) = oneshot::channel();
    let result = Rc::new(Cell::new(Some(send)));
    // SAFETY: The alert and its action handlers run on the UIKit main thread.
    let alert = unsafe {
        let alert = UIAlertController::alertControllerWithTitle_message_preferredStyle(
            title.as_deref(),
            message.as_deref(),
            UIAlertControllerStyle::Alert,
            mtm,
        );
        let bundle = NSBundle::mainBundle();
        for &(label, button, style) in buttons {
            let label =
                bundle.localizedStringForKey_value_table(&NSString::from_str(label), None, None);
            let result = result.clone();
            let action = RcBlock::new(move |_: NonNull<UIAlertAction>| {
                if let Some(send) = result.take() {
                    let _ = send.send(button);
                }
            });
            alert.addAction(&UIAlertAction::actionWithTitle_style_handler(
                Some(&label),
                style,
                Some(&action),
                mtm,
            ));
        }
        alert
    };
    drop(result);
    present_panel(&host, &alert, until_canceled(canceled, receive))
        .await
        .transpose()
        .map_err(|_| "The system did not return a button selection.")
}

async fn present_panel<T>(
    host: &UIViewController,
    controller: &UIViewController,
    result: impl Future<Output = T>,
) -> T {
    let (send, presented) = oneshot::channel();
    let send = Cell::new(Some(send));
    let complete = RcBlock::new(move || {
        if let Some(send) = send.take() {
            let _ = send.send(());
        }
    });
    let _panel = PresentedPanel(controller);
    // SAFETY: Both view controllers belong to the UIKit main thread.
    unsafe { host.presentViewController_animated_completion(controller, true, Some(&complete)) };

    let result = result.await;
    // SAFETY: When a caller is canceled during presentation, wait until UIKit
    // can dismiss the panel and finish its transition before passing the queue on.
    if unsafe { controller.isBeingPresented() } {
        let _ = presented.await;
    }
    wait_for_dismissal(controller).await;
    result
}

struct PresentedPanel<'a>(&'a UIViewController);

impl Drop for PresentedPanel<'_> {
    fn drop(&mut self) {
        // SAFETY: The window executor drops the guard on the UIKit main thread.
        unsafe {
            if self.0.presentingViewController().is_some() && !self.0.isBeingDismissed() {
                self.0.dismissViewControllerAnimated_completion(false, None);
            }
        }
    }
}

async fn wait_for_dismissal(controller: &UIViewController) {
    let (send, receive) = oneshot::channel();
    let send = Rc::new(Cell::new(Some(send)));
    // SAFETY: The controller is used on the UIKit main thread. Retain the
    // transition completion until UIKit has removed the controller.
    unsafe {
        if controller.presentingViewController().is_none() {
            return;
        }
        let mut observing = false;
        if controller.isBeingDismissed()
            && let Some(transition) = controller.transitionCoordinator()
        {
            let send = send.clone();
            let complete = RcBlock::new(
                move |_: NonNull<
                    ProtocolObject<dyn UIViewControllerTransitionCoordinatorContext>,
                >| {
                    if let Some(send) = send.take() {
                        let _ = send.send(());
                    }
                },
            );
            observing = transition.animateAlongsideTransition_completion(None, Some(&complete));
        }
        if !observing {
            let complete = RcBlock::new(move || {
                if let Some(send) = send.take() {
                    let _ = send.send(());
                }
            });
            controller.dismissViewControllerAnimated_completion(false, Some(&complete));
        }
    }
    let _ = receive.await;
}

declare_class!(
    struct PickerDelegate;

    unsafe impl ClassType for PickerDelegate {
        type Super = NSObject;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "XRackDocumentPickerDelegate";
    }

    impl DeclaredClass for PickerDelegate {
        type Ivars = Cell<Option<oneshot::Sender<Vec<Retained<NSURL>>>>>;
    }

    unsafe impl NSObjectProtocol for PickerDelegate {}

    #[allow(non_snake_case)]
    unsafe impl UIDocumentPickerDelegate for PickerDelegate {
        #[method(documentPicker:didPickDocumentsAtURLs:)]
        unsafe fn documentPicker_didPickDocumentsAtURLs(
            &self,
            picker: &UIDocumentPickerViewController,
            urls: &NSArray<NSURL>,
        ) {
            self.finish(picker, urls.iter_retained().collect());
        }

        #[method(documentPickerWasCancelled:)]
        unsafe fn documentPickerWasCancelled(&self, picker: &UIDocumentPickerViewController) {
            self.finish(picker, Vec::new());
        }
    }
);

impl PickerDelegate {
    fn new(mtm: MainThreadMarker, result: oneshot::Sender<Vec<Retained<NSURL>>>) -> Retained<Self> {
        let delegate = mtm.alloc::<Self>().set_ivars(Cell::new(Some(result)));
        // SAFETY: NSObject initializes the allocated delegate with its Rust ivars in place.
        unsafe { msg_send_id![super(delegate), init] }
    }

    fn finish(&self, picker: &UIDocumentPickerViewController, result: Vec<Retained<NSURL>>) {
        let Some(sender) = self.ivars().take() else {
            return;
        };
        let delivery = Cell::new(Some((sender, result)));
        let complete = || {
            if let Some((sender, result)) = delivery.take() {
                let _ = sender.send(result);
            }
        };
        // SAFETY: These calls run in the main-thread UIKit delegate.
        unsafe {
            if picker.presentingViewController().is_none() {
                complete();
            } else {
                let completion = RcBlock::new(move || {
                    if let Some((sender, result)) = delivery.take() {
                        let _ = sender.send(result);
                    }
                });
                // Keep the native dialog queue occupied until dismissal finishes.
                picker.dismissViewControllerAnimated_completion(false, Some(&completion));
            }
        }
    }
}
