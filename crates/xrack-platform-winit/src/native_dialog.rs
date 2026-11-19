#[cfg(not(any(target_os = "ios", target_os = "android")))]
use crate::Window;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
use xrack_runtime::{NativeDialogOptions, NativeDialogResult};

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub(crate) use android::{
    AndroidDialogResult, ImportedFiles, SystemRequest as AndroidSystemRequest, present,
};

#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "ios")]
pub(crate) use ios::{
    ColorRequest, DirectoryRequest, ModalQueue, PhotoRequest, Transfer, TransferRequest, present,
};
#[cfg(target_os = "ios")]
pub use ios::{IosPhotoSelection, IosPhotoThumbnail};

#[cfg(any(target_os = "ios", test))]
async fn until_canceled<T>(
    canceled: impl std::future::Future<Output = ()>,
    operation: impl std::future::Future<Output = T>,
) -> Option<T> {
    use std::{future::poll_fn, pin::pin, task::Poll};

    let mut canceled = pin!(canceled);
    let mut operation = pin!(operation);
    poll_fn(|context| {
        if canceled.as_mut().poll(context).is_ready() {
            Poll::Ready(None)
        } else {
            operation.as_mut().poll(context).map(Some)
        }
    })
    .await
}

#[cfg(any(target_os = "ios", test))]
pub(super) async fn receive_color_with_preview(
    mut result: tokio::sync::oneshot::Receiver<Result<xrack_runtime::Color, String>>,
    mut previews: tokio::sync::watch::Receiver<xrack_runtime::Color>,
    mut on_preview: impl FnMut(xrack_runtime::Color),
) -> Result<xrack_runtime::Color, String> {
    use std::{
        future::poll_fn,
        pin::{Pin, pin},
        task::Poll,
    };

    enum Update {
        Preview(Option<xrack_runtime::Color>),
        Finished(Result<xrack_runtime::Color, String>),
    }

    let closed = || "the window closed before the color picker finished".to_owned();
    loop {
        let mut next = pin!(async {
            previews.changed().await.ok()?;
            Some(*previews.borrow_and_update())
        });
        let update = poll_fn(|context| {
            if let Poll::Ready(value) = Pin::new(&mut result).poll(context) {
                return Poll::Ready(Update::Finished(value.unwrap_or_else(|_| Err(closed()))));
            }
            next.as_mut().poll(context).map(Update::Preview)
        })
        .await;
        match update {
            Update::Finished(value) => return value,
            Update::Preview(Some(color)) => on_preview(color),
            Update::Preview(None) => return result.await.map_err(|_| closed())?,
        }
    }
}

#[cfg(test)]
#[test]
fn color_preview_coalesces_updates_and_yields_to_the_final_result() {
    use std::{
        cell::RefCell,
        pin::pin,
        task::{Context, Poll, Waker},
    };
    use tokio::sync::{oneshot, watch};
    use xrack_runtime::Color;

    let previews = RefCell::new(Vec::new());
    let (send, result) = oneshot::channel();
    let (update, updates) = watch::channel(Color(0x10203080));
    let mut selection = pin!(receive_color_with_preview(result, updates, |color| {
        previews.borrow_mut().push(color);
    }));
    let mut context = Context::from_waker(Waker::noop());
    assert!(selection.as_mut().poll(&mut context).is_pending());
    assert!(previews.borrow().is_empty());

    let latest = Color(0x30405080);
    update.send(Color(0x20304080)).unwrap();
    update.send(latest).unwrap();
    assert!(selection.as_mut().poll(&mut context).is_pending());
    assert_eq!(*previews.borrow(), [latest]);

    update.send(Color(0x40506080)).unwrap();
    let final_color = Color(0x50607080);
    send.send(Ok(final_color)).unwrap();
    assert_eq!(
        selection.as_mut().poll(&mut context),
        Poll::Ready(Ok(final_color))
    );
    assert_eq!(*previews.borrow(), [latest]);
}

#[cfg(test)]
#[test]
fn canceling_a_waiting_receiver_leaves_the_next_panel_queued() {
    use std::{
        future::Future,
        pin::pin,
        sync::Arc,
        task::{Context, Poll, Waker},
    };
    use tokio::sync::{Mutex, oneshot};

    let queue = Arc::new(Mutex::new(()));
    let current = queue.clone().try_lock_owned().unwrap();
    let (mut reply, receiver) = oneshot::channel::<()>();
    let mut canceled = pin!(until_canceled(reply.closed(), queue.clone().lock_owned()));
    let mut next = pin!(queue.lock_owned());
    let mut context = Context::from_waker(Waker::noop());
    assert!(canceled.as_mut().poll(&mut context).is_pending());
    assert!(next.as_mut().poll(&mut context).is_pending());

    drop(receiver);
    assert!(matches!(
        canceled.as_mut().poll(&mut context),
        Poll::Ready(None)
    ));
    assert!(next.as_mut().poll(&mut context).is_pending());
    drop(current);
    assert!(next.as_mut().poll(&mut context).is_ready());
}

