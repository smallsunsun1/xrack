use super::{
    STATUS_LIMIT, WorkspaceScope, commit::revision, git, git_command, git_root, read_limited,
};
use crate::read_in_background;
use std::{
    cell::{Cell, RefCell},
    io::Seek,
    path::{Path, PathBuf},
    process::Stdio,
    rc::Rc,
};
use workbench_ui::AgentWorkbench;

#[derive(Debug, PartialEq, Eq)]
struct Remote {
    name: String,
    urls: Vec<String>,
}

fn git_text(root: &Path, args: &[&str]) -> Result<String, String> {
    let (bytes, truncated) = git(root, args, STATUS_LIMIT)?;
    if truncated {
        return Err("Git 返回的推送配置过长".into());
    }
    String::from_utf8(bytes).map_err(|_| "Git 推送配置必须使用 UTF-8".into())
}

pub(super) fn current_branch(root: &Path) -> Option<String> {
    git_text(root, &["symbolic-ref", "--quiet", "HEAD"])
        .ok()?
        .trim_end()
        .strip_prefix("refs/heads/")
        .map(str::to_owned)
}

fn config(root: &Path, key: &str) -> Option<String> {
    git_text(root, &["config", "--get", key])
        .ok()
        .map(|value| value.trim_end().to_owned())
        .filter(|value| !value.is_empty())
}

impl Remote {
    fn read(root: &Path, name: &str) -> Result<Self, String> {
        let value = git_text(root, &["remote", "get-url", "--push", "--all", "--", name])?;
        let urls = value
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if urls.is_empty() {
            return Err("这个远端没有推送地址".into());
        }
        Ok(Self {
            name: name.into(),
            urls,
        })
    }

