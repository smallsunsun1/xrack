use super::{
    STATUS_LIMIT, WorkspaceScope, commit::revision, git, git_command, git_root,
    push::current_branch, read_limited,
};
use crate::read_in_background;
use session::{ConversationActivity, Snapshot};
use std::{
    cell::{Cell, RefCell},
    io::Seek,
    path::{Path, PathBuf},
    process::Stdio,
    rc::Rc,
};
use tokio::sync::watch;
use workbench_ui::{AgentWorkbench, BranchChoice};

#[derive(PartialEq, Eq)]
struct Checkout {
    root: PathBuf,
    branch: Option<String>,
    head: Option<String>,
}

impl Checkout {
    fn read(workspace: &Path) -> Result<Self, String> {
        let root = git_root(workspace)?;
        let branch = current_branch(&root);
        let head = revision(&root);
        if branch.is_none() && head.is_none() {
            return Err("无法读取当前 Git 工作树".into());
        }
        Ok(Self { root, branch, head })
    }

    fn label(&self) -> String {
        self.branch
            .clone()
            .or_else(|| {
                self.head
                    .as_ref()
                    .map(|head| head.chars().take(12).collect())
            })
            .unwrap_or_default()
    }

    fn publish(&self, ui: &mut AgentWorkbench) -> bool {
        let previous = ui.get_git_checkout_label();
        let label = self.label();
        let detached = self.branch.is_none();
        if previous == label && ui.get_git_checkout_detached() == detached {
            return false;
        }
        ui.set_git_checkout_label(label);
        ui.set_git_checkout_detached(detached);
        !previous.is_empty()
    }
}

#[derive(PartialEq, Eq)]
struct Branch {
    name: String,
    commit: String,
    checked_out: bool,
}

fn local_branches(root: &Path) -> Result<Vec<Branch>, String> {
    let (bytes, truncated) = git(
        root,
        &[
            "for-each-ref",
            "--sort=refname",
            "--format=%(refname:lstrip=2)%00%(objectname)%00%(if)%(worktreepath)%(then)1%(else)0%(end)",
            "refs/heads/",
        ],
        STATUS_LIMIT,
    )?;
    if truncated {
        return Err("本地分支列表超出读取上限".into());
    }
    std::str::from_utf8(&bytes)
        .map_err(|_| "分支名必须使用 UTF-8")?
        .lines()
        .map(|line| {
            let mut values = line.split('\0');
            let name = values
                .next()
                .filter(|name| !name.is_empty())
                .ok_or("无效的 Git 分支名")?;
            let commit = values
                .next()
                .filter(|commit| !commit.is_empty())
                .ok_or("缺少分支提交 ID")?;
            let checked_out = match values.next() {
                Some("1") => true,
                Some("0") => false,
                _ => return Err("无法读取分支的工作树占用状态".into()),
            };
            Ok(Branch {
                name: name.into(),
                commit: commit.into(),
                checked_out,
            })
        })
        .collect()
}

pub(crate) fn worktree_branches(root: &Path) -> Result<Vec<(String, String)>, String> {
    Ok(local_branches(root)?
        .into_iter()
        .map(|branch| (branch.name, branch.commit))
        .collect())
}

struct Plan {
    checkout: Checkout,
    branches: Vec<Branch>,
}

struct Completed {
    checkout: Checkout,
    warning: String,
}

impl Plan {
    fn read(workspace: &Path) -> Result<Self, String> {
        let checkout = Checkout::read(workspace)?;
        let branches = local_branches(&checkout.root)?;
        Ok(Self { checkout, branches })
    }

