use pocket_tasks_notes::{append_note, read_notes};
use std::{fs::read_dir, io, path::PathBuf};
use touch_ui::TouchDemo;
use xrack_platform_winit::{DesktopWindow, IosDirectory, run_desktop, spawn_local};

#[unsafe(no_mangle)]
pub extern "C" fn xrack_ios_main() -> i32 {
    match run_desktop(|desktop| {
        let mut app = TouchDemo::new();
        app.set_folder_import_available(true);
        let weak = app.as_weak();
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
        app.on_read_notes({
            let weak = weak.clone();
            move |paths| {
                let Some(mut app) = weak.upgrade() else {
                    return false;
                };
                let Some(notes) = read_notes(&paths) else {
                    return false;
                };
                app.set_notes(notes);
                true
            }
        });
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
                app.set_transfer_status(String::new());
                app.set_transfer_pending(true);
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
            app.set_transfer_status(String::new());
            app.set_transfer_pending(true);
            if spawn_local(async move {
                let result = if export_file {
                    export_note(&window, notes).await
                } else {
                    window.share_text(notes).await
                };
                if let Some(mut app) = weak.upgrade() {
                    let status = match result {
                        Ok(true) if export_file => "Saved to Files".into(),
                        Ok(true) => "Note shared".into(),
                        Ok(false) if export_file => "Save canceled".into(),
                        Ok(false) => "Share canceled".into(),
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
        });
        Ok(())
    }) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
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
            let _ = send.send(read_folder(&directory));
        })
        .map_err(|error| error.to_string())?;
    receive
        .await
        .map_err(|_| "The folder import stopped.")?
        .map(Some)
        .map_err(|error| error.to_string())
}

fn read_folder(directory: &IosDirectory) -> io::Result<(String, usize)> {
    let paths = directory.with_read_access("", |root| {
        let mut paths = Vec::new();
        for entry in read_dir(root)? {
            let entry = entry?;
            let name = PathBuf::from(entry.file_name());
            let is_note = name
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("txt") || extension.eq_ignore_ascii_case("md")
                });
            if is_note && entry.file_type()?.is_file() {
                paths.push(name);
            }
        }
        paths.sort();
        Ok(paths)
    })?;
    if paths.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "No text notes in this folder",
        ));
    }
    let mut notes = String::new();
    for (index, name) in paths.iter().enumerate() {
        directory.with_read_access(name, |path| append_note(path, index, &mut notes))?;
    }
    Ok((notes, paths.len()))
}

async fn export_note(window: &DesktopWindow, notes: String) -> Result<bool, String> {
    let staged = tempfile::tempdir().map_err(|error| error.to_string())?;
    let path = staged.path().join("inspection-notes.txt");
    std::fs::write(&path, notes).map_err(|error| error.to_string())?;
    window.export_file(path).await
}