    fn display(&self) -> String {
        self.urls
            .iter()
            .map(|url| display_url(url))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn display_url(url: &str) -> String {
    if let Some((scheme, rest)) = url.split_once("://") {
        let rest = rest.split(['?', '#']).next().unwrap_or_default();
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        let host = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        return if path.is_empty() {
            format!("{scheme}://{host}")
        } else {
            format!("{scheme}://{host}/{path}")
        };
    }
    if let Some((authority, path)) = url.split_once(':')
        && let Some((_, host)) = authority.rsplit_once('@')
    {
        return format!("{host}:{path}");
    }
    url.to_owned()
}

struct PushPlan {
    root: PathBuf,
    head: String,
    branch: Option<String>,
    remotes: Vec<Remote>,
    preferred: String,
    upstream: Option<(String, String)>,
}

pub(super) struct PublishedBranch {
    pub root: PathBuf,
    pub head: String,
    pub name: String,
    pub urls: Vec<String>,
}

pub(super) fn published_branch(
    workspace: &Path,
    selection: Option<(&str, &str)>,
) -> Result<PublishedBranch, String> {
    let plan = PushPlan::read(workspace)?;
    let (remote, branch) =
        selection.unwrap_or_else(|| (&plan.preferred, plan.destination(&plan.preferred)));
    let branch = branch.trim().to_owned();
    if branch.is_empty() {
        return Err("请先推送分支，再从推送结果打开 PR 表单".into());
    }
    let urls = plan
        .remotes
        .iter()
        .find(|candidate| candidate.name == remote)
        .ok_or("已选远端已不可用")?
        .urls
        .clone();
    Ok(PublishedBranch {
        root: plan.root,
        head: plan.head,
        name: branch,
        urls,
    })
}

impl PushPlan {
    fn read(workspace: &Path) -> Result<Self, String> {
        let root = git_root(workspace)?;
        let head = revision(&root).ok_or("当前工作树还没有可推送的提交")?;
        let branch = current_branch(&root);
        let names = git_text(&root, &["remote"])?;
        let remotes = names
            .lines()
            .map(|name| Remote::read(&root, name))
            .collect::<Result<Vec<_>, _>>()?;
        let first = remotes.first().ok_or("请先为当前仓库配置 Git 远端")?;
        let upstream = branch.as_deref().and_then(|branch| {
            let remote = config(&root, &format!("branch.{branch}.remote"))?;
            let reference = config(&root, &format!("branch.{branch}.merge"))?;
            Some((remote, reference.strip_prefix("refs/heads/")?.to_owned()))
        });
        let branch_remote = branch
            .as_deref()
            .and_then(|branch| config(&root, &format!("branch.{branch}.pushRemote")));
        let preferred = [
            branch_remote,
            config(&root, "remote.pushDefault"),
            upstream.as_ref().map(|(remote, _)| remote.clone()),
            Some("origin".into()),
        ]
        .into_iter()
        .flatten()
        .find(|name| remotes.iter().any(|remote| &remote.name == name))
        .unwrap_or_else(|| first.name.clone());
        Ok(Self {
            root,
            head,
            branch,
            remotes,
            preferred,
            upstream,
        })
    }

    fn destination(&self, remote: &str) -> &str {
        self.upstream
            .as_ref()
            .filter(|(name, _)| name == remote)
            .map_or_else(
                || self.branch.as_deref().unwrap_or_default(),
                |(_, branch)| branch,
            )
    }

    fn push(
        &self,
        workspace: &Path,
        remote: &Remote,
        branch: &str,
        track: bool,
    ) -> Result<String, String> {
        if branch.is_empty() || branch.starts_with('-') || branch.chars().any(char::is_control) {
            return Err("请输入有效的远端分支名".into());
        }
        let destination = format!("refs/heads/{branch}");
        git(&self.root, &["check-ref-format", &destination], 256)
            .map_err(|_| "请输入有效的远端分支名")?;
        if git_root(workspace)? != self.root
            || revision(&self.root).as_ref() != Some(&self.head)
            || current_branch(&self.root) != self.branch
            || Remote::read(&self.root, &remote.name)? != *remote
        {
            return Err("本地提交、分支或推送地址已变化，请重新读取".into());
        }

        let config = format!("remote.{}.mirror=false", remote.name);
        let refspec = format!("{}:{destination}", self.head);
        let mut output = tempfile::tempfile().map_err(|error| error.to_string())?;
        let stdout = output.try_clone().map_err(|error| error.to_string())?;
        let stderr = output.try_clone().map_err(|error| error.to_string())?;
        let status = git_command(&self.root)
            .args([
                "-c",
                &config,
                "push",
                "--porcelain",
                "--no-force",
                "--no-follow-tags",
                "--recurse-submodules=no",
                "--",
                &remote.name,
                &refspec,
            ])
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .status()
            .map_err(|error| error.to_string())?;
        if !status.success() {
            let output = output
                .rewind()
                .and_then(|()| read_limited(output, 32 * 1024))
                .map(|(bytes, _)| String::from_utf8_lossy(&bytes).to_ascii_lowercase())
                .unwrap_or_default();
            return Err(if remote.urls.len() > 1 {
                "至少一个推送地址返回失败，请分别检查远端分支的状态。".into()
            } else if output.contains("non-fast-forward") || output.contains("(fetch first)") {
                "远端有本地尚未包含的提交。同步后可以重试推送。".into()
            } else if output.contains("permission denied")
                || output.contains("authentication failed")
                || output.contains("could not read username")
            {
                "远端身份验证失败，请检查 Git 凭据后重试。".into()
            } else if output.contains("hook declined") || output.contains("hook rejected") {
                "Git hook 拒绝了推送".into()
            } else {
                format!("Git 推送失败（{status}）")
            });
        }
        if track && let Some(local) = &self.branch {
            let tracking = format!("--set-upstream-to=refs/remotes/{}/{branch}", remote.name);
            if git(
                &self.root,
                &["branch", &tracking, "--", local],
                STATUS_LIMIT,
            )
            .is_err()
            {
                return Ok("推送成功，Git 未能保存上游跟踪配置".into());
            }
        }
        Ok(String::new())
    }
}

type PendingPlan = Rc<RefCell<Option<(WorkspaceScope, PushPlan)>>>;

fn select_remote(ui: &mut AgentWorkbench, plan: &PushPlan) {
    let name = ui.get_git_push_remote();
    let Some(remote) = plan.remotes.iter().find(|remote| remote.name == name) else {
        ui.set_git_push_address(String::new());
        ui.set_git_push_target(String::new());
        return;
    };
    ui.set_git_push_address(remote.display());
    ui.set_git_push_target(plan.destination(&name).into());
    ui.set_git_push_track(false);
}

fn prepare(ui: &mut AgentWorkbench, pending: PendingPlan, generation: Rc<Cell<u64>>) {
    let scope = WorkspaceScope::current(ui);
    pending.take();
    let ticket = generation.get().wrapping_add(1);
    generation.set(ticket);
    ui.set_git_push_loading(true);
    ui.set_git_push_error(String::new());
    ui.set_git_push_source(String::new());
    ui.set_git_push_commit(String::new());
    ui.set_git_push_remote(String::new());
    ui.set_git_push_address(String::new());
    ui.set_git_push_target(String::new());
    ui.set_git_push_remotes(Vec::new());
    let weak = ui.as_weak();
    let workspace = PathBuf::from(&scope.workspace);
    if let Err(error) = xrack_platform_winit::spawn_local(async move {
        let result =
            read_in_background("workbench-push-preview", move || PushPlan::read(&workspace)).await;
        let Some(mut ui) = weak.upgrade() else { return };
        if generation.get() != ticket {
            return;
        }
        ui.set_git_push_loading(false);
        if !scope.matches(&ui) || !ui.get_git_push_open() {
            return;
        }
        match result {
            Ok(plan) => {
                ui.set_git_push_source(plan.branch.clone().unwrap_or_default());
                ui.set_git_push_commit(plan.head.chars().take(12).collect());
                ui.set_git_push_remotes(
                    plan.remotes
                        .iter()
                        .map(|remote| remote.name.clone())
                        .collect(),
                );
                ui.set_git_push_remote(plan.preferred.clone());
                select_remote(&mut ui, &plan);
                pending.replace(Some((scope, plan)));
            }
            Err(error) => ui.set_git_push_error(error),
        }
    }) {
        ui.set_git_push_loading(false);
        ui.set_git_push_error(error.to_string());
    }
}

pub(super) fn attach(ui: &mut AgentWorkbench) {
    let pending = Rc::new(RefCell::new(None));
    let generation = Rc::new(Cell::new(0u64));
    let weak = ui.as_weak();
    let open_plan = pending.clone();
    let open_generation = generation.clone();
    ui.on_open_git_push(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        if ui.get_workspace().is_empty()
            || ui.get_switching()
            || ui.get_git_push_pending()
            || ui.get_git_push_open()
        {
            return;
        }
        ui.set_git_push_open(true);
        ui.set_git_push_done(false);
        prepare(&mut ui, open_plan.clone(), open_generation.clone());
    });

    let weak = ui.as_weak();
    let reload_plan = pending.clone();
    ui.on_reload_git_push(move || {
        if let Some(mut ui) = weak.upgrade()
            && ui.get_git_push_open()
            && !ui.get_git_push_pending()
            && !ui.get_git_push_loading()
            && !ui.get_git_push_done()
        {
            prepare(&mut ui, reload_plan.clone(), generation.clone());
        }
    });

    let weak = ui.as_weak();
    let choose_plan = pending.clone();
    ui.on_select_git_push_remote(move || {
        if let Some(mut ui) = weak.upgrade()
            && let Some((scope, plan)) = choose_plan.borrow().as_ref()
            && scope.matches(&ui)
        {
            select_remote(&mut ui, plan);
        }
    });

    let weak = ui.as_weak();
    ui.on_confirm_git_push(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        if !ui.get_git_push_open() || !ui.get_can_confirm_git_push() {
            return;
        }
        let Some((scope, plan)) = pending.take().filter(|(scope, _)| scope.matches(&ui)) else {
            return;
        };
        let name = ui.get_git_push_remote();
        let target = ui.get_git_push_target().trim().to_owned();
        let track = ui.get_git_push_track();
        ui.set_git_push_pending(true);
        ui.set_git_push_error(String::new());
        let weak_result = weak.clone();
        let proposed = pending.clone();
        let workspace = PathBuf::from(&scope.workspace);
        if let Err(error) = xrack_platform_winit::spawn_local(async move {
            let outcome = read_in_background("workbench-git-push", move || {
                let result = plan
                    .remotes
                    .iter()
                    .find(|remote| remote.name == name)
                    .ok_or("请重新选择 Git 远端".to_owned())
                    .and_then(|remote| plan.push(&workspace, remote, &target, track));
                Ok((plan, result))
            })
            .await;
            let Some(mut ui) = weak_result.upgrade() else {
                return;
            };
            ui.set_git_push_pending(false);
            if !scope.matches(&ui) {
                return;
            }
            match outcome {
                Ok((_, Ok(warning))) => {
                    ui.set_git_push_done(true);
                    ui.set_git_push_error(warning);
                }
                Ok((plan, Err(error))) => {
                    proposed.replace(Some((scope, plan)));
                    ui.set_git_push_error(error);
                }
                Err(error) => ui.set_git_push_error(error),
            }
        }) {
            ui.set_git_push_pending(false);
            ui.set_git_push_error(error.to_string());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Fixture {
        _directory: tempfile::TempDir,
        local: PathBuf,
        remote: PathBuf,
    }

    fn run(root: &Path, args: &[&str]) -> Result<String, String> {
        let output = git_command(root)
            .args(args)
            .output()
            .map_err(|error| error.to_string())?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim_end().into())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim_end().into())
        }
    }

    fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let base = directory.path().canonicalize()?;
        let local = base.join("checkout");
        let remote = base.join("remote.git");
        fs::create_dir(&local)?;
        fs::create_dir(&remote)?;
        run(&local, &["init", "-q", "-b", "main"])?;
        run(&remote, &["init", "--bare", "-q", "-b", "main"])?;
        let hooks = local.join(".git/disabled-fixture-hooks");
        fs::create_dir(&hooks)?;
        for (key, value) in [
            ("user.name", "XRack fixture"),
            ("user.email", "xrack@example.invalid"),
            ("commit.gpgSign", "false"),
            ("push.gpgSign", "false"),
            ("tag.gpgSign", "false"),
            ("protocol.allow", "never"),
            ("protocol.file.allow", "always"),
            ("core.hooksPath", hooks.to_str().unwrap()),
        ] {
            run(&local, &["config", "--local", key, value])?;
        }
        run(
            &local,
            &["remote", "add", "xrack-fixture", remote.to_str().unwrap()],
        )?;
        run(&local, &["commit", "--allow-empty", "-m", "Fixture"])?;
        Ok(Fixture {
            _directory: directory,
            local,
            remote,
        })
    }

    fn selected<'a>(fixture: &Fixture, plan: &'a PushPlan) -> &'a Remote {
        let remote = plan
            .remotes
            .iter()
            .find(|remote| remote.name == "xrack-fixture")
            .unwrap();
        assert_eq!(remote.urls, [fixture.remote.to_str().unwrap()]);
        remote
    }

    #[test]
    fn pushes_only_the_confirmed_commit_and_can_set_upstream()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = fixture()?;
        let local = &fixture.local;
        let remote = &fixture.remote;
        run(local, &["tag", "-a", "-m", "Fixture tag", "fixture-tag"])?;
        run(local, &["config", "--local", "push.followTags", "true"])?;
        run(
            local,
            &["config", "--local", "remote.xrack-fixture.mirror", "true"],
        )?;
        let plan = PushPlan::read(local)?;
        let target = selected(&fixture, &plan);
        assert!(plan.push(local, target, "release/ui", true)?.is_empty());
        assert_eq!(
            run(remote, &["rev-parse", "refs/heads/release/ui"])?,
            plan.head
        );
        assert_eq!(
            run(remote, &["for-each-ref", "--format=%(refname)"])?,
            "refs/heads/release/ui"
        );
        assert_eq!(
            run(local, &["config", "--get", "branch.main.remote"])?,
            "xrack-fixture"
        );
        assert_eq!(
            run(local, &["config", "--get", "branch.main.merge"])?,
            "refs/heads/release/ui"
        );
        let refreshed = PushPlan::read(local)?;
        assert_eq!(refreshed.preferred, "xrack-fixture");
        assert_eq!(refreshed.destination("xrack-fixture"), "release/ui");
        Ok(())
    }

    #[test]
    fn rejects_changed_addresses_and_changed_local_commits()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = fixture()?;
        let local = &fixture.local;
        let plan = PushPlan::read(local)?;
        let target = selected(&fixture, &plan);
        let changed = fixture.remote.with_file_name("other.git");
        run(
            local,
            &[
                "remote",
                "set-url",
                "--push",
                "xrack-fixture",
                changed.to_str().unwrap(),
            ],
        )?;
        assert!(
            plan.push(local, target, "main", false)
                .unwrap_err()
                .contains("推送地址已变化")
        );
        run(
            local,
            &[
                "remote",
                "set-url",
                "--push",
                "xrack-fixture",
                fixture.remote.to_str().unwrap(),
            ],
        )?;
        run(
            local,
            &["commit", "--allow-empty", "-m", "New local commit"],
        )?;
        assert!(
            plan.push(local, target, "main", false)
                .unwrap_err()
                .contains("本地提交")
        );
        assert!(run(&fixture.remote, &["for-each-ref", "--format=%(refname)"])?.is_empty());
        Ok(())
    }

    #[test]
    fn detached_head_can_publish_a_branch_and_the_remote_rejects_divergence()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = fixture()?;
        let local = &fixture.local;
        let baseline = revision(local).unwrap();
        run(local, &["commit", "--allow-empty", "-m", "Upstream change"])?;
        let first = PushPlan::read(local)?;
        first.push(local, selected(&fixture, &first), "shared", false)?;
        run(local, &["switch", "--detach", &baseline])?;
        run(
            local,
            &["commit", "--allow-empty", "-m", "Divergent change"],
        )?;
        let detached = PushPlan::read(local)?;
        let remote = selected(&fixture, &detached);
        assert!(detached.branch.is_none());
        let error = detached.push(local, remote, "shared", false).unwrap_err();
        assert!(error.contains("尚未包含"));
        assert_eq!(
            run(&fixture.remote, &["rev-parse", "refs/heads/shared"])?,
            first.head
        );
        detached.push(local, remote, "isolated/ui", false)?;
        assert_eq!(
            run(&fixture.remote, &["rev-parse", "refs/heads/isolated/ui"])?,
            detached.head
        );
        assert!(
            detached
                .push(local, remote, "branch:refs/tags/bypass", false)
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn push_addresses_omit_credentials_and_query_values() {
        assert_eq!(
            display_url("https://account:fixture@example.invalid/org/project?token=hidden"),
            "https://example.invalid/org/project"
        );
        assert_eq!(
            display_url("git@example.invalid:org/project"),
            "example.invalid:org/project"
        );
        assert_eq!(
            display_url("/local/space in filename"),
            "/local/space in filename"
        );
    }
}
