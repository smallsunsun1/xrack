use crate::{
    dispatch, read_in_background,
    review::{git, git_root},
    worktree_include::IncludedFiles,
    worktrees::{apply_handoff_changes, prepare_handoff, tracked_patch},
};
use session::{Commands, ConversationActivity, Input, Snapshot};
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
};
use tokio::sync::watch;
use workbench_ui::AgentWorkbench;

#[derive(Clone, PartialEq, Eq)]
struct Scope {
    conversation: i64,
    workspace: String,
}

impl Scope {
    fn current(ui: &AgentWorkbench) -> Self {
        Self {
            conversation: ui.get_conversation_id(),
            workspace: ui.get_workspace(),
        }
    }
}

#[derive(PartialEq, Eq)]
struct Checkout {
    root: PathBuf,
    common: PathBuf,
    head: String,
}

impl Checkout {
    fn read(workspace: &Path) -> Result<Self, String> {
        let root = git_root(workspace)?
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let common = git_value(
            &root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        let common = Path::new(&common)
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let head = git_value(&root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
        Ok(Self { root, common, head })
    }
}

fn git_value(root: &Path, args: &[&str]) -> Result<String, String> {
    let (value, truncated) = git(root, args, 1024 * 1024)?;
    if truncated {
        return Err("Git 返回的工作树路径过长".into());
    }
    let value = String::from_utf8(value).map_err(|_| "Git 路径必须使用 UTF-8")?;
    Ok(value
        .strip_suffix("\r\n")
        .or_else(|| value.strip_suffix('\n'))
        .unwrap_or(&value)
        .into())
}

enum Destination {
    Existing { path: String, checkout: Checkout },
    New { base: PathBuf },
}

struct Plan {
    scope: Scope,
    original: Checkout,
    destination: Destination,
    patch: Result<Vec<u8>, String>,
    to_local: bool,
    prepared: Option<String>,
    included: usize,
}

impl Plan {
    fn read(
        scope: Scope,
        project: String,
        associated: Option<String>,
        base: Option<PathBuf>,
    ) -> Result<Self, String> {
        let original = Checkout::read(Path::new(&scope.workspace))?;
        let to_local = scope.workspace != project;
        let destination = if to_local {
            let checkout = Checkout::read(Path::new(&project))?;
            if checkout.common != original.common || checkout.root == original.root {
                return Err("本地项目目录与当前会话不属于同一仓库的两个工作树".into());
            }
            Destination::Existing {
                path: project,
                checkout,
            }
        } else {
            let existing = associated.and_then(|path| {
                let checkout = Checkout::read(Path::new(&path)).ok()?;
                (checkout.common == original.common && checkout.root != original.root)
                    .then_some(Destination::Existing { path, checkout })
            });
            match existing {
                Some(destination) => destination,
                None => Destination::New {
                    base: base.ok_or("工作树存储目录不可用")?,
                },
            }
        };
        let included = if matches!(destination, Destination::New { .. }) {
            IncludedFiles::read(&original.root)?.len()
        } else {
            0
        };
        let patch = tracked_patch(&original.root, &original.head);
        Ok(Self {
            scope,
            original,
            destination,
            patch,
            to_local,
            prepared: None,
            included,
        })
    }

    fn target(&self) -> Option<&str> {
        self.prepared.as_deref().or(match &self.destination {
            Destination::Existing { path, .. } => Some(path),
            Destination::New { .. } => None,
        })
    }

    fn run(&self, copy: bool, running: &[PathBuf]) -> Result<String, String> {
        let source = Path::new(&self.scope.workspace);
        if Checkout::read(source)? != self.original {
            return Err("源工作树或提交已变化，请重新读取".into());
        }
        let destination = match self.target() {
            Some(path) => {
                let current = Checkout::read(Path::new(path))?;
                if current.common != self.original.common || current.root == self.original.root {
                    return Err("目标目录已变化，请重新读取".into());
                }
                if let Destination::Existing { checkout, .. } = &self.destination
                    && current != *checkout
                {
                    return Err("目标工作树或提交已变化，请重新读取".into());
                }
                Some(current)
            }
            None => None,
        };
        if running.iter().any(|path| {
            git_root(path).ok().is_some_and(|root| {
                root == self.original.root
                    || destination
                        .as_ref()
                        .is_some_and(|target| root == target.root)
            })
        }) {
            return Err("源或目标工作树中有活跃会话，请待其空闲后交接".into());
        }
        let patch = if copy {
            let expected = self.patch.as_ref().map_err(Clone::clone)?;
            if tracked_patch(&self.original.root, &self.original.head)? != *expected {
                return Err("源改动已更新，请重新读取后再复制".into());
            }
            expected.as_slice()
        } else {
            &[]
        };
        if let Some(path) = &self.prepared {
            return Ok(path.clone());
        }
        match &self.destination {
            Destination::Existing { path, checkout } => {
                apply_handoff_changes(&checkout.root, patch)?;
                Ok(path.clone())
            }
            Destination::New { base } => {
                prepare_handoff(source, base, &self.original.root, &self.original.head, copy)
            }
        }
    }
}

type PendingPlan = Rc<RefCell<Option<Plan>>>;

fn read_plan(
    ui: &mut AgentWorkbench,
    snapshot: &Snapshot,
    base: Option<PathBuf>,
    pending: PendingPlan,
    generation: Rc<Cell<u64>>,
) {
    let scope = Scope::current(ui);
    let project = snapshot.project_workspace().to_owned();
    let associated = pending
        .borrow()
        .as_ref()
        .filter(|plan| plan.scope == scope && !plan.to_local)
        .and_then(|plan| plan.prepared.clone())
        .or_else(|| snapshot.view.associated_worktree.clone());
    let ticket = generation.get().wrapping_add(1);
    generation.set(ticket);
    pending.take();
    ui.set_handoff_loading(true);
    ui.set_handoff_preview_ready(false);
    ui.set_handoff_prepared(false);
    ui.set_handoff_error(String::new());
    ui.set_handoff_destination(String::new());
    ui.set_handoff_source_commit(String::new());
    ui.set_handoff_to_local(scope.workspace != project);
    ui.set_handoff_copy(false);
    ui.set_handoff_can_copy(false);
    ui.set_handoff_include_count(0);
    let weak = ui.as_weak();
    let expected = scope.clone();
    if let Err(error) = xrack_platform_winit::spawn_local(async move {
        let result = read_in_background("workbench-handoff-preview", move || {
            Plan::read(scope, project, associated, base)
        })
        .await;
        let Some(mut ui) = weak.upgrade() else { return };
        if generation.get() != ticket {
            return;
        }
        ui.set_handoff_loading(false);
        if expected != Scope::current(&ui) || !ui.get_handoff_open() {
            return;
        }
        match result {
            Ok(plan) => {
                ui.set_handoff_destination(plan.target().unwrap_or_default().into());
                ui.set_handoff_source_commit(plan.original.head.chars().take(12).collect());
                ui.set_handoff_can_copy(plan.patch.as_ref().is_ok_and(|bytes| !bytes.is_empty()));
                ui.set_handoff_include_count(plan.included as i64);
                if let Err(error) = &plan.patch {
                    ui.set_handoff_error(error.clone());
                }
                ui.set_handoff_preview_ready(true);
                pending.replace(Some(plan));
            }
            Err(error) => ui.set_handoff_error(error),
        }
    }) {
        ui.set_handoff_loading(false);
        ui.set_handoff_error(error.to_string());
    }
}

pub(super) fn attach(
    ui: &mut AgentWorkbench,
    snapshots: watch::Receiver<Snapshot>,
    commands: Commands,
    base: Option<PathBuf>,
) {
    let pending = Rc::new(RefCell::new(None::<Plan>));
    let generation = Rc::new(Cell::new(0_u64));
    let weak = ui.as_weak();
    let snapshot = snapshots.clone();
    let open_plan = pending.clone();
    let open_gen = generation.clone();
    let open_base = base.clone();
    ui.on_open_handoff(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        if !ui.get_can_open_handoff() {
            return;
        }
        let snapshot = snapshot.borrow();
        if snapshot.view.conversation != ui.get_conversation_id()
            || snapshot.view.workspace != ui.get_workspace()
        {
            return;
        }
        ui.set_handoff_open(true);
        if open_plan
            .borrow()
            .as_ref()
            .is_some_and(|plan| plan.scope == Scope::current(&ui) && plan.prepared.is_some())
        {
            return;
        }
        read_plan(
            &mut ui,
            &snapshot,
            open_base.clone(),
            open_plan.clone(),
            open_gen.clone(),
        );
    });

    let weak = ui.as_weak();
    let snapshot = snapshots.clone();
    let reload_plan = pending.clone();
    ui.on_reload_handoff(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        if !ui.get_handoff_open()
            || ui.get_handoff_loading()
            || ui.get_handoff_preparing()
            || ui.get_handoff_pending()
        {
            return;
        }
        let snapshot = snapshot.borrow();
        if snapshot.view.conversation == ui.get_conversation_id()
            && snapshot.view.workspace == ui.get_workspace()
        {
            read_plan(
                &mut ui,
                &snapshot,
                base.clone(),
                reload_plan.clone(),
                generation.clone(),
            );
        }
    });

