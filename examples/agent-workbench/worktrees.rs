use crate::{
    environment::{self, SetupFailure, SetupSelection},
    read_in_background,
    review::{git, git_command, git_root, worktree_branches},
    worktree_include::IncludedFiles,
};
use session::{Commands, Input, Prompt};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::oneshot;
use workbench_ui::AgentWorkbench;

const PATCH_LIMIT: usize = 24 * 1024 * 1024;

pub struct ForkPreparation {
    pub title: String,
    pub last_turn: String,
    pub copy_changes: bool,
}

pub fn prepare_fork(
    ui: &mut AgentWorkbench,
    base: PathBuf,
    commands: Commands,
    preparation: ForkPreparation,
) {
    let workspace = ui.get_workspace();
    let folder = PathBuf::from(&workspace);
    let conversation = ui.get_conversation_id();
    let (sender, receiver) = oneshot::channel();
    let ForkPreparation {
        title,
        last_turn,
        copy_changes,
    } = preparation;
    ui.set_worktree_preparing(true);
    ui.set_worktree_error(String::new());
    let weak = ui.as_weak();
    if let Err(error) = xrack_platform_winit::spawn_local(async move {
        let task = thread::Builder::new()
            .name("workbench-worktree".into())
            .spawn(move || {
                let _ = sender.send(prepare(&folder, &base, copy_changes));
            });
        let result = match task {
            Ok(_) => receiver
                .await
                .unwrap_or_else(|_| Err("工作树任务已退出".into())),
            Err(error) => Err(format!("无法启动工作树任务（{error}）")),
        };
        let Some(mut ui) = weak.upgrade() else { return };
        ui.set_worktree_preparing(false);
        match result {
            Ok(path) => {
                if ui.get_fork_open()
                    && ui.get_conversation_id() == conversation
                    && ui.get_workspace() == workspace
                    && !ui.get_busy()
                    && ui.get_connected()
                {
                    let input = Input::Fork {
                        title,
                        last_turn,
                        worktree: Some(path.clone()),
                    };
                    if crate::dispatch(&mut ui, &commands, input) {
                        ui.set_fork_pending(true);
                        return;
                    }
                }
                ui.set_worktree_error(format!("工作树已在 {path} 创建，可从该目录继续"));
            }
            Err(error) => ui.set_worktree_error(error),
        }
    }) {
        ui.set_worktree_preparing(false);
        ui.set_worktree_error(error.to_string());
    }
}

pub fn prepare(workspace: &Path, base: &Path, copy_changes: bool) -> Result<String, String> {
    prepare_from(workspace, base, None, copy_changes)
}

pub(super) fn prepare_handoff(
    workspace: &Path,
    base: &Path,
    root: &Path,
    head: &str,
    copy: bool,
) -> Result<String, String> {
    prepare_from(
        workspace,
        base,
        Some((
            root,
            &Start {
                name: "HEAD".into(),
                commit: head.into(),
            },
        )),
        copy,
    )
}

#[derive(Clone, PartialEq, Eq)]
struct Start {
    name: String,
    commit: String,
}

fn commit(repository: &Path, reference: &str) -> Result<String, String> {
    let reference = format!("{reference}^{{commit}}");
    let (oid, truncated) = git(
        repository,
        &["rev-parse", "--verify", "--end-of-options", &reference],
        128,
    )?;
    let oid = std::str::from_utf8(&oid)
        .map_err(|error| error.to_string())?
        .trim();
    if truncated
        || !matches!(oid.len(), 40 | 64)
        || !oid.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("无法读取工作树的起始提交".into());
    }
    Ok(oid.into())
}

