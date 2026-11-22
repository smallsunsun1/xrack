use super::{
    STATUS_LIMIT, WorkspaceScope, can_update_index, git, git_command, git_root, read_limited,
};
use crate::read_in_background;
use session::{ConversationActivity, Snapshot};
use std::{
    cell::{Cell, RefCell},
    io::{Seek, Write},
    path::{Path, PathBuf},
    rc::Rc,
};
use tokio::sync::watch;
use workbench_ui::AgentWorkbench;

#[derive(Debug, PartialEq, Eq)]
struct StagedIndex {
    root: PathBuf,
    branch: Option<String>,
    head: Option<String>,
    fingerprint: Vec<u8>,
    files: Vec<String>,
}

pub(super) fn revision(root: &Path) -> Option<String> {
    let (value, truncated) = git(root, &["rev-parse", "--verify", "--quiet", "HEAD"], 256).ok()?;
    (!truncated).then_some(())?;
    std::str::from_utf8(&value)
        .ok()
        .map(|value| value.trim().to_owned())
}

impl StagedIndex {
    fn read(workspace: &Path) -> Result<Self, String> {
        let root = git_root(workspace)?;
        let (directory, truncated) =
            git(&root, &["rev-parse", "--absolute-git-dir"], STATUS_LIMIT)?;
        let directory = std::str::from_utf8(&directory).map_err(|_| "Git 路径必须是 UTF-8")?;
        if truncated {
            return Err("Git 目录路径过长".into());
        }
        let directory = Path::new(
            directory
                .strip_suffix("\r\n")
                .or_else(|| directory.strip_suffix('\n'))
                .unwrap_or(directory),
        );
        if [
            "MERGE_HEAD",
            "CHERRY_PICK_HEAD",
            "REVERT_HEAD",
            "rebase-apply",
            "rebase-merge",
            "sequencer",
        ]
        .iter()
        .any(|operation| directory.join(operation).exists())
        {
            return Err("请先完成当前的合并、变基或其他 Git 操作".into());
        }
        let branch = git(
            &root,
            &["symbolic-ref", "--quiet", "--short", "HEAD"],
            STATUS_LIMIT,
        )
        .ok()
        .and_then(|(branch, truncated)| (!truncated).then_some(branch))
        .and_then(|branch| String::from_utf8(branch).ok())
        .map(|branch| branch.trim_end_matches(['\r', '\n']).to_owned());
        let head = revision(&root);
        if branch.is_none() && head.is_none() {
            return Err("无法读取当前 Git 分支或提交".into());
        }
        let (fingerprint, truncated) = git(
            &root,
            &[
                "diff",
                "--cached",
                "--raw",
                "-z",
                "--no-abbrev",
                "--no-renames",
                "--no-ext-diff",
                "--ita-invisible-in-index",
                "--",
            ],
            STATUS_LIMIT,
        )?;
        if truncated {
            return Err("暂存列表超出读取上限".into());
        }
        let files = staged_paths(&fingerprint)?;
        Ok(Self {
            root,
            branch,
            head,
            fingerprint,
            files,
        })
    }

    fn commit(self, workspace: &Path, message: &str) -> Result<String, String> {
        if message.trim().is_empty() || message.contains('\0') {
            return Err("请输入有效的提交说明".into());
        }
        if self.files.is_empty() || Self::read(workspace)? != self {
            return Err("分支或暂存区已变化，请重新读取暂存内容".into());
        }
        let mut input = tempfile::tempfile().map_err(|error| error.to_string())?;
        input
            .write_all(message.as_bytes())
            .and_then(|()| input.rewind())
            .map_err(|error| error.to_string())?;
        let mut log = tempfile::tempfile().map_err(|error| error.to_string())?;
        let stderr = log.try_clone().map_err(|error| error.to_string())?;
        let stdout = log.try_clone().map_err(|error| error.to_string())?;
        let status = git_command(&self.root)
            .args(["commit", "--file=-"])
            .stdin(input)
            .stdout(stdout)
            .stderr(stderr)
            .status()
            .map_err(|error| error.to_string())?;
        if !status.success() {
            let detail = log
                .rewind()
                .and_then(|()| read_limited(log, 4096))
                .map(|(bytes, _)| String::from_utf8_lossy(&bytes).trim().to_owned())
                .unwrap_or_default();
            return Err(if detail.is_empty() {
                status.to_string()
            } else {
                detail
            });
        }
        Ok(revision(&self.root).unwrap_or_default())
    }
}