#[cfg(test)]
#[test]
fn cancellation_takes_priority_over_a_ready_panel_result() {
    use std::{
        cell::Cell,
        future::ready,
        pin::pin,
        task::{Context, Poll, Waker},
    };

    let polled = Cell::new(false);
    let mut response = pin!(until_canceled(ready(()), async { polled.set(true) }));
    let mut context = Context::from_waker(Waker::noop());
    assert!(matches!(
        response.as_mut().poll(&mut context),
        Poll::Ready(None)
    ));
    assert!(!polled.get());
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub(crate) async fn present(window: &Window, options: &NativeDialogOptions) -> NativeDialogResult {
    catch_backend(async {
        match options {
            NativeDialogOptions::File(options) => present_file(window, options).await,
            NativeDialogOptions::Message(options) => present_message(window, options).await,
        }
    })
    .await
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
async fn catch_backend(
    dialog: impl std::future::Future<Output = NativeDialogResult>,
) -> NativeDialogResult {
    use std::{
        panic::{AssertUnwindSafe, catch_unwind},
        task::Poll,
    };

    let mut dialog = std::pin::pin!(dialog);
    // Some native bindings panic if the OS fails to allocate the dialog.
    std::future::poll_fn(|context| {
        catch_unwind(AssertUnwindSafe(|| dialog.as_mut().poll(context))).unwrap_or_else(|_| {
            Poll::Ready(NativeDialogResult::Failed(
                "The system dialog backend failed.".into(),
            ))
        })
    })
    .await
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
async fn present_file(
    window: &Window,
    options: &xrack_runtime::FileDialogOptions,
) -> NativeDialogResult {
    use xrack_runtime::FileDialogMode;

    let mut dialog = rfd::AsyncFileDialog::new().set_parent(window);
    if !options.title.is_empty() {
        dialog = dialog.set_title(&options.title);
    }
    if !options.directory.is_empty() {
        dialog = dialog.set_directory(&options.directory);
    }
    if !options.file_name.is_empty() {
        dialog = dialog.set_file_name(&options.file_name);
    }
    let filters = match options.filters() {
        Ok(filters) => filters,
        Err(message) => return NativeDialogResult::Failed(message.into()),
    };
    // AppKit flattens filter groups into one list of allowed extensions.
    // An all-files group makes that list unrestricted.
    if !cfg!(target_os = "macos")
        || !filters
            .iter()
            .any(|(_, extensions)| extensions.contains(&"*"))
    {
        for (name, extensions) in filters {
            dialog = dialog.add_filter(name, &extensions);
        }
    }
    let selected = match options.mode {
        FileDialogMode::OpenFile => dialog.pick_file().await.map(|file| vec![file]),
        FileDialogMode::OpenFiles => dialog.pick_files().await,
        FileDialogMode::SaveFile => dialog.save_file().await.map(|file| vec![file]),
        FileDialogMode::SelectFolder => dialog.pick_folder().await.map(|file| vec![file]),
        FileDialogMode::SelectFolders => dialog.pick_folders().await,
    };
    let Some(files) = selected.filter(|files| !files.is_empty()) else {
        return NativeDialogResult::Canceled;
    };
    let paths = files
        .iter()
        .map(|file| file.path().to_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>();
    paths.map_or_else(
        || NativeDialogResult::Failed("The selected path cannot be represented as UTF-8.".into()),
        NativeDialogResult::Files,
    )
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
async fn present_message(
    window: &Window,
    options: &xrack_runtime::MessageDialogOptions,
) -> NativeDialogResult {
    use xrack_runtime::{MessageButtons, MessageLevel, StandardButtonKind};

    let level = match options.level {
        MessageLevel::Info => rfd::MessageLevel::Info,
        MessageLevel::Warning => rfd::MessageLevel::Warning,
        MessageLevel::Error => rfd::MessageLevel::Error,
    };
    let buttons = match options.buttons {
        MessageButtons::Ok => rfd::MessageButtons::Ok,
        MessageButtons::OkCancel => rfd::MessageButtons::OkCancel,
        MessageButtons::YesNo => rfd::MessageButtons::YesNo,
        MessageButtons::YesNoCancel => rfd::MessageButtons::YesNoCancel,
    };
    let (title, description) = if options.title.is_empty() {
        (&options.text, options.informative_text.clone())
    } else {
        let description = [options.text.as_str(), options.informative_text.as_str()]
            .into_iter()
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
        (&options.title, description)
    };
    let result = rfd::AsyncMessageDialog::new()
        .set_parent(window)
        .set_title(title)
        .set_description(description)
        .set_level(level)
        .set_buttons(buttons)
        .show()
        .await;
    NativeDialogResult::Button(match result {
        rfd::MessageDialogResult::Ok => StandardButtonKind::Ok,
        rfd::MessageDialogResult::Yes => StandardButtonKind::Yes,
        rfd::MessageDialogResult::No => StandardButtonKind::No,
        rfd::MessageDialogResult::Cancel => StandardButtonKind::Cancel,
        rfd::MessageDialogResult::Custom(_) => {
            return NativeDialogResult::Failed(
                "The system message dialog returned an unknown button.".into(),
            );
        }
    })
}

#[cfg(not(any(
    target_os = "ios",
    target_os = "android",
    target_os = "macos",
    target_os = "windows",
    target_os = "linux"
)))]
pub(crate) async fn present(_: &Window, _: &NativeDialogOptions) -> NativeDialogResult {
    NativeDialogResult::Failed("Native dialogs are unavailable on this platform.".into())
}

#[cfg(all(
    test,
    any(target_os = "macos", target_os = "windows", target_os = "linux")
))]
mod tests {
    use super::{NativeDialogResult, catch_backend};

    #[test]
    fn native_backend_panics_report_failure() {
        let result = pollster::block_on(catch_backend(async { panic!("backend is unavailable") }));
        assert!(matches!(result, NativeDialogResult::Failed(_)));
    }
}
