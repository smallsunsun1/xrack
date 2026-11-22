use super::{Browser, FileOrigin, FileTab, close, ensure_preview, locate, show_active};
use crate::{read_in_background, review::read_limited};
use session::ExcerptOrigin;
use std::{
    cell::RefCell,
    fs::{self, File, Permissions},
    io::Write,
    path::Path,
    rc::Rc,
};
use workbench_ui::AgentWorkbench;

const EDIT_LIMIT: usize = 1024 * 1024;
const CONFLICT: &str = "文件已在磁盘上变化，草稿仍保留";

#[derive(Default)]
pub(super) enum Editor {
    #[default]
    Closed,
    Loading(u64),
    Ready(Document),
    Failed(String),
}

pub(super) struct Document {
    original: String,
    draft: String,
    saving: Option<u64>,
    status: String,
}

impl Document {
    fn new(text: String) -> Self {
        Self {
            original: text.clone(),
            draft: text,
            saving: None,
            status: String::new(),
        }
    }
}

impl Editor {
    pub(super) fn dirty(&self) -> bool {
        matches!(self, Self::Ready(document) if document.draft != document.original)
    }

    pub(super) fn source_for_excerpt(&self) -> Option<(&str, ExcerptOrigin)> {
        let Self::Ready(document) = self else {
            return None;
        };
        let origin = if self.dirty() {
            ExcerptOrigin::Draft
        } else {
            ExcerptOrigin::Source
        };
        Some((&document.draft, origin))
    }

    pub(super) fn saving(&self) -> bool {
        matches!(self, Self::Ready(document) if document.saving.is_some())
    }
}

#[derive(Clone, Copy)]
pub(super) enum CloseTarget {
    Preview,
    Tab,
}

pub(crate) fn read_text(workspace: &str, path: &Path) -> Result<(String, Permissions), String> {
    let selected = path.to_str().ok_or("文件路径需要使用 UTF-8")?;
    let actual = locate(workspace, selected, FileOrigin::Reference)?;
    if actual.absolute != path {
        return Err("文件路径已变化，请重新打开".into());
    }
    let metadata = fs::metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("编辑需要普通文件".into());
    }
    let file = File::open(path).map_err(|error| error.to_string())?;
    let (bytes, truncated) = read_limited(file, EDIT_LIMIT).map_err(|error| error.to_string())?;
    if truncated {
        return Err("超过 1 MiB 的文件使用预览模式".into());
    }
    if bytes.contains(&0) {
        return Err("此文件是二进制文件".into());
    }
    let text = String::from_utf8(bytes).map_err(|_| "此文件不是 UTF-8 文本")?;
    Ok((text, metadata.permissions()))
}

pub(crate) fn save_text(
    workspace: &str,
    path: &Path,
    original: &str,
    draft: &str,
) -> Result<(), String> {
    if draft.len() > EDIT_LIMIT || draft.contains('\0') {
        return Err("保存需要不超过 1 MiB 的 UTF-8 文本".into());
    }
    let (current, permissions) = read_text(workspace, path)?;
    if current != original {
        return Err(CONFLICT.into());
    }
    if permissions.readonly() {
        return Err("此文件为只读".into());
    }
    let parent = path.parent().ok_or("无法定位文件的所在目录")?;
    let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    staged
        .write_all(draft.as_bytes())
        .map_err(|error| error.to_string())?;
    staged
        .as_file()
        .set_permissions(permissions.clone())
        .map_err(|error| error.to_string())?;
    staged
        .as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;

    // Publish only a complete file and check again after staging the changes.
    let (current, latest_permissions) = read_text(workspace, path)?;
    if current != original || latest_permissions != permissions {
        return Err(CONFLICT.into());
    }
    staged
        .persist(path)
        .map_err(|error| error.error.to_string())?;
    Ok(())
}

