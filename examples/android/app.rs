use touch_ui::TouchDemo;
use xrack_platform_winit::{AndroidApp, AndroidDirectory, DesktopWindow, run_android, spawn_local};

// android-activity calls the Rust ABI entry point on its event-loop thread.
#[unsafe(no_mangle)]
fn android_main(activity: AndroidApp) {
    let result = run_android(activity, |desktop| {
        let mut app = TouchDemo::new();
        app.set_folder_import_available(true);
        let weak = app.as_weak();
        let reader = weak.clone();
        app.on_read_notes(move |paths| {
            let (Some(mut app), Some(notes)) =
                (reader.upgrade(), pocket_tasks_notes::read_notes(&paths))
            else {
                return false;
            };
            app.set_notes(notes);
            true
        });
        let window = desktop.open_with_viewport(app, |app, viewport| {
            let wide = viewport.width >= 615.0;
            if app.get_wide() != wide {
                app.set_wide(wide);
            }
            let compact = viewport.height < 300.0;
            if app.get_compact() != compact {
                app.set_compact(compact);
            }
        })?;
        let mut app = weak
            .upgrade()
            .ok_or("The Pocket Tasks window was released.")?;
        app.on_import_folder({
            let weak = weak.clone();
            let window = window.clone();
            move || {
                let Some(mut app) = weak.upgrade() else {
                    return;
                };
                let original = app.get_notes();
                let weak = weak.clone();
                let window = window.clone();
                app.set_transfer_pending(true);
                app.set_transfer_status(String::new());
                if spawn_local(async move {
                    let result = import_folder(&window).await;
                    if let Some(mut app) = weak.upgrade() {
                        let status = match result {
                            Ok(Some(_))
                                if app.get_notes_locked() || app.get_notes() != original =>
                            {
                                "Current notes kept".into()
                            }
                            Ok(Some((notes, count))) => {
                                app.set_notes(notes);
                                format!("{count} imported from folder")
                            }
                            Ok(None) => "Folder import canceled".into(),
                            Err(error) => error,
                        };
                        app.set_transfer_pending(false);
                        app.set_transfer_status(status);
                    }
                })
                .is_err()
                {
                    app.set_transfer_pending(false);
                    app.set_transfer_status("The UI event loop is unavailable.".into());
                }
            }
        });
        app.on_transfer_note(move |notes, export_file| {
            let Some(mut app) = weak.upgrade() else {
                return;
            };
            let weak = weak.clone();
            let window = window.clone();
            app.set_transfer_pending(true);
            app.set_transfer_status(String::new());
            if spawn_local(async move {
                let result = if export_file {
                    export_note(&window, notes).await
                } else {
                    window.share_text(notes).await
                };
                if let Some(mut app) = weak.upgrade() {
                    app.set_transfer_pending(false);
                    app.set_transfer_status(match result {
                        Ok(true) if export_file => "Saved to Files".into(),
                        Ok(true) => "Share target selected".into(),
                        Ok(false) if export_file => "Save canceled".into(),
                        Ok(false) => "Share canceled".into(),
                        Err(error) => error,
                    });
                }
            })
            .is_err()
            {
                app.set_transfer_pending(false);
                app.set_transfer_status("The UI event loop is unavailable.".into());
            }
        });
        Ok(())
    });
    if let Err(error) = result {
        eprintln!("Pocket Tasks could not start: {error}");
    }
}

async fn import_folder(window: &DesktopWindow) -> Result<Option<(String, usize)>, String> {
    let Some(directory) = window.select_directory().await? else {
        return Ok(None);
    };
    let (send, receive) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("pocket-tasks-import".into())
        .spawn(move || {
            let _ = send.send(read_folder(directory));
        })
        .map_err(|error| error.to_string())?;
    receive
        .await
        .map_err(|_| "The folder import stopped.")?
        .map(Some)
}

fn read_folder(directory: AndroidDirectory) -> Result<(String, usize), String> {
    let mut files = directory.entries()?;
    files.retain(|file| {
        !file.is_directory()
            && std::path::Path::new(file.name())
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("txt") || extension.eq_ignore_ascii_case("md")
                })
    });
    files.sort_by(|first, second| first.name().cmp(second.name()));
    if files.is_empty() {
        return Err("No text notes in this folder".into());
    }
    let mut notes = String::new();
    for (index, file) in files.iter().enumerate() {
        let separator = if index == 0 { "" } else { "\n\n" };
        let remaining = (64 * 1024_usize)
            .checked_sub(notes.len() + separator.len())
            .ok_or("Notes exceed 64 KiB")?;
        let content = file.read_to_string(remaining)?;
        notes.push_str(separator);
        notes.push_str(&content);
    }
    Ok((notes, files.len()))
}

async fn export_note(window: &DesktopWindow, notes: String) -> Result<bool, String> {
    let staging = tempfile::tempdir().map_err(|error| error.to_string())?;
    let path = staging.path().join("inspection-notes.txt");
    std::fs::write(&path, notes).map_err(|error| error.to_string())?;
    window.export_file(path).await
}