fn prepare_from(
    workspace: &Path,
    base: &Path,
    selected: Option<(&Path, &Start)>,
    copy_changes: bool,
) -> Result<String, String> {
    let original = workspace
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let repository = git_root(&original)
        .map_err(|error| format!("独立工作树需要 Git 仓库（{error}）"))?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let relative = original
        .strip_prefix(&repository)
        .map_err(|_| "无法定位工作目录相对路径")?;
    if base.starts_with(&repository) {
        return Err("请使用原仓库之外的工作树存储目录".into());
    }
    let head = commit(&repository, "HEAD");
    let oid = if let Some((expected_repository, start)) = selected {
        let reference = if start.name == "HEAD" {
            "HEAD".into()
        } else {
            format!("refs/heads/{}", start.name)
        };
        if expected_repository != repository
            || commit(&repository, &reference).ok().as_deref() != Some(start.commit.as_str())
        {
            return Err("起始分支已变化，请重新选择运行位置".into());
        }
        start.commit.as_str()
    } else {
        head.as_deref().map_err(Clone::clone)?
    };

    let patch = if copy_changes {
        if head.as_deref().ok() != Some(oid) {
            return Err("带上改动时，起点需要与当前工作目录的 HEAD 一致".into());
        }
        tracked_patch(&repository, oid)?
    } else {
        Vec::new()
    };

    let included = IncludedFiles::read(&repository)?;
    fs::create_dir_all(base).map_err(|error| error.to_string())?;
    let base = base.canonicalize().map_err(|error| error.to_string())?;
    if base.starts_with(&repository) {
        return Err("请使用原仓库之外的工作树存储目录".into());
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let serial = NEXT.fetch_add(1, Ordering::Relaxed);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let destination = base.join(format!("worktree-{stamp}-{}-{serial}", std::process::id()));
    let path = destination.to_str().ok_or("工作树路径必须为 UTF-8")?;
    git(
        &repository,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "worktree",
            "add",
            "--detach",
            "--checkout",
            "--",
            path,
            oid,
        ],
        16 * 1024,
    )
    .map_err(|error| format!("创建工作树失败（{error}）"))?;
    if !patch.is_empty() {
        apply_changes(&destination, &patch)
            .map_err(|error| format!("工作树已在 {path} 创建，复制改动失败（{error}）"))?;
    }
    included
        .copy_into(&repository, &destination)
        .map_err(|error| format!("工作树已在 {path} 创建，复制本地配置失败（{error}）"))?;
    let selected = destination
        .join(relative)
        .canonicalize()
        .ok()
        .filter(|folder| folder.is_dir() && folder.starts_with(&destination))
        .unwrap_or(destination);
    selected
        .into_os_string()
        .into_string()
        .map_err(|_| "工作树路径必须为 UTF-8".into())
}

pub(super) fn tracked_patch(repository: &Path, oid: &str) -> Result<Vec<u8>, String> {
    let (patch, truncated) = git(
        repository,
        &[
            "diff",
            "--binary",
            "--full-index",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--ignore-submodules=dirty",
            oid,
            "--",
        ],
        PATCH_LIMIT,
    )?;
    if truncated {
        return Err("已跟踪改动超过 24 MiB".into());
    }
    Ok(patch)
}

fn apply_changes(destination: &Path, patch: &[u8]) -> Result<(), String> {
    git_apply(destination, &[], patch)
}

pub(super) fn apply_handoff_changes(destination: &Path, patch: &[u8]) -> Result<(), String> {
    if patch.is_empty() {
        return Ok(());
    }
    if git_apply(destination, &["--check"], patch).is_ok() {
        return apply_changes(destination, patch);
    }
    if git_apply(destination, &["--check", "--reverse"], patch).is_ok() {
        return Ok(());
    }
    Err("目标工作目录有冲突，无法复制改动".into())
}

fn git_apply(destination: &Path, flags: &[&str], patch: &[u8]) -> Result<(), String> {
    let mut child = git_command(destination)
        .args(["apply", "--binary"])
        .args(flags)
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let written = child
        .stdin
        .take()
        .ok_or_else(|| "无法打开 Git 输入".to_owned())
        .and_then(|mut stdin| stdin.write_all(patch).map_err(|error| error.to_string()));
    let status = child.wait().map_err(|error| error.to_string())?;
    if status.success() {
        written
    } else {
        Err(format!("Git apply 执行失败（{status}）"))
    }
}

#[derive(Clone, PartialEq, Eq)]
struct DraftScope {
    conversation: i64,
    workspace: String,
}

