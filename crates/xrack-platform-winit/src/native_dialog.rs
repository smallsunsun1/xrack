use winit::window::Window;
use xrack_runtime::{NativeDialogOptions, NativeDialogResult};

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

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
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
