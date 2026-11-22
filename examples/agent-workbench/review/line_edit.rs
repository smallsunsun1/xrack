use crate::{
    read_in_background,
    workspace_files::{allowed_relative, read_text, save_text},
};
use std::{
    cell::RefCell,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
};
use workbench_ui::AgentWorkbench;
use xrack_platform_winit::spawn_local;

#[derive(Clone)]
struct Target {
    conversation: i64,
    workspace: String,
    selection: String,
    path: PathBuf,
    line: usize,
    expected: String,
}

impl Target {
    fn current(&self, ui: &AgentWorkbench) -> bool {
        ui.get_conversation_id() == self.conversation
            && ui.get_workspace() == self.workspace
            && ui.get_review_selection() == self.selection
    }
}

#[derive(Clone)]
struct LineDraft {
    target: Target,
    original: String,
    range: Range<usize>,
    ending: &'static str,
}

impl LineDraft {
    fn open(target: Target) -> Result<Self, String> {
        let (original, permissions) = read_text(&target.workspace, &target.path)?;
        if permissions.readonly() {
            return Err("此文件为只读".into());
        }
        Self::at_line(target, original)
    }

    fn at_line(target: Target, original: String) -> Result<Self, String> {
        let mut lines = original.split_inclusive('\n');
        let row = target.line.checked_sub(1).ok_or("无效行号")?;
        let start = lines.by_ref().take(row).map(str::len).sum::<usize>();
        let raw = lines.next().ok_or("该行已变化，请刷新差异")?;
        let text = raw
            .strip_suffix('\n')
            .map_or(raw, |line| line.strip_suffix('\r').unwrap_or(line));
        if text != target.expected {
            return Err("该行已变化，请刷新差异".into());
        }
        let ending = if raw.ends_with("\r\n") || !raw.ends_with('\n') && original.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let range = start..start + text.len();
        Ok(Self {
            target,
            original,
            range,
            ending,
        })
    }

    fn save(self, replacement: &str) -> Result<(), String> {
        let mut next = self.original.clone();
        next.replace_range(
            self.range,
            &replacement.replace("\r\n", "\n").replace('\n', self.ending),
        );
        save_text(
            &self.target.workspace,
            &self.target.path,
            &self.original,
            &next,
        )
    }
}

fn target(ui: &AgentWorkbench, id: i64) -> Option<Target> {
    if ui.get_switching()
        || !ui.get_review_open()
        || ui.get_files_active()
        || ui.get_review_turn() >= 0
        || !ui.get_review_comparison_mode().is_empty()
        || ui.get_review_filter() == "staged"
        || ui.get_review_index_busy()
    {
        return None;
    }
    let index = usize::try_from(id).ok()?;
    let row = ui.model_review_lines().row(index)?;
    if row.id != id || row.new_line <= 0 || !matches!(row.section.as_str(), "未暂存" | "未跟踪文件")
    {
        return None;
    }
    let body = ui.get_review_body();
    let expected = body
        .text()
        .lines()
        .nth(index)?
        .strip_prefix(['+', ' '])?
        .to_owned();
    let workspace = ui.get_workspace();
    let selection = ui.get_review_selection();
    if workspace.is_empty() || selection.is_empty() || !allowed_relative(Path::new(&selection)) {
        return None;
    }
    let base = ui.get_review_base();
    let path = Path::new(if base.is_empty() { &workspace } else { &base }).join(&selection);
    Some(Target {
        conversation: ui.get_conversation_id(),
        workspace,
        selection,
        path,
        line: row.new_line as usize,
        expected,
    })
}

#[derive(Default)]
struct EditState {
    serial: u64,
    draft: Option<LineDraft>,
}

fn finish_save(
    ui: &mut AgentWorkbench,
    state: &mut EditState,
    serial: u64,
    target: &Target,
    result: Result<(), String>,
) {
    if state.serial != serial || !ui.get_review_line_edit_open() {
        return;
    }
    ui.set_review_line_edit_busy(false);
    match result {
        Ok(()) => {
            state.draft = None;
            ui.set_review_line_edit_open(false);
            if target.current(ui) {
                ui.invoke_refresh_review();
            }
        }
        Err(error) => ui.set_review_line_edit_error(error),
    }
}