impl DraftScope {
    fn current(ui: &AgentWorkbench) -> Self {
        Self {
            conversation: ui.get_conversation_id(),
            workspace: ui.get_workspace(),
        }
    }
}

struct Catalog {
    repository: PathBuf,
    head: Option<String>,
    starts: Vec<Start>,
    included: usize,
    setups: Vec<SetupSelection>,
    environment_error: String,
}

impl Catalog {
    fn read(workspace: &Path) -> Result<Self, String> {
        let repository = git_root(workspace)?
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let head = commit(&repository, "HEAD").ok();
        let mut starts = head
            .iter()
            .cloned()
            .map(|commit| Start {
                name: "HEAD".into(),
                commit,
            })
            .collect::<Vec<_>>();
        starts.extend(
            worktree_branches(&repository)?
                .into_iter()
                .map(|(name, commit)| Start { name, commit }),
        );
        if starts.is_empty() {
            return Err("Git 仓库还没有可作为工作树起点的提交".into());
        }
        let included = IncludedFiles::read(&repository)?.len();
        let (setups, environment_error) = match environment::profiles(&repository) {
            Ok(profiles) => (
                profiles
                    .into_iter()
                    .filter_map(|profile| profile.setup)
                    .collect(),
                String::new(),
            ),
            Err(error) => (Vec::new(), error),
        };
        Ok(Self {
            repository,
            head,
            starts,
            included,
            setups,
            environment_error,
        })
    }

    fn start(&self, name: &str) -> Option<&Start> {
        self.starts.iter().find(|start| start.name == name)
    }

    fn publish_environment(&self, ui: &mut AgentWorkbench) {
        let selected = ui.get_worktree_start_environment();
        let script = self
            .setups
            .iter()
            .find(|setup| setup.label == selected)
            .map_or("", |setup| setup.script.as_str());
        ui.set_worktree_start_script(script.into());
    }

    fn publish_choice(&self, ui: &mut AgentWorkbench) {
        let selected = ui.get_worktree_start_base();
        let start = self.start(&selected);
        let can_copy =
            start.is_some_and(|start| self.head.as_deref() == Some(start.commit.as_str()));
        ui.set_worktree_start_commit(
            start.map_or_else(String::new, |start| start.commit.chars().take(12).collect()),
        );
        ui.set_worktree_start_can_copy(can_copy);
        if !can_copy {
            ui.set_worktree_start_copy(false);
        }
    }
}

#[derive(Clone)]
struct Configuration {
    scope: DraftScope,
    repository: PathBuf,
    start: Start,
    copy: bool,
    setup: Option<SetupSelection>,
    prepared: Option<Prepared>,
    setup_output: String,
}

#[derive(Clone)]
struct Prepared {
    path: String,
    setup_done: bool,
}

struct Attempt {
    prepared: Prepared,
    failure: Option<SetupFailure>,
}

fn prepare_config(
    configuration: Configuration,
    source: &Path,
    base: &Path,
) -> Result<Attempt, String> {
    let existing = configuration
        .prepared
        .filter(|prepared| Path::new(&prepared.path).is_dir());
    if let Some(prepared) = existing.as_ref().filter(|prepared| prepared.setup_done) {
        return Ok(Attempt {
            prepared: prepared.clone(),
            failure: None,
        });
    }
    if let Some(setup) = &configuration.setup
        && !environment::profiles(&configuration.repository)?
            .iter()
            .any(|profile| profile.setup.as_ref() == Some(setup))
    {
        return Err("初始化配置已变化，请重新查看并选择环境".into());
    }
    let path = if let Some(prepared) = existing {
        prepared.path
    } else {
        prepare_from(
            source,
            base,
            Some((&configuration.repository, &configuration.start)),
            configuration.copy,
        )?
    };
    let failure = if let Some(setup) = &configuration.setup {
        match git_root(Path::new(&path)) {
            Ok(root) => environment::run_setup(setup, &configuration.repository, &root).err(),
            Err(message) => Some(SetupFailure {
                message,
                output: String::new(),
            }),
        }
    } else {
        None
    };
    Ok(Attempt {
        prepared: Prepared {
            path,
            setup_done: failure.is_none(),
        },
        failure,
    })
}