    let weak = ui.as_weak();
    ui.on_confirm_handoff(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        if !ui.get_handoff_open() || !ui.get_can_confirm_handoff() {
            return;
        }
        let Some(plan) = pending
            .take()
            .filter(|plan| plan.scope == Scope::current(&ui))
        else {
            return;
        };
        let scope = plan.scope.clone();
        let copy = ui.get_handoff_copy();
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
        ui.set_handoff_preparing(true);
        ui.set_handoff_error(String::new());
        let weak_result = weak.clone();
        let retry = pending.clone();
        let commands = commands.clone();
        if let Err(error) = xrack_platform_winit::spawn_local(async move {
            let result = read_in_background("workbench-handoff", move || {
                let prepared = plan.run(copy, &running);
                Ok((plan, prepared))
            })
            .await;
            let Some(mut ui) = weak_result.upgrade() else {
                return;
            };
            ui.set_handoff_preparing(false);
            if scope != Scope::current(&ui) {
                return;
            }
            match result {
                Ok((mut plan, Ok(path))) => {
                    let worktree = if plan.to_local {
                        scope.workspace.clone()
                    } else {
                        path.clone()
                    };
                    plan.prepared = Some(path.clone());
                    retry.replace(Some(plan));
                    ui.set_handoff_prepared(true);
                    ui.set_handoff_destination(path.clone());
                    let input = Input::Handoff {
                        conversation: scope.conversation,
                        original: scope.workspace,
                        workspace: path,
                        worktree,
                    };
                    if dispatch(&mut ui, &commands, input) {
                        ui.set_handoff_pending(true);
                    } else {
                        ui.set_handoff_error(ui.get_status());
                    }
                }
                Ok((plan, Err(error))) => {
                    retry.replace(Some(plan));
                    ui.set_handoff_error(error);
                }
                Err(error) => ui.set_handoff_error(error),
            }
        }) {
            ui.set_handoff_preparing(false);
            ui.set_handoff_error(error.to_string());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::git_command;
    use std::fs;

    fn fixture() -> Result<(tempfile::TempDir, String, String), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let root = directory.path().canonicalize()?;
        let local = root.join("project");
        fs::create_dir(&local)?;
        let run = |args: &[&str]| -> Result<(), String> {
            let output = git_command(&local)
                .args(args)
                .output()
                .map_err(|error| error.to_string())?;
            if output.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).into_owned())
            }
        };
        run(&["init", "-q", "-b", "main"])?;
        let hooks = root.join("empty-hooks");
        fs::create_dir(&hooks)?;
        run(&[
            "config",
            "--local",
            "core.hooksPath",
            hooks.to_str().unwrap(),
        ])?;
        fs::write(local.join("tracked.txt"), "base\n")?;
        fs::write(local.join(".gitignore"), "fixture.setup\n")?;
        fs::write(local.join(".worktreeinclude"), "/fixture.setup\n")?;
        fs::write(local.join("fixture.setup"), "opted-in setup")?;
        run(&["add", "--", "tracked.txt", ".gitignore", ".worktreeinclude"])?;
        run(&[
            "-c",
            "user.name=XRack fixture",
            "-c",
            "user.email=xrack@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
            "Fixture",
        ])?;
        let local = local.into_os_string().into_string().unwrap();
        let new = Plan::read(
            Scope {
                conversation: 7,
                workspace: local.clone(),
            },
            local.clone(),
            None,
            Some(root.join("worktrees")),
        )?;
        let isolated = new.run(false, &[])?;
        assert_eq!(new.included, 1);
        assert_eq!(
            fs::read_to_string(Path::new(&isolated).join("fixture.setup"))?,
            "opted-in setup"
        );
        Ok((directory, local, isolated))
    }

    #[test]
    fn returns_to_local_and_reuses_the_associated_worktree()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_fixture, local, isolated) = fixture()?;
        let source = Path::new(&isolated).join("tracked.txt");
        let target = Path::new(&local).join("tracked.txt");
        fs::write(&source, "worktree draft\n")?;
        fs::write(Path::new(&isolated).join("untracked.txt"), "temporary")?;
        let handoff = Plan::read(
            Scope {
                conversation: 7,
                workspace: isolated.clone(),
            },
            local.clone(),
            None,
            None,
        )?;
        assert!(handoff.to_local);
        assert_eq!(handoff.target(), Some(local.as_str()));
        assert_eq!(handoff.run(true, &[])?, local);
        assert_eq!(fs::read_to_string(&source)?, "worktree draft\n");
        assert_eq!(fs::read_to_string(&target)?, "worktree draft\n");
        assert_eq!(handoff.run(true, &[])?, local);
        assert!(!Path::new(&local).join("untracked.txt").exists());

        let restore = Plan::read(
            Scope {
                conversation: 7,
                workspace: local,
            },
            handoff.target().unwrap().into(),
            Some(isolated.clone()),
            None,
        )?;
        assert!(!restore.to_local);
        assert_eq!(restore.run(false, &[])?, isolated);
        Ok(())
    }

    #[test]
    fn checks_conflicts_and_rejects_a_busy_target_or_a_stale_source()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_fixture, local, isolated) = fixture()?;
        let source = Path::new(&isolated).join("tracked.txt");
        let target = Path::new(&local).join("tracked.txt");
        fs::write(&source, "isolated edit\n")?;
        fs::write(&target, "local edit\n")?;
        let plan = Plan::read(
            Scope {
                conversation: 7,
                workspace: isolated,
            },
            local.clone(),
            None,
            None,
        )?;
        assert!(plan.run(true, &[]).unwrap_err().contains("冲突"));
        assert!(
            plan.run(false, &[PathBuf::from(&local)])
                .unwrap_err()
                .contains("活跃会话")
        );
        fs::write(&source, "new isolated edit\n")?;
        assert!(plan.run(true, &[]).unwrap_err().contains("源改动已更新"));
        assert_eq!(fs::read_to_string(target)?, "local edit\n");
        assert_eq!(plan.run(false, &[])?, local);
        Ok(())
    }
}