pub(super) fn attach(ui: &mut AgentWorkbench) {
    let state = Rc::new(RefCell::new(EditState::default()));
    let weak = ui.as_weak();
    let draft_state = state.clone();
    ui.on_edit_review_line(move |id| {
        let Some(mut ui) = weak.upgrade() else { return };
        if ui.get_review_line_edit_open() {
            return;
        }
        let Some(target) = target(&ui, id) else {
            return;
        };
        let serial = {
            let mut state = draft_state.borrow_mut();
            state.serial = state.serial.wrapping_add(1);
            state.draft = None;
            state.serial
        };
        ui.set_review_line_edit_location(format!("{} · 新行 {}", target.selection, target.line));
        ui.set_review_line_edit_text(String::new());
        ui.set_review_line_edit_original(String::new());
        ui.set_review_line_edit_error(String::new());
        ui.set_review_line_edit_ready(false);
        ui.set_review_line_edit_busy(true);
        ui.set_review_line_edit_open(true);
        let weak = weak.clone();
        let state = draft_state.clone();
        let requested = target.clone();
        if let Err(error) = spawn_local(async move {
            let result =
                read_in_background("workbench-review-line", move || LineDraft::open(target)).await;
            let Some(mut ui) = weak.upgrade() else { return };
            if state.borrow().serial != serial || !ui.get_review_line_edit_open() {
                return;
            }
            ui.set_review_line_edit_busy(false);
            if !requested.current(&ui) {
                ui.set_review_line_edit_error("差异已切换，请重新打开编辑器".into());
                return;
            }
            match result {
                Ok(draft) => {
                    let text = draft.original[draft.range.clone()].to_owned();
                    ui.set_review_line_edit_text(text.clone());
                    ui.set_review_line_edit_original(text);
                    ui.set_review_line_edit_ready(true);
                    state.borrow_mut().draft = Some(draft);
                    ui.invoke_focus_review_line_edit();
                }
                Err(error) => ui.set_review_line_edit_error(error),
            }
        }) {
            ui.set_review_line_edit_busy(false);
            ui.set_review_line_edit_error(error.to_string());
        }
    });
    let weak = ui.as_weak();
    ui.on_save_review_line_edit(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        if !ui.get_review_line_edit_open() || ui.get_review_line_edit_busy() {
            return;
        }
        let (serial, draft) = {
            let state = state.borrow();
            (state.serial, state.draft.clone())
        };
        let Some(draft) = draft else { return };
        if !draft.target.current(&ui) {
            ui.set_review_line_edit_error("差异已切换，请重新打开编辑器".into());
            return;
        }
        let text = ui.get_review_line_edit_text();
        if text == draft.original[draft.range.clone()] {
            return;
        }
        let target = draft.target.clone();
        ui.set_review_line_edit_busy(true);
        ui.set_review_line_edit_error(String::new());
        let weak = weak.clone();
        let state = state.clone();
        if let Err(error) = spawn_local(async move {
            let result =
                read_in_background("workbench-save-review-line", move || draft.save(&text)).await;
            let Some(mut ui) = weak.upgrade() else { return };
            finish_save(&mut ui, &mut state.borrow_mut(), serial, &target, result);
        }) {
            ui.set_review_line_edit_busy(false);
            ui.set_review_line_edit_error(error.to_string());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::super::{git, set_patch};
    use super::*;
    use std::fs;

    #[test]
    fn saved_review_line_refreshes_the_real_worktree_diff_and_preserves_crlf() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let file = root.join("source.rs");
        git(&root, &["init", "-q"], 4096).unwrap();
        git(&root, &["config", "core.autocrlf", "false"], 4096).unwrap();
        let indexed = "head\r\n\told\r\ntail\r\n";
        fs::write(&file, indexed).unwrap();
        git(&root, &["add", "source.rs"], 4096).unwrap();
        fs::write(&file, "head\r\n\tworking\r\ntail\r\n").unwrap();
        let patch = |root: &Path| {
            let (bytes, _) = git(root, &["diff", "--", "source.rs"], 8192).unwrap();
            format!("未暂存\n{}", String::from_utf8(bytes).unwrap())
        };
        let mut ui = AgentWorkbench::new();
        ui.set_workspace(root.to_string_lossy().into_owned());
        ui.set_review_open(true);
        ui.set_review_selection("source.rs".into());
        set_patch(&mut ui, patch(&root));
        let row = ui
            .model_review_lines()
            .snapshot()
            .into_iter()
            .find(|row| row.kind == "add")
            .unwrap();
        let target = target(&ui, row.id).unwrap();
        assert_eq!(target.expected, "\tworking");
        let draft = LineDraft::open(target.clone()).unwrap();
        let mut state = EditState {
            serial: 1,
            draft: Some(draft.clone()),
        };
        ui.set_review_line_edit_open(true);
        ui.set_review_line_edit_busy(true);
        let weak = ui.as_weak();
        let repository = root.clone();
        ui.on_refresh_review(move || set_patch(&mut weak.upgrade().unwrap(), patch(&repository)));
        let result = draft.save("\tchanged\n\tinserted");
        assert!(result.is_ok());
        finish_save(&mut ui, &mut state, 1, &target, result);
        assert!(!ui.get_review_line_edit_open());
        assert!(ui.get_review_body().text().contains("+\tinserted"));
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "head\r\n\tchanged\r\n\tinserted\r\ntail\r\n"
        );
        assert_eq!(
            git(&root, &["show", ":source.rs"], 8192).unwrap().0,
            indexed.as_bytes()
        );
    }

    #[test]
    fn stale_patch_conflicts_keep_the_editor_draft_and_reject_index_rows() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let file = root.join("source.rs");
        fs::write(&file, "head\nworking\n").unwrap();
        let mut ui = AgentWorkbench::new();
        ui.set_workspace(root.to_string_lossy().into_owned());
        ui.set_review_open(true);
        ui.set_review_selection("source.rs".into());
        set_patch(&mut ui, "已暂存\n@@ -2 +2 @@\n-old\n+working\n".into());
        assert!(target(&ui, 3).is_none());
        set_patch(&mut ui, "未暂存\n@@ -2 +2 @@\n-old\n+working\n".into());
        let target = target(&ui, 3).unwrap();
        let draft = LineDraft::open(target.clone()).unwrap();
        let mut state = EditState {
            serial: 1,
            draft: Some(draft.clone()),
        };
        fs::write(&file, "head changed externally\nworking\n").unwrap();
        ui.set_review_line_edit_open(true);
        ui.set_review_line_edit_text("preserved draft".into());
        finish_save(
            &mut ui,
            &mut state,
            1,
            &target,
            draft.save("preserved draft"),
        );
        assert!(ui.get_review_line_edit_open());
        assert!(!ui.get_review_line_edit_busy());
        assert_eq!(ui.get_review_line_edit_text(), "preserved draft");
        assert!(ui.get_review_line_edit_error().contains("草稿仍保留"));
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "head changed externally\nworking\n"
        );
        fs::write(&file, "working\nunrelated\n").unwrap();
        assert!(matches!(LineDraft::open(target), Err(error) if error.contains("该行已变化")));
    }
}