#[derive(Clone)]
pub(super) struct ComposerWorktrees {
    base: Option<PathBuf>,
    selected: Rc<RefCell<HashMap<i64, Configuration>>>,
}

impl ComposerWorktrees {
    pub(super) fn attach(ui: &mut AgentWorkbench, base: Option<PathBuf>) -> Self {
        let controller = Self {
            base,
            selected: Rc::new(RefCell::new(HashMap::new())),
        };
        let catalog = Rc::new(RefCell::new(None::<(DraftScope, Catalog)>));
        let generation = Rc::new(Cell::new(0_u64));
        let weak = ui.as_weak();
        let choices = catalog.clone();
        let settings = controller.clone();
        ui.on_open_worktree_start(move || {
            let Some(mut ui) = weak.upgrade() else { return };
            if !ui.get_can_configure_worktree_start() || ui.get_worktree_preparing() {
                return;
            }
            let scope = DraftScope::current(&ui);
            let previously = settings
                .selected
                .borrow()
                .get(&scope.conversation)
                .filter(|configuration| configuration.scope == scope)
                .map(|configuration| {
                    (
                        configuration.start.name.clone(),
                        configuration.copy,
                        configuration.setup.clone(),
                    )
                });
            ui.set_worktree_start_open(true);
            ui.set_worktree_start_loading(true);
            ui.set_worktree_start_error(String::new());
            ui.set_worktree_start_commit(String::new());
            ui.set_worktree_start_choices(Vec::new());
            ui.set_worktree_start_environments(Vec::new());
            ui.set_worktree_start_environment(String::new());
            ui.set_worktree_start_script(String::new());
            ui.set_worktree_start_can_copy(false);
            ui.set_worktree_start_include_count(0);
            choices.take();
            let ticket = generation.get().wrapping_add(1);
            generation.set(ticket);
            let generation = generation.clone();
            let choices = choices.clone();
            let weak = weak.clone();
            let path = PathBuf::from(&scope.workspace);
            if let Err(error) = xrack_platform_winit::spawn_local(async move {
                let result =
                    read_in_background("workbench-worktree-bases", move || Catalog::read(&path))
                        .await;
                let Some(mut ui) = weak.upgrade() else { return };
                if generation.get() != ticket {
                    return;
                }
                ui.set_worktree_start_loading(false);
                if scope != DraftScope::current(&ui) || !ui.get_worktree_start_open() {
                    return;
                }
                match result {
                    Ok(value) => {
                        let (selected, copy, setup) = previously
                            .filter(|(name, _, _)| value.start(name).is_some())
                            .unwrap_or_else(|| {
                                (
                                    value
                                        .starts
                                        .first()
                                        .map_or_else(String::new, |start| start.name.clone()),
                                    false,
                                    None,
                                )
                            });
                        ui.set_worktree_start_choices(
                            value
                                .starts
                                .iter()
                                .map(|start| start.name.clone())
                                .collect(),
                        );
                        ui.set_worktree_start_base(selected);
                        ui.set_worktree_start_copy(copy);
                        value.publish_choice(&mut ui);
                        ui.set_worktree_start_include_count(value.included as i64);
                        ui.set_worktree_start_environments(
                            value
                                .setups
                                .iter()
                                .map(|setup| setup.label.clone())
                                .collect(),
                        );
                        if let Some(setup) = setup
                            && value.setups.contains(&setup)
                        {
                            ui.set_worktree_start_environment(setup.label);
                        }
                        value.publish_environment(&mut ui);
                        ui.set_worktree_start_error(value.environment_error.clone());
                        choices.replace(Some((scope, value)));
                    }
                    Err(error) => ui.set_worktree_start_error(error),
                }
            }) {
                ui.set_worktree_start_loading(false);
                ui.set_worktree_start_error(error.to_string());
            }
        });

        let weak = ui.as_weak();
        let choices = catalog.clone();
        ui.on_select_worktree_start_base(move || {
            if let Some(mut ui) = weak.upgrade()
                && let Some((scope, value)) = choices.borrow().as_ref()
                && *scope == DraftScope::current(&ui)
            {
                value.publish_choice(&mut ui);
            }
        });
        let weak = ui.as_weak();
        let choices = catalog.clone();
        ui.on_select_worktree_start_environment(move || {
            if let Some(mut ui) = weak.upgrade()
                && let Some((scope, value)) = choices.borrow().as_ref()
                && *scope == DraftScope::current(&ui)
            {
                value.publish_environment(&mut ui);
            }
        });
        let weak = ui.as_weak();
        let settings = controller.clone();
        ui.on_configure_worktree_start(move |isolated| {
            let Some(mut ui) = weak.upgrade() else { return };
            if !ui.get_worktree_start_open() {
                return;
            }
            let scope = DraftScope::current(&ui);
            if isolated {
                let value = catalog.borrow();
                let Some((owner, choice)) = value.as_ref().filter(|(owner, _)| *owner == scope)
                else {
                    return;
                };
                let Some(start) = choice.start(&ui.get_worktree_start_base()) else {
                    return;
                };
                let copy = choice.head.as_deref() == Some(start.commit.as_str())
                    && ui.get_worktree_start_copy();
                let environment = ui.get_worktree_start_environment();
                let setup = choice
                    .setups
                    .iter()
                    .find(|setup| setup.label == environment)
                    .cloned();
                let mut selected = settings.selected.borrow_mut();
                let (prepared, setup_output) = selected
                    .get(&owner.conversation)
                    .filter(|previous| {
                        previous.scope == *owner
                            && previous.start == *start
                            && previous.copy == copy
                            && previous.setup == setup
                    })
                    .map(|previous| (previous.prepared.clone(), previous.setup_output.clone()))
                    .unwrap_or_default();
                selected.insert(
                    owner.conversation,
                    Configuration {
                        scope,
                        repository: choice.repository.clone(),
                        start: start.clone(),
                        copy,
                        setup,
                        prepared,
                        setup_output,
                    },
                );
            } else {
                settings.selected.borrow_mut().remove(&scope.conversation);
            }
            ui.set_worktree_start_open(false);
            ui.set_worktree_start_error(String::new());
            settings.sync(&mut ui);
            ui.invoke_focus_composer();
        });
        controller
    }