fn staged_paths(raw: &[u8]) -> Result<Vec<String>, String> {
    let mut records = raw.split(|byte| *byte == 0);
    let mut files = Vec::new();
    while let Some(header) = records.next().filter(|header| !header.is_empty()) {
        let status = header
            .last()
            .copied()
            .filter(|_| header.starts_with(b":"))
            .filter(|status| matches!(status, b'A' | b'D' | b'M' | b'T' | b'U' | b'X'))
            .ok_or("Git 返回了无效的暂存记录")?;
        if status == b'U' {
            return Err("请先解决暂存区中的合并冲突".into());
        }
        let path = records
            .next()
            .filter(|path| !path.is_empty())
            .ok_or("暂存路径缺失")?;
        let path = String::from_utf8_lossy(path)
            .replace('\r', "\\r")
            .replace('\n', "\\n")
            .replace('\t', "\\t");
        files.push(format!("{}    {path}", char::from(status)));
    }
    Ok(files)
}

type Proposal = Rc<RefCell<Option<(WorkspaceScope, StagedIndex)>>>;

fn prepare(ui: &mut AgentWorkbench, proposal: Proposal, generation: Rc<Cell<u64>>) {
    let scope = WorkspaceScope::current(ui);
    proposal.take();
    let ticket = generation.get().wrapping_add(1);
    generation.set(ticket);
    ui.set_git_commit_ready(false);
    ui.set_git_commit_loading(true);
    ui.set_git_commit_error(String::new());
    let weak = ui.as_weak();
    let workspace = PathBuf::from(&scope.workspace);
    if let Err(error) = xrack_platform_winit::spawn_local(async move {
        let result = read_in_background("workbench-commit-preview", move || {
            StagedIndex::read(&workspace)
        })
        .await;
        let Some(mut ui) = weak.upgrade() else { return };
        if generation.get() != ticket {
            return;
        }
        ui.set_git_commit_loading(false);
        if !scope.matches(&ui) || !ui.get_git_commit_open() {
            return;
        }
        match result {
            Ok(index) => {
                ui.set_git_commit_detached(index.branch.is_none());
                ui.set_git_commit_branch(
                    index
                        .branch
                        .clone()
                        .or_else(|| {
                            index
                                .head
                                .as_ref()
                                .map(|head| head.chars().take(12).collect())
                        })
                        .unwrap_or_default(),
                );
                ui.set_git_commit_files(index.files.clone());
                ui.set_git_commit_ready(!index.files.is_empty());
                proposal.replace(Some((scope, index)));
            }
            Err(error) => ui.set_git_commit_error(error),
        }
    }) {
        ui.set_git_commit_loading(false);
        ui.set_git_commit_error(error.to_string());
    }
}