pub(super) fn show(ui: &mut AgentWorkbench, tab: Option<&FileTab>) {
    let editor = tab.map(|tab| &tab.editor);
    ui.set_workspace_file_editing(matches!(editor, Some(Editor::Ready(_))));
    ui.set_workspace_file_edit_loading(matches!(editor, Some(Editor::Loading(_))));
    ui.set_workspace_file_edit_dirty(editor.is_some_and(Editor::dirty));
    ui.set_workspace_file_edit_saving(editor.is_some_and(Editor::saving));
    let (text, status) = match editor {
        Some(Editor::Ready(document)) => (document.draft.as_str(), document.status.as_str()),
        Some(Editor::Loading(_)) => ("", "正在读取完整文件…"),
        Some(Editor::Failed(error)) => ("", error.as_str()),
        _ => ("", ""),
    };
    if ui.get_workspace_file_edit_text() != text {
        ui.set_workspace_file_edit_text(text.into());
    }
    ui.set_workspace_file_edit_status(status.into());
}

fn publish_state(ui: &mut AgentWorkbench, browser: &Browser, id: i64) {
    if let Some(index) = browser.files.index(id) {
        let row = browser.files.tabs[index].row();
        let model = ui.model_workspace_preview_tabs();
        if model
            .row(index)
            .is_some_and(|current| current.dirty != row.dirty)
        {
            model.set_row(index, row);
        }
    }
    let editors = || {
        std::iter::once(&browser.files)
            .chain(browser.previous.values())
            .flat_map(|files| files.tabs.iter().map(|tab| &tab.editor))
    };
    ui.set_workspace_edits_dirty(editors().any(Editor::dirty));
    ui.set_workspace_edits_saving(editors().any(Editor::saving));
}

fn tab_at<'a>(browser: &'a mut Browser, workspace: &str, id: i64) -> Option<&'a mut FileTab> {
    if workspace == browser.workspace {
        browser.files.get_mut(id)
    } else {
        browser.previous.get_mut(workspace)?.get_mut(id)
    }
}

fn is_visible_tab(browser: &Browser, ui: &AgentWorkbench, workspace: &str, id: i64) -> bool {
    browser.workspace == workspace
        && ui.get_workspace() == workspace
        && browser.files.active == Some(id)
}

fn begin(ui: &mut AgentWorkbench, browser: Rc<RefCell<Browser>>) {
    if ui.get_switching() || !ui.get_review_open() || !ui.get_files_active() {
        return;
    }
    let (id, token, workspace, path) = {
        let mut browser = browser.borrow_mut();
        let Some(id) = browser.files.active else {
            return;
        };
        browser.remember_offset(ui);
        browser.edit_token = browser.edit_token.wrapping_add(1);
        let token = browser.edit_token;
        let workspace = browser.workspace.clone();
        let Some(tab) = browser.files.get_mut(id) else {
            return;
        };
        if matches!(tab.editor, Editor::Loading(_) | Editor::Ready(_)) {
            return;
        }
        tab.editor = Editor::Loading(token);
        show(ui, Some(tab));
        (id, token, workspace, tab.file.absolute.clone())
    };
    let weak = ui.as_weak();
    let state = browser.clone();
    let save_scope = workspace.clone();
    let delivery_scope = workspace.clone();
    if let Err(error) = xrack_platform_winit::spawn_local(async move {
        let result = read_in_background("workbench-file-edit", move || {
            let (text, _) = read_text(&save_scope, &path)?;
            Ok(text)
        })
        .await;
        let Some(mut ui) = weak.upgrade() else { return };
        finish_open(
            &mut ui,
            &mut state.borrow_mut(),
            &delivery_scope,
            id,
            token,
            result,
        );
    }) {
        finish_open(
            ui,
            &mut browser.borrow_mut(),
            &workspace,
            id,
            token,
            Err(error.to_string()),
        );
    }
}