    pub(super) fn sync(&self, ui: &mut AgentWorkbench) {
        let scope = DraftScope::current(ui);
        let mut selected = self.selected.borrow_mut();
        if selected
            .get(&scope.conversation)
            .is_some_and(|configuration| configuration.scope != scope || ui.get_renamable())
        {
            selected.remove(&scope.conversation);
        }
        let (label, output) = selected
            .get(&scope.conversation)
            .map_or(("", ""), |configuration| {
                (
                    configuration.start.name.as_str(),
                    configuration.setup_output.as_str(),
                )
            });
        if ui.get_worktree_start_label() != label {
            ui.set_worktree_start_label(label.into());
        }
        if ui.get_worktree_setup_output() != output {
            ui.set_worktree_setup_output(output.into());
        }
    }

    pub(super) fn send(
        &self,
        ui: &mut AgentWorkbench,
        commands: Commands,
        prompt: Prompt,
        text: String,
    ) {
        let scope = DraftScope::current(ui);
        let selection = self
            .selected
            .borrow()
            .get(&scope.conversation)
            .filter(|configuration| configuration.scope == scope)
            .cloned();
        let (Some(configuration), Some(base)) = (selection, self.base.clone()) else {
            ui.set_worktree_start_error("请重新选择新会话的运行位置".into());
            return;
        };
        if ui.get_worktree_preparing() {
            return;
        }
        ui.set_worktree_preparing(true);
        ui.set_worktree_start_error(String::new());
        let weak = ui.as_weak();
        let selected = self.selected.clone();
        let source = PathBuf::from(&scope.workspace);
        if let Err(error) = xrack_platform_winit::spawn_local(async move {
            let result = read_in_background("workbench-new-worktree", move || {
                prepare_config(configuration, &source, &base)
            })
            .await;
            let Some(mut ui) = weak.upgrade() else { return };
            ui.set_worktree_preparing(false);
            if let Ok(attempt) = &result
                && let Some(config) = selected.borrow_mut().get_mut(&scope.conversation)
                && config.scope == scope
            {
                config.prepared = Some(attempt.prepared.clone());
                config.setup_output = attempt
                    .failure
                    .as_ref()
                    .map_or_else(String::new, |failure| failure.output.clone());
            }
            if scope != DraftScope::current(&ui) {
                return;
            }
            match result {
                Ok(Attempt {
                    failure: Some(failure),
                    ..
                }) => {
                    ui.set_worktree_setup_output(failure.output);
                    ui.set_worktree_start_error(failure.message);
                }
                Ok(Attempt {
                    prepared,
                    failure: None,
                }) if !ui.get_busy() && ui.get_connected() && !ui.get_renamable() => {
                    ui.set_worktree_setup_output(String::new());
                    let workspace = prepared.path;
                    let command = Input::SendInWorktree {
                        conversation: scope.conversation,
                        original: scope.workspace,
                        workspace,
                        prompt,
                    };
                    if crate::dispatch(&mut ui, &commands, command) {
                        ui.set_busy(true);
                        crate::show_status(&mut ui, session::Status::AwaitingReply);
                        ui.set_submitted_draft(text);
                        ui.set_sending_prompt(true);
                    }
                }
                Ok(attempt) => ui.set_worktree_start_error(format!(
                    "工作树已在 {} 创建，可在连接后重试发送",
                    attempt.prepared.path
                )),
                Err(error) => ui.set_worktree_start_error(error),
            }
        }) {
            ui.set_worktree_preparing(false);
            ui.set_worktree_start_error(error.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
        Ok(git(root, args, 4096)?.0)
    }

    #[test]
    fn opted_in_setup_reaches_a_new_worktree_without_replacing_a_tracked_destination()
    -> Result<(), Box<dyn std::error::Error>> {
        let scratch = tempfile::tempdir()?;
        let repo = scratch.path().join("source");
        let storage = scratch.path().join("worktrees");
        fs::create_dir(&repo)?;
        run(&repo, &["init", "-q", "-b", "main"])?;
        let author = [
            "-c",
            "user.name=XRack",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
        ];
        fs::write(
            repo.join(".gitignore"),
            ".env.local\nsetup.local\nAGENTS.override.md\n",
        )?;
        fs::write(repo.join(".worktreeinclude"), "/.env.local\n/setup.local\n")?;
        run(&repo, &["add", "--", ".gitignore", ".worktreeinclude"])?;
        run(&repo, &[&author[..], &["Configuration rules"]].concat())?;
        run(&repo, &["switch", "-c", "feature/setup"])?;
        fs::write(repo.join(".env.local"), "tracked branch content")?;
        run(&repo, &["add", "-f", "--", ".env.local"])?;
        run(
            &repo,
            &[&author[..], &["Tracked branch placeholder"]].concat(),
        )?;
        run(&repo, &["switch", "main"])?;
        fs::write(repo.join(".env.local"), "local fixture setting")?;
        fs::write(repo.join("setup.local"), "ordinary local setup")?;
        fs::write(repo.join("AGENTS.override.md"), "local instructions")?;
        let catalog = Catalog::read(&repo)?;
        assert_eq!(catalog.included, 3);
        let start = catalog.start("feature/setup").unwrap();
        let destination = prepare_from(&repo, &storage, Some((&catalog.repository, start)), false)?;
        let destination = Path::new(&destination);
        assert_eq!(
            fs::read_to_string(destination.join(".env.local"))?,
            "tracked branch content"
        );
        assert_eq!(
            fs::read_to_string(destination.join("setup.local"))?,
            "ordinary local setup"
        );
        assert_eq!(
            fs::read_to_string(destination.join("AGENTS.override.md"))?,
            "local instructions"
        );
        assert_eq!(
            fs::read_to_string(repo.join(".env.local"))?,
            "local fixture setting"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn retries_the_approved_setup_in_the_same_worktree() -> Result<(), Box<dyn std::error::Error>> {
        let scratch = tempfile::tempdir()?;
        let repo = scratch.path().join("source");
        let storage = scratch.path().join("worktrees");
        let environments = repo.join(".codex/environments");
        fs::create_dir_all(&environments)?;
        run(&repo, &["init", "-qb", "main"])?;
        run(
            &repo,
            &[
                "-c",
                "user.name=XRack",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "--allow-empty",
                "-qm",
                "Worktree setup fixture",
            ],
        )?;
        let file = environments.join("environment.toml");
        let manifest = r#"
version = 1
name = "Fixture"
[setup]
script = '''
if [ ! -f .setup-attempt ]; then
    printf '%s\n' "$CODEX_SOURCE_TREE_PATH" "$CODEX_WORKTREE_PATH" > .setup-attempt
    printf 'fixture setup failed\n' >&2
    exit 7
fi
printf 'retried' > .setup-result
'''
"#;
        fs::write(&file, manifest)?;
        let catalog = Catalog::read(&repo)?;
        let setup = catalog
            .setups
            .first()
            .cloned()
            .ok_or("missing fixture environment")?;
        let mut configuration = Configuration {
            scope: DraftScope {
                conversation: 1,
                workspace: repo.to_string_lossy().into_owned(),
            },
            repository: catalog.repository.clone(),
            start: catalog.start("HEAD").cloned().ok_or("missing HEAD")?,
            copy: false,
            setup: Some(setup),
            prepared: None,
            setup_output: String::new(),
        };
        let first = prepare_config(configuration.clone(), &repo, &storage)?;
        let failure = first
            .failure
            .ok_or("fixture setup should fail on the first attempt")?;
        assert!(failure.message.contains('7'));
        assert_eq!(failure.output.trim(), "fixture setup failed");
        let destination = PathBuf::from(&first.prepared.path);
        let written = fs::read_to_string(destination.join(".setup-attempt"))?;
        assert_eq!(
            written.lines().collect::<Vec<_>>(),
            [
                catalog
                    .repository
                    .to_str()
                    .ok_or("fixture path is not UTF-8")?,
                destination.to_str().ok_or("fixture path is not UTF-8")?,
            ]
        );
        configuration.prepared = Some(first.prepared);
        fs::write(&file, manifest.replace("retried", "changed"))?;
        assert!(prepare_config(configuration.clone(), &repo, &storage).is_err());
        assert!(!destination.join(".setup-result").exists());
        fs::write(&file, manifest)?;
        let second = prepare_config(configuration, &repo, &storage)?;
        assert!(second.failure.is_none());
        assert!(second.prepared.setup_done);
        assert_eq!(Path::new(&second.prepared.path), destination);
        assert_eq!(
            fs::read_to_string(destination.join(".setup-result"))?,
            "retried"
        );
        Ok(())
    }

    #[test]
    fn selected_start_uses_the_branch_commit_without_checkout_or_copying_unrelated_changes()
    -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let workspace = temporary.path().join("source");
        let storage = temporary.path().join("worktrees");
        fs::create_dir_all(&workspace)?;
        run(&workspace, &["init", "-q", "-b", "main"])?;
        let commit_args = [
            "-c",
            "user.name=XRack",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-qm",
        ];
        fs::write(workspace.join("source.txt"), "main")?;
        run(&workspace, &["add", "--", "source.txt"])?;
        run(&workspace, &[&commit_args[..], &["Base"]].concat())?;
        let main = commit(&workspace, "HEAD")?;
        run(&workspace, &["switch", "-c", "feature/ui"])?;
        fs::write(workspace.join("source.txt"), "feature")?;
        run(&workspace, &["add", "--", "source.txt"])?;
        run(&workspace, &[&commit_args[..], &["Feature"]].concat())?;
        let feature = commit(&workspace, "HEAD")?;
        run(&workspace, &["switch", "main"])?;
        fs::write(workspace.join("source.txt"), "local draft")?;

        let catalog = Catalog::read(&workspace)?;
        let start = catalog.start("feature/ui").unwrap();
        assert_eq!(start.commit, feature);
        let isolated = prepare_from(
            &workspace,
            &storage,
            Some((&catalog.repository, start)),
            false,
        )?;
        let isolated = Path::new(&isolated);
        assert_eq!(commit(isolated, "HEAD")?, feature);
        assert_eq!(fs::read_to_string(isolated.join("source.txt"))?, "feature");
        assert!(run(isolated, &["symbolic-ref", "--quiet", "HEAD"]).is_err());
        assert_eq!(
            fs::read_to_string(workspace.join("source.txt"))?,
            "local draft"
        );
        assert_eq!(commit(&workspace, "HEAD")?, main);
        assert!(
            prepare_from(
                &workspace,
                &storage,
                Some((&catalog.repository, start)),
                true
            )
            .is_err()
        );

        run(&workspace, &["update-ref", "refs/heads/feature/ui", &main])?;
        assert!(
            prepare_from(
                &workspace,
                &storage,
                Some((&catalog.repository, start)),
                false
            )
            .unwrap_err()
            .contains("起始分支已变化")
        );
        assert_eq!(fs::read_dir(&storage)?.count(), 1);
        run(&workspace, &["restore", "--", "source.txt"])?;
        run(&workspace, &["switch", "--orphan", "unborn"])?;
        let unborn = Catalog::read(&workspace)?;
        assert!(unborn.head.is_none());
        let existing = unborn.start("main").unwrap();
        let detached = prepare_from(
            &workspace,
            &storage,
            Some((&unborn.repository, existing)),
            false,
        )?;
        assert_eq!(commit(Path::new(&detached), "HEAD")?, main);
        assert!(commit(&workspace, "HEAD").is_err());
        Ok(())
    }

    #[test]
    fn detached_worktrees_use_head_and_optionally_copy_tracked_changes()
    -> Result<(), Box<dyn std::error::Error>> {
        let scratch = tempfile::tempdir()?;
        let repo = scratch.path().join("demo");
        let source = repo.join("src");
        let storage = scratch.path().join("worktrees");
        fs::create_dir_all(&source)?;
        run(&repo, &["init", "-q"])?;
        fs::write(source.join("counter.txt"), "committed\n")?;
        fs::write(source.join("data.bin"), [0, 1, 2, 3])?;
        run(&repo, &["add", "--", "src"])?;
        run(
            &repo,
            &[
                "-c",
                "user.name=XRack",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "fixture",
            ],
        )?;
        fs::write(source.join("counter.txt"), "staged\n")?;
        run(&repo, &["add", "--", "src/counter.txt"])?;
        fs::write(source.join("counter.txt"), "working\n")?;
        fs::write(source.join("data.bin"), [0, 8, 9, 255])?;
        fs::write(source.join("untracked.txt"), "local only")?;
        let status = run(&repo, &["status", "--porcelain=v1", "-z"])?;
        let head = run(&repo, &["rev-parse", "HEAD"])?;

        let clean = prepare(&source, &storage, false)?;
        let clean = Path::new(&clean);
        assert_eq!(
            clean.file_name().and_then(|name| name.to_str()),
            Some("src")
        );
        assert_eq!(
            fs::read_to_string(clean.join("counter.txt"))?,
            "committed\n"
        );
        assert_eq!(fs::read(clean.join("data.bin"))?, [0, 1, 2, 3]);
        assert_eq!(run(clean, &["rev-parse", "HEAD"])?, head);
        assert!(run(clean, &["symbolic-ref", "-q", "HEAD"]).is_err());

        let carried = prepare(&source, &storage, true)?;
        let carried = Path::new(&carried);
        assert_ne!(clean, carried);
        assert_eq!(
            fs::read_to_string(carried.join("counter.txt"))?,
            "working\n"
        );
        assert_eq!(fs::read(carried.join("data.bin"))?, [0, 8, 9, 255]);
        assert!(!carried.join("untracked.txt").exists());
        assert_eq!(run(carried, &["show", ":src/counter.txt"])?, b"committed\n");
        assert_eq!(run(&repo, &["show", ":src/counter.txt"])?, b"staged\n");
        assert_eq!(run(&repo, &["status", "--porcelain=v1", "-z"])?, status);
        assert_eq!(run(&repo, &["rev-parse", "HEAD"])?, head);
        Ok(())
    }
}
