use std::{cell::RefCell, fs, path::Path, rc::Rc};
use xrack::{
    Component, FileDialogMode, NativeDialogOptions, NativeDialogQueue, NativeDialogResult,
    StandardButtonKind,
};
use xrack_desktop_notes::{FieldNotes, connect};
use xrack_testing::{Size, UiDriver};

fn editor() -> UiDriver<FieldNotes> {
    let mut ui = FieldNotes::new();
    connect(&mut ui, |job, complete| {
        complete(job.run());
        Ok(())
    });
    UiDriver::headless(ui, Size::new(720.0, 460.0))
}

fn respond(
    ui: &mut UiDriver<FieldNotes>,
    mode: Option<FileDialogMode>,
    result: NativeDialogResult,
) {
    let mut queue = NativeDialogQueue::default();
    let scene = ui.component().render_shared();
    queue.sync(&scene.native_dialogs.iter().collect::<Vec<_>>());
    let request = queue
        .begin_next()
        .expect("expected a native dialog request");
    match (mode, request.options.as_ref()) {
        (Some(mode), NativeDialogOptions::File(options)) => assert_eq!(options.mode, mode),
        (None, NativeDialogOptions::Message(_)) => {}
        _ => panic!("unexpected native dialog"),
    }
    ui.update(|_| assert!(queue.complete(request.id, result)));
}

fn choose(ui: &mut UiDriver<FieldNotes>, mode: FileDialogMode, path: &Path) {
    respond(
        ui,
        Some(mode),
        NativeDialogResult::Files(vec![path.to_str().expect("UTF-8 test path").into()]),
    );
}

#[test]
fn edits_save_and_reopen_as_utf8() -> std::io::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("现场笔记.md");
    let mut ui = editor();
    ui.click("editor");
    ui.fill_focused("# 现场记录\n\n检查了设备 A-17。");
    assert!(ui.component().get_dirty());

    ui.click("save");
    choose(&mut ui, FileDialogMode::SaveFile, &path);
    assert_eq!(ui.component().get_status(), "已保存");
    assert!(!ui.component().get_dirty());

    ui.click("editor");
    let revision = "# 现场记录\n\n设备 A-17 已修复。";
    ui.fill_focused(revision);
    ui.click("save");
    assert_eq!(fs::read_to_string(&path)?, revision);
    assert!(!ui.component().get_dirty());
    assert!(ui.scene().native_dialogs.is_empty());

    let mut reopened = editor();
    reopened.click("open");
    choose(&mut reopened, FileDialogMode::OpenFile, &path);
    assert_eq!(reopened.node("editor").text("text"), Some(revision));
    assert_eq!(reopened.component().get_document_name(), "现场笔记.md");
    assert!(!reopened.component().get_dirty());
    Ok(())
}

#[test]
fn cancel_open_failure_and_cancel_close_keep_the_draft() -> std::io::Result<()> {
    let directory = tempfile::tempdir()?;
    let mut ui = editor();
    ui.click("editor");
    ui.fill_focused("未保存的草稿");

    ui.click("open");
    respond(
        &mut ui,
        None,
        NativeDialogResult::Button(StandardButtonKind::Ok),
    );
    respond(
        &mut ui,
        Some(FileDialogMode::OpenFile),
        NativeDialogResult::Canceled,
    );
    assert_eq!(ui.component().get_status(), "已取消打开");
    assert!(ui.component().get_dirty());

    ui.click("open");
    respond(
        &mut ui,
        None,
        NativeDialogResult::Button(StandardButtonKind::Ok),
    );
    choose(
        &mut ui,
        FileDialogMode::OpenFile,
        &directory.path().join("missing.txt"),
    );
    assert!(ui.component().get_status().starts_with("文件操作失败"));
    assert_eq!(ui.component().get_content(), "未保存的草稿");

    ui.update(|component| assert!(!component.close_requested()));
    respond(
        &mut ui,
        None,
        NativeDialogResult::Button(StandardButtonKind::Cancel),
    );
    assert!(ui.component().get_dirty());
    assert_eq!(ui.node("editor").text("text"), Some("未保存的草稿"));
    ui.update(|component| {
        assert!(!component.take_window_close_request());
        assert!(!component.close_requested());
    });
    respond(
        &mut ui,
        None,
        NativeDialogResult::Button(StandardButtonKind::Ok),
    );
    ui.update(|component| {
        assert!(component.take_window_close_request());
        assert!(component.close_requested());
    });
    Ok(())
}

#[test]
fn save_failure_preserves_the_existing_document_and_draft() -> std::io::Result<()> {
    let directory = tempfile::tempdir()?;
    let original = directory.path().join("existing.txt");
    fs::write(&original, "已保存")?;
    let mut ui = editor();
    ui.click("open");
    choose(&mut ui, FileDialogMode::OpenFile, &original);
    ui.click("editor");
    ui.fill_focused("草稿");
    ui.click("save-as");
    choose(
        &mut ui,
        FileDialogMode::SaveFile,
        &directory.path().join("missing").join("new.txt"),
    );
    assert_eq!(fs::read_to_string(&original)?, "已保存");
    assert_eq!(ui.component().get_path(), original.to_string_lossy());
    assert_eq!(ui.component().get_content(), "草稿");
    assert!(ui.component().get_dirty());
    assert!(ui.component().get_status().starts_with("文件操作失败"));
    Ok(())
}

#[test]
fn pending_save_marks_only_the_snapshot_as_saved() -> std::io::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("existing.txt");
    let pending = Rc::new(RefCell::new(None));
    let work = pending.clone();
    let mut component = FieldNotes::new();
    connect(&mut component, move |job, complete| {
        assert!(work.replace(Some((job, complete))).is_none());
        Ok(())
    });
    let mut ui = UiDriver::headless(component, Size::new(720.0, 460.0));
    ui.click("editor");
    ui.fill_focused("正在保存的版本");
    ui.click("save");
    choose(&mut ui, FileDialogMode::SaveFile, &path);
    assert!(ui.component().get_working());
    assert!(ui.node("editor").boolean("read-only", false));
    ui.update(|component| {
        assert!(!component.close_requested());
        component.set_content("后来到达的程序更新".into());
    });

    let (job, complete) = pending.take().expect("queued save");
    ui.update(|_| complete(job.run()));
    assert!(!ui.component().get_working());
    assert!(ui.component().get_dirty());
    assert_eq!(ui.component().get_content(), "后来到达的程序更新");
    assert_eq!(fs::read_to_string(path)?, "正在保存的版本");
    Ok(())
}
