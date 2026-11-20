use pocket_tasks_notes::{append_note, read_notes};
use std::{
    cell::RefCell,
    fs::{self, read_dir},
    io::{self, Write},
    path::{Path, PathBuf},
    rc::Rc,
};
use touch_ui::{NativePanelQueueDemo, TouchDemo};
use xrack_platform_winit::{
    Desktop, DesktopWindow, IosDirectory, IosPhotoSelection, IosPhotoThumbnail, LocalTask,
    run_desktop, spawn_local,
};

#[unsafe(no_mangle)]
pub extern "C" fn xrack_ios_main() -> i32 {
    match run_desktop(|desktop| {
        if std::env::args().any(|argument| argument == "--reorder-list") {
            desktop.open(touch_ui::ListReorderDemo::new())?;
            return Ok(());
        }
        if std::env::args().any(|argument| argument == "--choose-photos") {
            return open_photo_demo(desktop);
        }
        if std::env::args().any(|argument| argument == "--native-panels") {
            return open_panel_queue_demo(desktop);
        }
        if std::env::args().any(|argument| argument == "--folder-dialog") {
            return open_folder_dialog_demo(desktop);
        }
        if std::env::args().any(|argument| argument == "--native-colors") {
            return open_color_demo(desktop);
        }
        if std::env::args().any(|argument| argument == "--ipad-pointer") {
            desktop.open(touch_ui::PointerInspectionDemo::new())?;
            return Ok(());
        }
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
        register_folder_import(&mut app, window.clone());
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

fn open_folder_dialog_demo(desktop: &Desktop) -> Result<(), String> {
    let home = std::env::var_os("HOME").ok_or("The app container is unavailable.")?;
    let directory = PathBuf::from(home).join("Documents/Inspection folders");
    for (name, note) in [
        ("Pump notes", "Pump reading"),
        ("Valve notes", "Valve reading"),
    ] {
        let notes = directory.join(name).join("reports");
        fs::create_dir_all(&notes).map_err(|error| error.to_string())?;
        match fs::File::create_new(notes.join("summary.txt")) {
            Ok(mut file) => file
                .write_all(note.as_bytes())
                .map_err(|error| error.to_string())?,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    let directory = directory
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let mut app = touch_ui::FolderDialogDemo::new();
    app.set_starting_directory(directory.to_string_lossy().into_owned());
    let weak = app.as_weak();
    app.on_inspect_folders(move |paths| {
        if let Some(mut app) = weak.upgrade() {
            let status = inspect_folder_copies(&paths, &directory)
                .unwrap_or_else(|error| format!("Cannot inspect imported folders: {error}"));
            app.set_status(status);
        }
    });
    desktop.open(app)?;
    Ok(())
}

fn inspect_folder_copies(paths: &[String], original: &Path) -> io::Result<String> {
    let mut names = Vec::new();
    for path in paths {
        let path = Path::new(path).canonicalize()?;
        if path.starts_with(original) || !path.metadata()?.is_dir() {
            return Err(io::Error::other("Files did not return a local folder copy"));
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Folder");
        let summary = path.join("reports/summary.txt");
        let label = if matches!(name, "Pump notes" | "Valve notes") {
            fs::read_to_string(summary)?
        } else {
            name.to_owned()
        };
        names.push(label);
    }
    names.sort_unstable();
    Ok(format!("Imported {} · {}", names.len(), names.join(", ")))
}

fn open_color_demo(desktop: &Desktop) -> Result<(), String> {
    let app = touch_ui::NativeColorDemo::new();
    let weak = app.as_weak();
    let window = desktop.open(app)?;
    let mut app = weak
        .upgrade()
        .ok_or("The marker color window was released.")?;
    app.on_edit_color(move || {
        let Some(mut app) = weak.upgrade() else {
            return;
        };
        let initial = app.get_marker_color();
        let show_alpha = app.get_show_alpha();
        let weak = weak.clone();
        let window = window.clone();
        app.set_preview_color(initial);
        app.set_preview_readout(app.get_color_readout());
        app.set_picker_error(String::new());
        app.set_pending(true);
        if spawn_local(async move {
            let preview = weak.clone();
            let result = window
                .select_color_with_preview(initial, show_alpha, move |color| {
                    if let Some(mut app) = preview.upgrade() {
                        app.set_preview_color(color);
                        app.set_preview_readout(format!("#{:08X}", color.0));
                    }
                })
                .await;
            if let Some(mut app) = weak.upgrade() {
                match result {
                    Ok(color) => {
                        app.set_marker_color(color);
                        app.set_color_readout(format!("#{:08X}", color.0));
                    }
                    Err(error) => app.set_picker_error(error),
                }
                app.set_pending(false);
            }
        })
        .is_err()
        {
            app.set_pending(false);
            app.set_picker_error("The UI event loop is unavailable.".into());
        }
    });
    Ok(())
}

#[derive(Clone, Copy)]
enum DemoPanel {
    Files,
    Photos,
    Share,
}

fn open_panel_queue_demo(desktop: &Desktop) -> Result<(), String> {
    let app = NativePanelQueueDemo::new();
    let weak = app.as_weak();
    let window = desktop.open(app)?;
    let mut app = weak
        .upgrade()
        .ok_or("The panel queue window was released.")?;
    let files = Rc::new(RefCell::new(None::<LocalTask<()>>));
    let share = Rc::new(RefCell::new(None::<LocalTask<()>>));
    app.on_queue_panels({
        let weak = weak.clone();
        let files = files.clone();
        let share = share.clone();
        move |timed| {
            let panels = if timed {
                &[DemoPanel::Files, DemoPanel::Share][..]
            } else {
                &[DemoPanel::Files, DemoPanel::Photos, DemoPanel::Share][..]
            };
            for &panel in panels {
                let weak = weak.clone();
                let finish = move |result: Result<bool, String>| {
                    let Some(mut app) = weak.upgrade() else {
                        return;
                    };
                    let label = match panel {
                        DemoPanel::Files => "Files",
                        DemoPanel::Photos => "Photos",
                        DemoPanel::Share => "Share",
                    };
                    let status = match result {
                        Ok(true) => format!("{label} completed"),
                        Ok(false) => format!("{label} canceled"),
                        Err(error) => format!("{label} failed: {error}"),
                    };
                    finish_demo_panel(&mut app, panel, status);
                };
                let window = window.clone();
                let task = spawn_local({
                    let finish = finish.clone();
                    async move {
                        finish(match panel {
                            DemoPanel::Files if timed => {
                                export_note(&window, "Pocket Tasks export timeout".into()).await
                            }
                            DemoPanel::Files => window
                                .select_directory()
                                .await
                                .map(|folder| folder.is_some()),
                            DemoPanel::Photos => {
                                window.select_photos(1).await.map(|photos| photos.is_some())
                            }
                            DemoPanel::Share => window.share_text("Pocket Tasks panel queue").await,
                        });
                    }
                });
                match task {
                    Ok(task) if timed => match panel {
                        DemoPanel::Files => {
                            files.replace(Some(task));
                        }
                        DemoPanel::Share => {
                            share.replace(Some(task));
                        }
                        DemoPanel::Photos => {}
                    },
                    Ok(_) => {}
                    Err(_) => finish(Err("The UI event loop is unavailable.".into())),
                }
            }
        }
    });
    app.on_abort_panel(move |is_files| {
        let Some(mut app) = weak.upgrade() else {
            return;
        };
        let (slot, panel, queued, status) = if is_files {
            (
                &files,
                DemoPanel::Files,
                app.get_files_status() == "Files queued",
                "Files timed out",
            )
        } else {
            (
                &share,
                DemoPanel::Share,
                app.get_share_status() == "Share queued",
                "Share timed out",
            )
        };
        if let Some(task) = slot.take()
            && queued
        {
            task.abort();
            finish_demo_panel(&mut app, panel, status.into());
        }
    });
    Ok(())
}

fn finish_demo_panel(app: &mut NativePanelQueueDemo, panel: DemoPanel, status: String) {
    match panel {
        DemoPanel::Files => app.set_files_status(status),
        DemoPanel::Photos => app.set_photos_status(status),
        DemoPanel::Share => app.set_share_status(status),
    }
    app.set_panels_left(app.get_panels_left() - 1);
}

#[derive(Clone, Copy)]
enum PhotoTransfer {
    SaveOriginal(i64),
    SaveAll,
    ShareOriginals,
}

fn open_photo_demo(desktop: &Desktop) -> Result<(), String> {
    let app = touch_ui::PhotoPickerDemo::new();
    let weak = app.as_weak();
    let window = desktop.open(app)?;
    let mut app = weak.upgrade().ok_or("The photo window was released.")?;
    let selection = Rc::new(RefCell::new(None));
    app.on_choose_photos({
        let weak = weak.clone();
        let window = window.clone();
        let selection = selection.clone();
        move || {
            let Some(mut app) = weak.upgrade() else {
                return;
            };
            if app.get_pending() {
                return;
            }
            app.set_pending(true);
            let weak = weak.clone();
            let window = window.clone();
            let selection = selection.clone();
            if spawn_local(async move {
                let result = import_photos(&window).await;
                if let Some(mut app) = weak.upgrade() {
                    match result {
                        Ok(Some((files, previews))) => {
                            let count = previews.len();
                            *selection.borrow_mut() = Some(Rc::new(files));
                            app.set_photos(
                                previews.into_iter().map(PhotoPreview::into_row).collect(),
                            );
                            app.set_current_photo(0);
                            let status = if count == 1 {
                                "Imported 1 photo".into()
                            } else {
                                format!("Imported {count} photos")
                            };
                            app.set_photo_status(status);
                        }
                        Ok(None) => app.set_photo_status("Photo selection canceled".into()),
                        Err(error) => app.set_photo_status(error),
                    }
                    app.set_pending(false);
                }
            })
            .is_err()
            {
                app.set_pending(false);
                app.set_photo_status("The UI event loop is unavailable.".into());
            }
        }
    });
    let transfer = Rc::new(move |operation: PhotoTransfer| {
        let Some(mut app) = weak.upgrade() else {
            return;
        };
        if app.get_pending() {
            return;
        }
        let Some(photos) = selection.borrow().clone() else {
            return;
        };
        let paths = if let PhotoTransfer::SaveOriginal(index) = operation {
            let Some(path) = usize::try_from(index)
                .ok()
                .and_then(|index| photos.paths().get(index).cloned())
            else {
                return;
            };
            vec![path]
        } else {
            photos.paths().to_vec()
        };
        let count = paths.len();
        app.set_pending(true);
        let weak = weak.clone();
        let window = window.clone();
        if spawn_local(async move {
            let status = match operation {
                PhotoTransfer::ShareOriginals => window.share_files(paths).await.map(|shared| {
                    if shared {
                        "Sharing completed".into()
                    } else {
                        "Share canceled".into()
                    }
                }),
                _ => window.export_files(paths).await.map(|saved| {
                    if !saved {
                        "Save canceled".into()
                    } else if count == 1 {
                        "Original saved".into()
                    } else {
                        format!("{count} originals saved")
                    }
                }),
            };
            // The system activity may read the files before its sheet closes.
            drop(photos);
            if let Some(mut app) = weak.upgrade() {
                app.set_photo_status(status.unwrap_or_else(|error| error));
                app.set_pending(false);
            }
        })
        .is_err()
        {
            app.set_pending(false);
            app.set_photo_status("The UI event loop is unavailable.".into());
        }
    });
    app.on_save_original({
        let transfer = transfer.clone();
        move |index| transfer(PhotoTransfer::SaveOriginal(index))
    });
    app.on_save_all({
        let transfer = transfer.clone();
        move || transfer(PhotoTransfer::SaveAll)
    });
    app.on_share_originals(move || transfer(PhotoTransfer::ShareOriginals));
    Ok(())
}

struct PhotoPreview {
    thumbnail: Option<IosPhotoThumbnail>,
    details: String,
}

impl PhotoPreview {
    fn into_row(self) -> touch_ui::InspectionPhoto {
        touch_ui::InspectionPhoto {
            has_preview: self.thumbnail.is_some(),
            preview: self
                .thumbnail
                .map(IosPhotoThumbnail::into_image)
                .unwrap_or_default(),
            details: self.details,
        }
    }
}

async fn import_photos(
    window: &DesktopWindow,
) -> Result<Option<(IosPhotoSelection, Vec<PhotoPreview>)>, String> {
    let Some(photos) = window.select_photos(4).await? else {
        return Ok(None);
    };
    let (send, receive) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("pocket-tasks-photo-preview".into())
        .spawn(move || {
            let previews = photos
                .paths()
                .iter()
                .enumerate()
                .map(|(index, path)| {
                    let format = path
                        .extension()
                        .and_then(|extension| extension.to_str())
                        .map_or_else(|| String::from("Image"), str::to_ascii_uppercase);
                    match photos.thumbnail(index, 800) {
                        Ok(thumbnail) => {
                            let [width, height] = thumbnail.size();
                            PhotoPreview {
                                thumbnail: Some(thumbnail),
                                details: format!("{format} · {width} × {height} px"),
                            }
                        }
                        Err(_) => PhotoPreview {
                            thumbnail: None,
                            details: format!("{format} · Preview unavailable"),
                        },
                    }
                })
                .collect();
            let _ = send.send((photos, previews));
        })
        .map_err(|error| format!("Cannot start photo preview decoding: {error}"))?;
    receive
        .await
        .map(Some)
        .map_err(|_| "Photo preview decoding stopped.".to_owned())
}

fn register_folder_import(app: &mut TouchDemo, window: DesktopWindow) {
    let bookmark = std::env::var_os("HOME").map(|home| {
        PathBuf::from(home).join("Library/Application Support/Pocket Tasks/notes-folder.bookmark")
    });
    app.set_saved_folder_available(bookmark.as_deref().is_some_and(Path::is_file));
    let weak = app.as_weak();
    let import = Rc::new(move |source: FolderSource| {
        let Some(mut app) = weak.upgrade() else {
            return;
        };
        let original = app.get_notes();
        let weak = weak.clone();
        let window = window.clone();
        let bookmark = bookmark.clone();
        app.set_transfer_status(String::new());
        app.set_transfer_pending(true);
        if spawn_local(async move {
            let result = import_folder(&window, source, bookmark.clone()).await;
            if let Some(mut app) = weak.upgrade() {
                let status = match result {
                    Ok(Some(_)) if app.get_notes_locked() || app.get_notes() != original => {
                        "Current notes kept".into()
                    }
                    Ok(Some(import)) => {
                        app.set_notes(import.notes);
                        if let Some(error) = import.bookmark_error {
                            format!(
                                "{} imported. Could not remember folder: {error}",
                                import.count
                            )
                        } else {
                            format!("{} imported from folder", import.count)
                        }
                    }
                    Ok(None) => "Folder import canceled".into(),
                    Err(error) => error,
                };
                app.set_saved_folder_available(bookmark.as_deref().is_some_and(Path::is_file));
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
    app.on_import_folder({
        let import = import.clone();
        move || import(FolderSource::Picker)
    });
    app.on_reload_folder(move || import(FolderSource::Bookmark));
}

enum FolderSource {
    Picker,
    Bookmark,
}

struct FolderImport {
    notes: String,
    count: usize,
    bookmark_error: Option<io::Error>,
}

async fn import_folder(
    window: &DesktopWindow,
    source: FolderSource,
    bookmark: Option<PathBuf>,
) -> Result<Option<FolderImport>, String> {
    let selection = match source {
        FolderSource::Picker => {
            let Some(directory) = window.select_directory().await? else {
                return Ok(None);
            };
            Some(directory)
        }
        FolderSource::Bookmark => None,
    };
    let (send, receive) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("pocket-tasks-import".into())
        .spawn(move || {
            let _ = send.send(read_selected_folder(selection, bookmark.as_deref()));
        })
        .map_err(|error| error.to_string())?;
    receive
        .await
        .map_err(|_| "The folder import stopped.")?
        .map(Some)
}

fn read_selected_folder(
    selection: Option<IosDirectory>,
    bookmark: Option<&Path>,
) -> Result<FolderImport, String> {
    let remember = selection.is_some();
    let storage_unavailable = || io::Error::other("Application storage is unavailable");
    let directory = match selection {
        Some(directory) => directory,
        None => {
            let restore = || {
                let path = bookmark.ok_or_else(storage_unavailable)?;
                IosDirectory::from_bookmark(&fs::read(path)?)
            };
            restore().map_err(|error| format!("Choose the folder again. {error}"))?
        }
    };
    let (notes, count) = read_folder(&directory).map_err(|error| error.to_string())?;
    let bookmark_error = if remember {
        bookmark
            .ok_or_else(storage_unavailable)
            .and_then(|path| remember_folder(&directory, path))
            .err()
    } else {
        None
    };
    Ok(FolderImport {
        notes,
        count,
        bookmark_error,
    })
}

fn remember_folder(directory: &IosDirectory, path: &Path) -> io::Result<()> {
    let data = directory.bookmark_data()?;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("The bookmark has no storage directory"))?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(&data)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(())
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
