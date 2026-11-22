use super::{IndexAction, hunks};
use crate::workspace_files::{allowed_relative, read_text, save_text};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

pub(super) struct Selection {
    expected: String,
    patch: String,
}

impl Selection {
    pub(super) fn file(body: &str) -> Option<Self> {
        let mut lines = body.split_inclusive('\n');
        lines.find(|line| *line == "未暂存\n")?;
        let patch: String = lines.collect();
        hunks::editable(&patch).then(|| Self {
            expected: patch.clone(),
            patch,
        })
    }

    pub(super) fn hunk(body: &str, row: usize) -> Option<Self> {
        let selected = hunks::select(body, row)?;
        matches!(selected.action, IndexAction::Stage).then_some(Self {
            expected: selected.expected,
            patch: selected.patch,
        })
    }

    pub(super) fn preview(&self) -> &str {
        &self.patch
    }

    pub(super) fn apply(&self, workspace: &Path, path: &str) -> Result<(), String> {
        if path.is_empty() || !allowed_relative(Path::new(path)) {
            return Err("无法定位工作区文件".into());
        }
        let root = hunks::validated_root(workspace, path, IndexAction::Stage, &self.expected)?;
        let target = root.join(path);
        let workspace_text = workspace.to_str().ok_or("工作区路径需要使用 UTF-8")?;
        let (original, _) = read_text(workspace_text, &target)?;
        let replacement = reverse_with_git(&original, &self.patch)?;
        if hunks::validated_root(workspace, path, IndexAction::Stage, &self.expected)? != root {
            return Err("Git 工作区已变化，请刷新差异".into());
        }
        save_text(workspace_text, &target, &original, &replacement)
    }
}