    fn change(
        &self,
        workspace: &Path,
        name: &str,
        create: bool,
        running: &[PathBuf],
    ) -> Result<Completed, String> {
        if name.is_empty() || name.starts_with('-') || name.chars().any(char::is_control) {
            return Err("请输入有效的 Git 分支名".into());
        }
        let current = Checkout::read(workspace)?;
        if current != self.checkout {
            return Err("当前分支或提交已变化，请重新读取分支列表".into());
        }
        if running
            .iter()
            .any(|workspace| git_root(workspace).as_ref() == Ok(&current.root))
        {
            return Err("此工作树有活动会话，请等待当前轮次结束".into());
        }
        let target = local_branches(&current.root)?;
        if create {
            let (checked, truncated) =
                git(&current.root, &["check-ref-format", "--branch", name], 4096)
                    .map_err(|_| "请输入有效的 Git 分支名")?;
            if truncated || std::str::from_utf8(&checked).map(str::trim_end) != Ok(name) {
                return Err("请输入明确的分支名".into());
            }
            if current.branch.as_deref() == Some(name)
                || target.iter().any(|branch| branch.name == name)
            {
                return Err("该本地分支已存在".into());
            }
        } else {
            let selected = self
                .branches
                .iter()
                .find(|branch| branch.name == name)
                .ok_or("请从列表选择本地分支")?;
            let latest = target
                .iter()
                .find(|branch| branch.name == name)
                .ok_or("目标分支已被删除")?;
            if latest.checked_out {
                return Err("这个分支已在工作树中检出".into());
            }
            if latest != selected {
                return Err("目标分支的提交已变化，请重新读取分支列表".into());
            }
        }
        let mut command = git_command(&current.root);
        command.args([
            "switch",
            "--no-guess",
            "--no-overwrite-ignore",
            "--no-recurse-submodules",
        ]);
        if create {
            command.args(["-c", name]);
            if let Some(start) = &current.head {
                command.arg(start);
            }
        } else {
            command.args(["--", name]);
        }
        let mut output = tempfile::tempfile().map_err(|error| error.to_string())?;
        let stdout = output.try_clone().map_err(|error| error.to_string())?;
        let stderr = output.try_clone().map_err(|error| error.to_string())?;
        let status = command
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .status()
            .map_err(|error| format!("无法运行 Git：{error}"))?;
        let checkout = Checkout::read(workspace)?;
        let detail = || {
            output
                .rewind()
                .and_then(|()| read_limited(output, 8192))
                .map(|(bytes, _)| String::from_utf8_lossy(&bytes).trim().to_owned())
                .unwrap_or_default()
        };
        if checkout.branch.as_deref() == Some(name) {
            return Ok(Completed {
                checkout,
                warning: if status.success() {
                    String::new()
                } else {
                    detail()
                },
            });
        }
        let error = detail();
        Err(if error.is_empty() {
            format!("Git 未切换到所选分支（{status}）")
        } else {
            error
        })
    }
}

type PendingPlan = Rc<RefCell<Option<(WorkspaceScope, Plan)>>>;

fn list(ui: &mut AgentWorkbench, plan: &Plan, filter: &str) {
    let filter = filter.to_lowercase();
    let rows = plan
        .branches
        .iter()
        .filter(|branch| branch.name.to_lowercase().contains(&filter))
        .map(|branch| {
            let current = plan.checkout.branch.as_ref() == Some(&branch.name);
            BranchChoice {
                name: branch.name.clone(),
                state: if current {
                    "current"
                } else if branch.checked_out {
                    "occupied"
                } else {
                    ""
                }
                .into(),
            }
        })
        .collect();
    ui.set_git_branch_choices(rows);
}

fn prepare(ui: &mut AgentWorkbench, pending: PendingPlan, generation: Rc<Cell<u64>>) {
    let scope = WorkspaceScope::current(ui);
    pending.take();
    let ticket = generation.get().wrapping_add(1);
    generation.set(ticket);
    ui.set_git_branch_target(String::new());
    ui.set_git_branch_choices(Vec::new());
    ui.set_git_branch_loading(true);
    ui.set_git_branch_error(String::new());
    let workspace = PathBuf::from(&scope.workspace);
    let weak = ui.as_weak();
    if let Err(error) = xrack_platform_winit::spawn_local(async move {
        let result = read_in_background("workbench-branches", move || Plan::read(&workspace)).await;
        let Some(mut ui) = weak.upgrade() else { return };
        if generation.get() != ticket {
            return;
        }
        ui.set_git_branch_loading(false);
        if !scope.matches(&ui) || !ui.get_git_branch_open() {
            return;
        }
        match result {
            Ok(plan) => {
                if plan.checkout.publish(&mut ui) {
                    refresh_changed_files(&mut ui);
                }
                let query = ui.get_git_branch_query();
                list(&mut ui, &plan, &query);
                ui.set_git_branch_ready(true);
                pending.replace(Some((scope, plan)));
            }
            Err(error) => ui.set_git_branch_error(error),
        }
    }) {
        ui.set_git_branch_loading(false);
        ui.set_git_branch_error(error.to_string());
    }
}