pub(super) fn attach(ui: &mut AgentWorkbench, snapshots: watch::Receiver<Snapshot>) {
    let proposal = Rc::new(RefCell::new(None));
    let generation = Rc::new(Cell::new(0u64));
    let weak = ui.as_weak();
    let pending = proposal.clone();
    let read_generation = generation.clone();
    ui.on_open_git_commit(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        if !can_update_index(&ui) || ui.get_git_commit_open() || ui.get_review_staged_count() == 0 {
            return;
        }
        ui.set_git_commit_open(true);
        ui.set_git_commit_done(false);
        ui.set_git_commit_result(String::new());
        ui.set_git_commit_branch(String::new());
        ui.set_git_commit_detached(false);
        ui.set_git_commit_files(Vec::new());
        ui.set_git_commit_message(String::new());
        ui.invoke_focus_git_commit_message();
        prepare(&mut ui, pending.clone(), read_generation.clone());
    });

    let weak = ui.as_weak();
    let pending = proposal.clone();
    ui.on_reload_git_commit(move || {
        if let Some(mut ui) = weak.upgrade()
            && ui.get_git_commit_open()
            && !ui.get_git_commit_loading()
            && !ui.get_git_commit_pending()
            && !ui.get_git_commit_done()
        {
            prepare(&mut ui, pending.clone(), generation.clone());
        }
    });

    let weak = ui.as_weak();
    ui.on_confirm_git_commit(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        if !ui.get_git_commit_open() || !ui.get_can_confirm_git_commit() || !can_update_index(&ui) {
            return;
        }
        let Some((scope, index)) = proposal.take().filter(|(scope, _)| scope.matches(&ui)) else {
            ui.set_git_commit_ready(false);
            return;
        };
        let running = snapshots.borrow().conversations.iter().any(|row| {
            matches!(
                row.activity,
                ConversationActivity::Running | ConversationActivity::NeedsInput
            ) && Path::new(&row.workspace).starts_with(&index.root)
        });
        if running {
            proposal.replace(Some((scope, index)));
            ui.set_git_commit_error("此工作树有活动会话，请等待当前轮次结束".into());
            return;
        }
        ui.set_git_commit_pending(true);
        ui.set_review_index_busy(true);
        ui.set_git_commit_ready(false);
        ui.set_git_commit_error(String::new());
        let message = ui.get_git_commit_message();
        let workspace = PathBuf::from(&scope.workspace);
        let weak_result = weak.clone();
        if let Err(error) = xrack_platform_winit::spawn_local(async move {
            let result = read_in_background("workbench-git-commit", move || {
                index.commit(&workspace, &message)
            })
            .await;
            let Some(mut ui) = weak_result.upgrade() else {
                return;
            };
            ui.set_git_commit_pending(false);
            ui.set_review_index_busy(false);
            if !scope.matches(&ui) {
                return;
            }
            match result {
                Ok(hash) => {
                    ui.set_git_commit_done(true);
                    ui.set_git_commit_result(hash.chars().take(12).collect());
                    ui.invoke_refresh_review();
                }
                Err(error) => ui.set_git_commit_error(error),
            }
        }) {
            ui.set_git_commit_pending(false);
            ui.set_review_index_busy(false);
            ui.set_git_commit_error(error.to_string());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn repo() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path();
        run(root, &["init", "-q", "-b", "main"])?;
        for (key, value) in [
            ("user.name", "XRack fixture"),
            ("user.email", "xrack@example.invalid"),
            ("commit.gpgSign", "false"),
        ] {
            run(root, &["config", "--local", key, value])?;
        }
        let hooks = root.join(".git/test-hooks");
        fs::create_dir(&hooks)?;
        run(
            root,
            &[
                "config",
                "--local",
                "core.hooksPath",
                hooks.to_str().unwrap(),
            ],
        )?;
        Ok(temporary)
    }

    fn run(root: &Path, args: &[&str]) -> Result<String, String> {
        let output = git_command(root)
            .args(args)
            .output()
            .map_err(|error| error.to_string())?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
            .ok_or_else(|| String::from_utf8_lossy(&output.stderr).into_owned())
    }

    #[test]
    fn commits_only_staged_content_and_preserves_worktree_edits()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = repo()?;
        let root = fixture.path().canonicalize()?;
        fs::write(root.join("source.rs"), "baseline\n")?;
        run(&root, &["add", "--", "source.rs"])?;
        let initial = StagedIndex::read(&root)?;
        assert_eq!(initial.branch.as_deref(), Some("main"));
        assert!(initial.head.is_none());
        initial.commit(&root, "Initial fixture")?;

        fs::write(root.join("source.rs"), "staged version\n")?;
        fs::write(root.join("界 面.md"), "staged addition\n")?;
        run(&root, &["add", "--", "source.rs", "界 面.md"])?;
        fs::write(root.join("source.rs"), "later worktree content\n")?;
        fs::write(root.join("untracked.txt"), "still untracked\n")?;
        fs::write(root.join("intent.txt"), "intent only\n")?;
        run(&root, &["add", "-N", "--", "intent.txt"])?;
        let staged = StagedIndex::read(&root)?;
        assert_eq!(staged.files, ["M    source.rs", "A    界 面.md"]);
        let hash = staged.commit(&root, "Commit staged source\n\nKeep the local draft.")?;
        assert_eq!(revision(&root).as_deref(), Some(hash.as_str()));
        assert_eq!(run(&root, &["show", "HEAD:source.rs"])?, "staged version\n");
        assert_eq!(run(&root, &["show", "HEAD:界 面.md"])?, "staged addition\n");
        assert_eq!(
            fs::read_to_string(root.join("source.rs"))?,
            "later worktree content\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("untracked.txt"))?,
            "still untracked\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("intent.txt"))?,
            "intent only\n"
        );
        assert!(run(&root, &["show", "HEAD:intent.txt"]).is_err());
        assert!(run(&root, &["log", "-1", "--format=%B"])?.contains("Keep the local draft."));
        Ok(())
    }

    #[test]
    fn rejects_a_changed_index_or_branch_before_writing_a_commit()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = repo()?;
        let root = fixture.path();
        run(root, &["commit", "--allow-empty", "-m", "Baseline"])?;
        let original = revision(root);
        fs::write(root.join("staged.txt"), "first version")?;
        run(root, &["add", "--", "staged.txt"])?;
        let stale = StagedIndex::read(root)?;
        fs::write(root.join("staged.txt"), "replacement")?;
        run(root, &["add", "--", "staged.txt"])?;
        assert!(stale.commit(root, "Old staged content").is_err());
        let other_branch = StagedIndex::read(root)?;
        run(root, &["switch", "-c", "other"])?;
        assert!(other_branch.commit(root, "Old branch").is_err());
        assert_eq!(revision(root), original);
        assert_eq!(run(root, &["show", ":staged.txt"])?, "replacement");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn preserves_the_index_and_surfaces_a_git_hook_failure()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::PermissionsExt;
        let fixture = repo()?;
        let root = fixture.path();
        let hook = root.join(".git/test-hooks/pre-commit");
        fs::write(
            &hook,
            "#!/bin/sh\nprintf '%s\\n' 'fixture hook rejected this commit' >&2\nexit 1\n",
        )?;
        fs::set_permissions(hook, fs::Permissions::from_mode(0o700))?;
        fs::write(root.join("example"), "staged")?;
        run(root, &["add", "--", "example"])?;
        let result = StagedIndex::read(root)?
            .commit(root, "Hook failure")
            .unwrap_err();
        assert!(result.contains("fixture hook rejected this commit"));
        assert!(revision(root).is_none());
        assert_eq!(run(root, &["show", ":example"])?, "staged");
        Ok(())
    }

    #[test]
    fn commits_in_a_detached_worktree_without_advancing_the_parent_branch()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = repo()?;
        let root = fixture.path();
        run(root, &["commit", "--allow-empty", "-m", "Baseline"])?;
        let initial = revision(root);
        let worktree_parent = tempfile::tempdir()?;
        let worktree = worktree_parent.path().join("detached");
        run(
            root,
            &[
                "worktree",
                "add",
                "--detach",
                worktree.to_str().unwrap(),
                "HEAD",
            ],
        )?;
        fs::write(worktree.join("detached.txt"), "worktree only")?;
        run(&worktree, &["add", "--", "detached.txt"])?;
        let index = StagedIndex::read(&worktree)?;
        assert!(index.branch.is_none());
        assert_eq!(index.head, initial);
        let committed = index.commit(&worktree, "Commit in detached worktree")?;
        assert_eq!(revision(&worktree).as_deref(), Some(committed.as_str()));
        assert_eq!(revision(root), initial);
        assert_eq!(
            run(&worktree, &["show", "HEAD:detached.txt"])?,
            "worktree only"
        );
        Ok(())
    }
}