fn finish_open(
    ui: &mut AgentWorkbench,
    browser: &mut Browser,
    workspace: &str,
    id: i64,
    token: u64,
    result: Result<String, String>,
) {
    let visible = is_visible_tab(browser, ui, workspace, id);
    let Some(tab) = tab_at(browser, workspace, id) else {
        return;
    };
    if !matches!(tab.editor, Editor::Loading(pending) if pending == token) {
        return;
    }
    tab.editor = match result {
        Ok(text) => Editor::Ready(Document::new(text)),
        Err(error) => Editor::Failed(error),
    };
    if visible {
        show(ui, Some(tab));
        if matches!(tab.editor, Editor::Ready(_)) {
            ui.set_workspace_file_edit_focus(true);
        }
    }
}

fn save(ui: &mut AgentWorkbench, browser: Rc<RefCell<Browser>>) {
    if ui.get_switching() || !ui.get_review_open() || !ui.get_files_active() {
        return;
    }
    let (id, token, workspace, path, original, draft) = {
        let mut state = browser.borrow_mut();
        let Some(id) = state.files.active else { return };
        state.edit_token = state.edit_token.wrapping_add(1);
        let token = state.edit_token;
        let workspace = state.workspace.clone();
        let Some(tab) = state.files.get_mut(id) else {
            return;
        };
        let Editor::Ready(document) = &mut tab.editor else {
            return;
        };
        if document.saving.is_some() || document.draft == document.original {
            return;
        }
        document.saving = Some(token);
        document.status = "正在保存…".into();
        let payload = (
            id,
            token,
            workspace,
            tab.file.absolute.clone(),
            document.original.clone(),
            document.draft.clone(),
        );
        show(ui, Some(tab));
        publish_state(ui, &state, id);
        payload
    };
    let weak = ui.as_weak();
    let state = browser.clone();
    let save_scope = workspace.clone();
    let delivery_scope = workspace.clone();
    if let Err(error) = xrack_platform_winit::spawn_local(async move {
        let result = read_in_background("workbench-file-save", move || {
            save_text(&save_scope, &path, &original, &draft)?;
            Ok(draft)
        })
        .await;
        let Some(mut ui) = weak.upgrade() else { return };
        finish_save(
            &mut ui,
            &mut state.borrow_mut(),
            &delivery_scope,
            id,
            token,
            result,
        );
    }) {
        finish_save(
            ui,
            &mut browser.borrow_mut(),
            &workspace,
            id,
            token,
            Err(error.to_string()),
        );
    }
}

fn finish_save(
    ui: &mut AgentWorkbench,
    browser: &mut Browser,
    workspace: &str,
    id: i64,
    token: u64,
    result: Result<String, String>,
) {
    let visible = is_visible_tab(browser, ui, workspace, id);
    let Some(tab) = tab_at(browser, workspace, id) else {
        return;
    };
    let Editor::Ready(document) = &mut tab.editor else {
        return;
    };
    if document.saving != Some(token) {
        return;
    }
    document.saving = None;
    match result {
        Ok(text) => {
            document.original = text;
            document.status = "已保存".into();
            tab.invalidate();
        }
        Err(error) => document.status = format!("无法保存（{error}）"),
    }
    if visible {
        show(ui, Some(tab));
    }
    publish_state(ui, browser, id);
}

pub(super) fn allow_close(
    ui: &mut AgentWorkbench,
    browser: &mut Browser,
    id: i64,
    target: CloseTarget,
) -> bool {
    let Some(tab) = browser.files.get_mut(id) else {
        return false;
    };
    if tab.editor.saving() {
        crate::show_status(ui, "此文件正在保存…");
        return false;
    }
    if tab.editor.dirty() {
        browser.pending_edit_close = Some((id, target));
        ui.set_workspace_edit_confirm_file(tab.file.relative.clone());
        ui.set_workspace_edit_confirm_closing(matches!(target, CloseTarget::Tab));
        ui.set_workspace_edit_confirm_open(true);
        return false;
    }
    true
}