fn refresh_changed_files(ui: &mut AgentWorkbench) {
    ui.set_git_checkout_revision(ui.get_git_checkout_revision().wrapping_add(1));
    ui.invoke_dismiss_file_mention();
    ui.invoke_workspace_checkout_changed();
    super::workspace_changed(ui);
}

pub(super) fn attach(ui: &mut AgentWorkbench, snapshots: watch::Receiver<Snapshot>) {
    let badge_loading = Rc::new(Cell::new(false));
    let weak = ui.as_weak();
    ui.on_refresh_git_checkout(move || {
        let Some(ui) = weak.upgrade() else { return };
        let workspace = ui.get_workspace();
        if workspace.is_empty()
            || ui.get_git_branch_open()
            || ui.get_git_branch_pending()
            || badge_loading.replace(true)
        {
            return;
        }
        let active = badge_loading.clone();
        let weak_result = weak.clone();
        if xrack_platform_winit::spawn_local(async move {
            let path = PathBuf::from(&workspace);
            let result =
                read_in_background("workbench-current-branch", move || Checkout::read(&path)).await;
            active.set(false);
            if let Some(mut ui) = weak_result.upgrade()
                && ui.get_workspace() == workspace
                && !ui.get_git_branch_open()
                && !ui.get_git_branch_pending()
            {
                match result {
                    Ok(checkout) => {
                        if checkout.publish(&mut ui) {
                            refresh_changed_files(&mut ui);
                        }
                    }
                    Err(_) => ui.set_git_checkout_label(String::new()),
                }
            }
        })
        .is_err()
        {
            badge_loading.set(false);
        }
    });

    let pending = Rc::new(RefCell::new(None));
    let generation = Rc::new(Cell::new(0u64));
    let weak = ui.as_weak();
    let open_plan = pending.clone();
    let open_gen = generation.clone();
    ui.on_open_git_branches(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        if ui.get_workspace().is_empty()
            || ui.get_switching()
            || ui.get_git_branch_open()
            || ui.get_git_branch_pending()
        {
            return;
        }
        ui.set_git_branch_open(true);
        ui.set_git_branch_done(false);
        ui.set_git_branch_ready(false);
        ui.set_git_branch_new_name(String::new());
        ui.set_git_branch_query(String::new());
        ui.set_git_branch_mode(
            if ui.get_git_checkout_detached() {
                "create"
            } else {
                "switch"
            }
            .into(),
        );
        prepare(&mut ui, open_plan.clone(), open_gen.clone());
    });

    let weak = ui.as_weak();
    let refresh_plan = pending.clone();
    ui.on_reload_git_branches(move || {
        if let Some(mut ui) = weak.upgrade()
            && ui.get_git_branch_open()
            && !ui.get_git_branch_loading()
            && !ui.get_git_branch_pending()
            && !ui.get_git_branch_done()
        {
            ui.set_git_branch_ready(false);
            prepare(&mut ui, refresh_plan.clone(), generation.clone());
        }
    });

    let weak = ui.as_weak();
    let search_plan = pending.clone();
    ui.on_filter_git_branches(move |query| {
        if let Some(mut ui) = weak.upgrade()
            && let Some((scope, plan)) = search_plan.borrow().as_ref()
            && scope.matches(&ui)
        {
            ui.set_git_branch_target(String::new());
            list(&mut ui, plan, &query);
        }
    });

    let weak = ui.as_weak();
    ui.on_confirm_git_branch(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        if !ui.get_git_branch_open() || !ui.get_can_confirm_git_branch() {
            return;
        }
        let Some((scope, plan)) = pending.take().filter(|(scope, _)| scope.matches(&ui)) else {
            return;
        };
        let create = ui.get_git_branch_mode() == "create";
        if !create && !ui.invoke_can_switch_workspace_checkout() {
            pending.replace(Some((scope, plan)));
            ui.set_git_branch_error("请先保存或关闭此工作目录中有未保存改动的编辑器".into());
            return;
        }
        let name = if create {
            ui.get_git_branch_new_name()
        } else {
            ui.get_git_branch_target()
        };
        let running = snapshots
            .borrow()
            .conversations
            .iter()
            .filter(|row| {
                matches!(
                    row.activity,
                    ConversationActivity::Running | ConversationActivity::NeedsInput
                )
            })
            .map(|row| PathBuf::from(&row.workspace))
            .collect::<Vec<_>>();
        ui.set_git_branch_pending(true);
        ui.set_git_branch_error(String::new());
        let weak_result = weak.clone();
        let retry = pending.clone();
        let workspace = PathBuf::from(&scope.workspace);
        if let Err(error) = xrack_platform_winit::spawn_local(async move {
            let result = read_in_background("workbench-git-switch", move || {
                let outcome = plan.change(&workspace, name.trim(), create, &running);
                Ok((plan, outcome))
            })
            .await;
            let Some(mut ui) = weak_result.upgrade() else {
                return;
            };
            ui.set_git_branch_pending(false);
            if !scope.matches(&ui) {
                return;
            }
            match result {
                Ok((_, Ok(completed))) => {
                    completed.checkout.publish(&mut ui);
                    ui.set_git_branch_done(true);
                    ui.set_git_branch_error(completed.warning);
                    if !create {
                        refresh_changed_files(&mut ui);
                    }
                }
                Ok((plan, Err(error))) => {
                    retry.replace(Some((scope, plan)));
                    ui.set_git_branch_error(error);
                }
                Err(error) => ui.set_git_branch_error(error),
            }
        }) {
            ui.set_git_branch_pending(false);
            ui.set_git_branch_error(error.to_string());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn run(root: &Path, args: &[&str]) -> Result<String, String> {
        let response = git_command(root)
            .args(args)
            .output()
            .map_err(|error| error.to_string())?;
        if response.status.success() {
            Ok(String::from_utf8_lossy(&response.stdout).trim_end().into())
        } else {
            Err(String::from_utf8_lossy(&response.stderr).trim_end().into())
        }
    }

    fn fixture() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path();
        run(root, &["init", "-q", "-b", "main"])?;
        let hooks = root.join(".git/test-hooks");
        fs::create_dir(&hooks)?;
        for (key, value) in [
            ("user.name", "XRack fixture"),
            ("user.email", "xrack@example.invalid"),
            ("commit.gpgSign", "false"),
            ("core.hooksPath", hooks.to_str().unwrap()),
        ] {
            run(root, &["config", "--local", key, value])?;
        }
        fs::write(root.join(".gitignore"), "cache.data\n")?;
        fs::write(root.join("common.txt"), "baseline")?;
        fs::write(root.join("specific.txt"), "main")?;
        run(
            root,
            &["add", "--", ".gitignore", "common.txt", "specific.txt"],
        )?;
        run(root, &["commit", "-qm", "Baseline"])?;
        run(root, &["switch", "-c", "feature/ui"])?;
        fs::write(root.join("specific.txt"), "feature")?;
        run(root, &["add", "--", "specific.txt"])?;
        run(root, &["commit", "-qm", "Feature"])?;
        run(root, &["switch", "main"])?;
        Ok(temporary)
    }

    #[test]
    fn switches_branches_and_preserves_compatible_staged_and_unstaged_changes()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = fixture()?;
        let root = fixture.path();
        fs::write(root.join("common.txt"), "staged")?;
        run(root, &["add", "--", "common.txt"])?;
        fs::write(root.join("common.txt"), "unstaged")?;
        fs::write(root.join("cache.data"), "ignored")?;
        let result = Plan::read(root)?.change(root, "feature/ui", false, &[])?;
        assert_eq!(result.checkout.branch.as_deref(), Some("feature/ui"));
        assert_eq!(fs::read_to_string(root.join("specific.txt"))?, "feature");
        assert_eq!(run(root, &["show", ":common.txt"])?, "staged");
        assert_eq!(fs::read_to_string(root.join("common.txt"))?, "unstaged");
        assert_eq!(fs::read_to_string(root.join("cache.data"))?, "ignored");
        Ok(())
    }

    #[test]
    fn git_rejects_tracked_and_ignored_file_collisions_before_switching()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = fixture()?;
        let root = fixture.path();
        fs::write(root.join("specific.txt"), "local draft")?;
        assert!(
            Plan::read(root)?
                .change(root, "feature/ui", false, &[])
                .is_err()
        );
        assert_eq!(current_branch(root).as_deref(), Some("main"));
        assert_eq!(
            fs::read_to_string(root.join("specific.txt"))?,
            "local draft"
        );
        run(root, &["restore", "--", "specific.txt"])?;
        run(root, &["switch", "feature/ui"])?;
        fs::write(root.join("cache.data"), "tracked on feature")?;
        run(root, &["add", "-f", "--", "cache.data"])?;
        run(root, &["commit", "-qm", "Track cache on feature"])?;
        run(root, &["switch", "main"])?;
        fs::write(root.join("cache.data"), "keep ignored bytes")?;
        assert!(
            Plan::read(root)?
                .change(root, "feature/ui", false, &[])
                .is_err()
        );
        assert_eq!(current_branch(root).as_deref(), Some("main"));
        assert_eq!(
            fs::read_to_string(root.join("cache.data"))?,
            "keep ignored bytes"
        );
        Ok(())
    }

    #[test]
    fn detects_checkout_occupancy_and_a_changed_target_tip()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = fixture()?;
        let root = fixture.path();
        let stale = Plan::read(root)?;
        let head = revision(root).unwrap();
        run(root, &["update-ref", "refs/heads/feature/ui", &head])?;
        assert!(
            stale
                .change(root, "feature/ui", false, &[])
                .err()
                .unwrap()
                .contains("目标分支的提交已变化")
        );
        let sibling = tempfile::tempdir()?;
        let worktree = sibling.path().join("other");
        run(
            root,
            &["worktree", "add", worktree.to_str().unwrap(), "feature/ui"],
        )?;
        let refreshed = Plan::read(root)?;
        assert!(
            refreshed
                .branches
                .iter()
                .find(|branch| branch.name == "feature/ui")
                .unwrap()
                .checked_out
        );
        assert!(
            refreshed
                .change(root, "feature/ui", false, &[])
                .err()
                .unwrap()
                .contains("已在工作树中检出")
        );
        assert_eq!(current_branch(root).as_deref(), Some("main"));
        assert_eq!(current_branch(&worktree).as_deref(), Some("feature/ui"));
        Ok(())
    }

    #[test]
    fn creates_a_branch_at_the_detached_worktree_head_and_keeps_its_index()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = fixture()?;
        let root = fixture.path();
        let sibling = tempfile::tempdir()?;
        let worktree = sibling.path().join("other");
        run(
            root,
            &[
                "worktree",
                "add",
                "--detach",
                worktree.to_str().unwrap(),
                "feature/ui",
            ],
        )?;
        let head = revision(&worktree);
        fs::write(worktree.join("common.txt"), "worktree staged")?;
        run(&worktree, &["add", "--", "common.txt"])?;
        let plan = Plan::read(&worktree)?;
        assert!(plan.checkout.branch.is_none());
        let result = plan.change(&worktree, "codex/worktree-task", true, &[])?;
        assert_eq!(
            result.checkout.branch.as_deref(),
            Some("codex/worktree-task")
        );
        assert_eq!(result.checkout.head, head);
        assert_eq!(run(&worktree, &["show", ":common.txt"])?, "worktree staged");
        assert_eq!(current_branch(root).as_deref(), Some("main"));
        Ok(())
    }
}