/// Apply the exact textual patch to a sandbox copy. Publication uses the
/// editor's original-content check, so Git never writes to the real worktree here.
fn reverse_with_git(original: &str, diff: &str) -> Result<String, String> {
    let directory = tempfile::tempdir().map_err(|error| error.to_string())?;
    let file = directory.path().join("source");
    let crlf = original.contains("\r\n")
        && !diff.contains("\r\n")
        && original.bytes().enumerate().all(|(offset, byte)| {
            byte != b'\n' || offset > 0 && original.as_bytes()[offset - 1] == b'\r'
        });
    let input = if crlf {
        original.replace("\r\n", "\n")
    } else {
        original.to_owned()
    };
    fs::write(&file, input).map_err(|error| error.to_string())?;
    let body = diff
        .split_inclusive('\n')
        .skip_while(|line| !line.starts_with("@@ "))
        .collect::<String>();
    let patch = format!("diff --git a/source b/source\n--- a/source\n+++ b/source\n{body}");
    let mut command = Command::new("git");
    command
        .current_dir(directory.path())
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .args([
            "-c",
            "apply.ignoreWhitespace=no",
            "-c",
            "core.autocrlf=false",
            "apply",
            "--no-index",
            "--reverse",
            "--whitespace=nowarn",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(parent) = directory.path().parent() {
        command.env("GIT_CEILING_DIRECTORIES", parent);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("无法运行 Git：{error}"))?;
    let written = child
        .stdin
        .take()
        .ok_or("Git 未打开输入管道")
        .and_then(|mut stdin| {
            stdin
                .write_all(patch.as_bytes())
                .map_err(|_| "无法写入 Git 补丁")
        });
    let status = child
        .wait()
        .map_err(|error| format!("Git 执行失败：{error}"))?;
    written?;
    if !status.success() {
        return Err("无法预演该补丁的回退，请刷新差异".into());
    }
    let restored = fs::read_to_string(file).map_err(|error| error.to_string())?;
    Ok(if crlf {
        restored.replace('\n', "\r\n")
    } else {
        restored
    })
}

#[cfg(test)]
mod tests {
    use super::super::{Filter, git, load, propose_revert, set_patch};
    use super::*;
    use workbench_ui::AgentWorkbench;

    fn patch(root: &Path) -> String {
        load(root, "notes.txt", Filter::All).unwrap().body.unwrap()
    }

    #[test]
    fn tracked_hunk_and_file_revert_preserve_the_index_and_other_worktree_files() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let target = root.join("notes.txt");
        let other = root.join("other.txt");
        git(&root, &["init", "-q"], 4096).unwrap();
        git(&root, &["config", "core.autocrlf", "false"], 4096).unwrap();
        let indexed = (0..44)
            .map(|line| format!("indexed line {line}\n"))
            .collect::<String>();
        fs::write(&target, &indexed).unwrap();
        fs::write(&other, "index copy\n").unwrap();
        git(&root, &["add", "--", "notes.txt", "other.txt"], 4096).unwrap();
        let working = indexed
            .replace("indexed line 4\n", "first local edit\n")
            .replace("indexed line 35\n", "second local edit\n");
        fs::write(&target, &working).unwrap();
        fs::write(&other, "unrelated edit\n").unwrap();

        let display = patch(&root);
        let row = display
            .lines()
            .enumerate()
            .skip_while(|(_, line)| *line != "未暂存")
            .find(|(_, line)| line.starts_with("@@ "))
            .unwrap()
            .0;
        let hunk = Selection::hunk(&display, row).unwrap();
        assert!(hunk.preview().contains("+first local edit"));
        assert!(!hunk.preview().contains("+second local edit"));
        assert_eq!(fs::read_to_string(&target).unwrap(), working);
        hunk.apply(&root, "notes.txt").unwrap();
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            indexed.replace("indexed line 35\n", "second local edit\n")
        );
        assert!(hunk.apply(&root, "notes.txt").is_err());

        let file = Selection::file(&patch(&root)).unwrap();
        assert!(file.preview().contains("+second local edit"));
        file.apply(&root, "notes.txt").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), indexed);
        assert_eq!(fs::read_to_string(&other).unwrap(), "unrelated edit\n");
        assert_eq!(
            git(&root, &["show", ":notes.txt"], 8192).unwrap().0,
            indexed.as_bytes()
        );
        assert_eq!(
            git(&root, &["show", ":other.txt"], 8192).unwrap().0,
            b"index copy\n"
        );
    }

    #[test]
    fn revert_rejects_stale_worktree_and_index_versions() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let target = root.join("notes.txt");
        git(&root, &["init", "-q"], 4096).unwrap();
        fs::write(&target, "index\noriginal context\n").unwrap();
        git(&root, &["add", "--", "notes.txt"], 4096).unwrap();
        fs::write(&target, "worktree\noriginal context\n").unwrap();
        let proposal = Selection::file(&patch(&root)).unwrap();
        fs::write(&target, "worktree\nexternal edit\n").unwrap();
        assert!(
            proposal
                .apply(&root, "notes.txt")
                .unwrap_err()
                .contains("差异已变化")
        );
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "worktree\nexternal edit\n"
        );
        let proposal = Selection::file(&patch(&root)).unwrap();
        git(&root, &["add", "--", "notes.txt"], 4096).unwrap();
        assert!(
            proposal
                .apply(&root, "notes.txt")
                .unwrap_err()
                .contains("差异已变化")
        );
        assert_eq!(
            git(&root, &["show", ":notes.txt"], 4096).unwrap().0,
            b"worktree\nexternal edit\n"
        );
    }

    #[test]
    fn normalized_patch_restores_text_without_changing_crlf() {
        let diff = "diff --git a/source b/source\n--- a/source\n+++ b/source\n@@ -1,2 +1,2 @@\n-old\n+edited\n keep\n";
        let replacement = reverse_with_git("edited\r\nkeep\r\n", diff).unwrap();
        assert_eq!(replacement, "old\r\nkeep\r\n");
    }

    #[test]
    fn a_preview_is_bound_to_its_visible_patch_and_only_selects_unstaged_text() {
        const DIFF: &str = "diff --git a/notes.txt b/notes.txt\nindex 1125c41..56aef2b 100644\n--- a/notes.txt\n+++ b/notes.txt\n@@ -1 +1 @@\n-old\n+new\n";
        let mut ui = AgentWorkbench::new();
        ui.set_workspace("/workspace".into());
        ui.set_review_open(true);
        ui.set_review_selection("notes.txt".into());
        ui.set_review_can_stage_hunks(true);
        let body = format!("未暂存\n{DIFF}");
        set_patch(&mut ui, body.clone());
        let proposal = propose_revert(&mut ui, Some(5)).unwrap();
        assert_eq!(ui.get_review_revert_path(), "notes.txt");
        assert!(ui.get_review_revert_preview().text().contains("+new"));
        assert!(proposal.visible_patch_is_current(&ui));
        set_patch(&mut ui, body.replace("+new", "+other"));
        assert!(!proposal.visible_patch_is_current(&ui));
        ui.set_review_revert_open(false);
        set_patch(&mut ui, format!("已暂存\n{DIFF}"));
        assert!(propose_revert(&mut ui, Some(5)).is_none());
        assert!(propose_revert(&mut ui, None).is_none());
    }
}