fn leave_editor(ui: &mut AgentWorkbench, browser: Rc<RefCell<Browser>>, id: i64) {
    {
        let mut browser = browser.borrow_mut();
        let Some(tab) = browser.files.get_mut(id) else {
            return;
        };
        tab.editor = Editor::Closed;
        tab.invalidate();
        show_active(ui, &browser.files);
        publish_state(ui, &browser, id);
    }
    ensure_preview(ui, browser);
}

pub(super) fn attach(ui: &mut AgentWorkbench, browser: &Rc<RefCell<Browser>>) {
    let weak = ui.as_weak();
    let state = browser.clone();
    ui.on_start_workspace_file_edit(move || {
        if let Some(mut ui) = weak.upgrade() {
            begin(&mut ui, state.clone());
        }
    });
    let weak = ui.as_weak();
    let state = browser.clone();
    ui.on_change_workspace_file_edit(move |text| {
        let Some(mut ui) = weak.upgrade() else { return };
        let mut state = state.borrow_mut();
        let Some(id) = state.files.active else { return };
        let Some(tab) = state.files.get_mut(id) else {
            return;
        };
        let Editor::Ready(document) = &mut tab.editor else {
            return;
        };
        if document.saving.is_some() {
            return;
        }
        document.draft = text;
        document.status.clear();
        show(&mut ui, Some(tab));
        publish_state(&mut ui, &state, id);
    });
    let weak = ui.as_weak();
    let state = browser.clone();
    ui.on_save_workspace_file_edit(move || {
        if let Some(mut ui) = weak.upgrade() {
            save(&mut ui, state.clone());
        }
    });
    let weak = ui.as_weak();
    let state = browser.clone();
    ui.on_leave_workspace_file_edit(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        let id = {
            let mut browser = state.borrow_mut();
            let Some(id) = browser.files.active else {
                return;
            };
            if !allow_close(&mut ui, &mut browser, id, CloseTarget::Preview) {
                return;
            }
            id
        };
        leave_editor(&mut ui, state.clone(), id);
    });
    let weak = ui.as_weak();
    let state = browser.clone();
    ui.on_resolve_workspace_edit_close(move |discard| {
        let Some(mut ui) = weak.upgrade() else { return };
        ui.set_workspace_edit_confirm_open(false);
        let pending = state.borrow_mut().pending_edit_close.take();
        let Some((id, target)) = pending.filter(|_| discard) else {
            return;
        };
        match target {
            CloseTarget::Preview => leave_editor(&mut ui, state.clone(), id),
            CloseTarget::Tab => {
                {
                    let mut browser = state.borrow_mut();
                    let Some(tab) = browser.files.get_mut(id) else {
                        return;
                    };
                    if tab.editor.saving() {
                        return;
                    }
                    tab.editor = Editor::Closed;
                    publish_state(&mut ui, &browser, id);
                }
                close(&mut ui, state.clone(), id);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace_files::insert_tab;

    #[test]
    fn saving_replaces_complete_utf8_text_and_preserves_permissions()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let workspace = root.to_str().unwrap();
        let path = root.join("main.rs");
        let original = "let 名字 = 1;\r\n";
        let draft = "let 名字 = 2;\r\n\r\n";
        fs::write(&path, original)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, Permissions::from_mode(0o751))?;
        }
        let (loaded, mode) = read_text(workspace, &path)?;
        assert_eq!(loaded, original);
        save_text(workspace, &path, original, draft)?;
        assert_eq!(fs::read(&path)?, draft.as_bytes());
        assert_eq!(fs::metadata(&path)?.permissions(), mode);
        assert_eq!(fs::read_dir(&root)?.count(), 1);
        Ok(())
    }

    #[test]
    fn save_preserves_external_changes_and_rejects_a_retargeted_file()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let workspace = root.join("project");
        fs::create_dir(&workspace)?;
        let path = workspace.join("notes.md");
        let scope = workspace.to_str().unwrap();
        fs::write(&path, "first")?;
        let (original, _) = read_text(scope, &path)?;
        fs::write(&path, "updated by another editor")?;
        assert_eq!(
            save_text(scope, &path, &original, "my draft"),
            Err(CONFLICT.into())
        );
        assert_eq!(fs::read_to_string(&path)?, "updated by another editor");
        assert_eq!(fs::read_dir(&workspace)?.count(), 1);

        #[cfg(unix)]
        {
            let other = root.join("outside.md");
            fs::write(&other, "outside")?;
            fs::remove_file(&path)?;
            std::os::unix::fs::symlink(&other, &path)?;
            assert!(save_text(scope, &path, "outside", "overwrite").is_err());
            assert_eq!(fs::read_to_string(other)?, "outside");
        }
        Ok(())
    }

    #[test]
    fn editor_keeps_a_draft_after_background_save_failure_and_requires_confirmation_to_close()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let scope = root.to_str().unwrap();
        let other = root.join("other");
        fs::create_dir(&other)?;
        fs::write(root.join("project.rs"), "first")?;
        let mut ui = AgentWorkbench::new();
        ui.set_workspace(scope.into());
        let state = Rc::new(RefCell::new(Browser::default()));
        state.borrow_mut().reset(&mut ui);
        attach(&mut ui, &state);
        let id = {
            let mut browser = state.borrow_mut();
            let file = locate(scope, "project.rs", FileOrigin::Workspace)?;
            let id = insert_tab(&mut ui, &mut browser, file, (None, None), 1, true).unwrap();
            browser.files.active = Some(id);
            browser.files.get_mut(id).unwrap().editor = Editor::Loading(4);
            finish_open(&mut ui, &mut browser, scope, id, 4, Ok("first".into()));
            id
        };
        ui.invoke_change_workspace_file_edit("unsaved draft".into());
        assert!(ui.get_workspace_edits_dirty());
        assert!(ui.model_workspace_preview_tabs().row(0).unwrap().dirty);
        close(&mut ui, state.clone(), id);
        assert!(ui.get_workspace_edit_confirm_open());
        ui.invoke_resolve_workspace_edit_close(false);
        assert_eq!(ui.model_workspace_preview_tabs().len(), 1);

        {
            let mut browser = state.borrow_mut();
            let Editor::Ready(document) = &mut browser.files.get_mut(id).unwrap().editor else {
                panic!("expected an open editor")
            };
            document.saving = Some(8);
            publish_state(&mut ui, &browser, id);
        }
        ui.set_workspace(other.to_str().unwrap().into());
        state.borrow_mut().reset(&mut ui);
        assert!(!ui.get_workspace_file_editing());
        assert!(ui.get_workspace_edits_dirty() && ui.get_workspace_edits_saving());
        finish_save(
            &mut ui,
            &mut state.borrow_mut(),
            scope,
            id,
            7,
            Ok("wrong reply".into()),
        );
        assert!(ui.get_workspace_edits_saving());
        finish_save(
            &mut ui,
            &mut state.borrow_mut(),
            scope,
            id,
            8,
            Err(CONFLICT.into()),
        );
        assert!(!ui.get_workspace_edits_saving());
        assert!(!ui.get_workspace_file_editing());

        ui.set_workspace(scope.into());
        state.borrow_mut().reset(&mut ui);
        assert_eq!(ui.get_workspace_file_edit_text(), "unsaved draft");
        assert!(ui.get_workspace_file_edit_status().contains(CONFLICT));
        close(&mut ui, state.clone(), id);
        ui.invoke_resolve_workspace_edit_close(true);
        assert!(ui.model_workspace_preview_tabs().is_empty());
        assert!(!ui.get_workspace_edits_dirty());
        assert_eq!(fs::read_to_string(root.join("project.rs"))?, "first");
        Ok(())
    }
}
