use super::{STATUS_LIMIT, WorkspaceScope, git, push::published_branch, read_limited};
use crate::{external_open::find_program, read_in_background};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    fmt::Write as _,
    io::Seek,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
};
use workbench_ui::AgentWorkbench;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Repository {
    host: String,
    owner: String,
    name: String,
}

impl Repository {
    fn remote(url: &str) -> Option<Self> {
        let (authority, slug) = if let Some((scheme, rest)) = url.split_once("://") {
            if !matches!(scheme, "https" | "ssh" | "git") {
                return None;
            }
            rest.split_once('/')?
        } else {
            let (authority, slug) = url.split_once(':')?;
            authority.rsplit_once('@')?;
            (authority, slug)
        };
        let authority = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        let host = match authority {
            "ssh.github.com" | "ssh.github.com:443" => "github.com",
            host => host,
        };
        if !host.contains('.')
            || host.starts_with('.')
            || host.ends_with('.')
            || host.contains("..")
            || !host
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'.' | b'-'))
        {
            return None;
        }
        Self::named(
            host,
            slug.trim_end_matches('/')
                .strip_suffix(".git")
                .unwrap_or(slug.trim_end_matches('/')),
        )
    }

    fn named(host: &str, slug: &str) -> Option<Self> {
        let (owner, name) = slug.split_once('/')?;
        if owner.is_empty()
            || !owner
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == b'-')
            || name.is_empty()
            || matches!(name, "." | "..")
            || !name
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'.' | b'-' | b'_'))
        {
            return None;
        }
        Some(Self {
            host: host.to_ascii_lowercase(),
            owner: owner.into(),
            name: name.into(),
        })
    }

    fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    fn endpoint(&self) -> String {
        format!("repos/{}", self.slug())
    }
}

fn segment(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

struct GitHub(PathBuf);

impl GitHub {
    fn find() -> Result<Self, String> {
        let executable = if cfg!(windows) { "gh.exe" } else { "gh" };
        let launcher = find_program(executable);
        #[cfg(target_os = "macos")]
        let launcher = launcher.or_else(|| {
            ["/opt/homebrew/bin/gh", "/usr/local/bin/gh"]
                .into_iter()
                .map(PathBuf::from)
                .find(|path| path.is_file())
        });
        launcher
            .map(Self)
            .ok_or_else(|| "请安装 GitHub CLI（gh）并登录 GitHub".into())
    }

    fn api(
        &self,
        host: &str,
        endpoint: &str,
        fields: &[String],
        body: Option<&Value>,
    ) -> Result<Value, String> {
        let mut command = Command::new(&self.0);
        command
            .args([
                "api",
                "--hostname",
                host,
                "--method",
                if body.is_some() { "POST" } else { "GET" },
                endpoint,
            ])
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_NO_UPDATE_NOTIFIER", "1")
            .env_remove("GH_DEBUG");
        // An Enterprise remote must use credentials stored for that host, not an
        // environment token that gh would otherwise apply to any Enterprise host.
        if host != "github.com" {
            for key in [
                "GH_TOKEN",
                "GITHUB_TOKEN",
                "GH_ENTERPRISE_TOKEN",
                "GITHUB_ENTERPRISE_TOKEN",
            ] {
                command.env_remove(key);
            }
        }
        for field in fields {
            command.args(["--raw-field", field]);
        }
        if let Some(body) = body {
            let mut input = tempfile::tempfile().map_err(|error| error.to_string())?;
            serde_json::to_writer(&mut input, body).map_err(|error| error.to_string())?;
            input.rewind().map_err(|error| error.to_string())?;
            command.args(["--input", "-"]).stdin(input);
        } else {
            command.stdin(Stdio::null());
        }
        let mut stdout = tempfile::tempfile().map_err(|error| error.to_string())?;
        let mut stderr = tempfile::tempfile().map_err(|error| error.to_string())?;
        let status = command
            .stdout(stdout.try_clone().map_err(|error| error.to_string())?)
            .stderr(stderr.try_clone().map_err(|error| error.to_string())?)
            .status()
            .map_err(|error| format!("无法运行 GitHub CLI：{error}"))?;
        if !status.success() {
            let detail = stderr
                .rewind()
                .and_then(|()| read_limited(stderr, 8192))
                .map(|(bytes, _)| String::from_utf8_lossy(&bytes).into_owned())
                .unwrap_or_default();
            return Err(
                if detail.contains("HTTP 401") || detail.contains("gh auth login") {
                    format!("请先使用 gh 登录 {host}")
                } else if detail.contains("HTTP 403") {
                    "GitHub 拒绝请求，请检查仓库权限或 API 限流状态".into()
                } else if detail.contains("HTTP 404") {
                    "GitHub 找不到仓库或分支，请检查推送状态和仓库权限".into()
                } else if detail.contains("HTTP 422") {
                    "GitHub 未接受此次 PR，请检查分支差异和是否已有开放 PR".into()
                } else {
                    format!("GitHub CLI 请求失败（{status}）")
                },
            );
        }
        let (bytes, truncated) = stdout
            .rewind()
            .and_then(|()| read_limited(stdout, 4 * 1024 * 1024))
            .map_err(|error| error.to_string())?;
        if truncated {
            return Err("GitHub 返回的信息过长".into());
        }
        serde_json::from_slice(&bytes).map_err(|_| "无法读取 GitHub API 返回的信息".into())
    }

    fn repository(&self, repo: &Repository) -> Result<Value, String> {
        self.api(&repo.host, &repo.endpoint(), &[], None)
    }

    fn branch_head(&self, repo: &Repository, branch: &str) -> Result<String, String> {
        self.api(
            &repo.host,
            &format!("{}/branches/{}", repo.endpoint(), segment(branch)),
            &[],
            None,
        )?["commit"]["sha"]
            .as_str()
            .map(str::to_owned)
            .ok_or("GitHub 未返回分支提交 ID".into())
    }
}

struct Base {
    repo: Repository,
    id: u64,
    default_branch: String,
}

impl Base {
    fn parse(host: &str, value: &Value) -> Option<Self> {
        Some(Self {
            repo: Repository::named(host, value["full_name"].as_str()?)?,
            id: value["id"].as_u64()?,
            default_branch: value["default_branch"].as_str()?.into(),
        })
    }
}

struct Plan {
    gh: GitHub,
    source: Base,
    branch: String,
    head: String,
    bases: Vec<Base>,
    title: String,
    body: String,
}

struct Form {
    repo: String,
    branch: String,
    title: String,
    body: String,
    draft: bool,
}

struct ResultLink {
    url: String,
    existing: bool,
}

impl Plan {
    fn read(workspace: &Path, selection: Option<(&str, &str)>, gh: GitHub) -> Result<Self, String> {
        let source = published_branch(workspace, selection)?;
        let mut remotes = source.urls.iter().map(|url| Repository::remote(url));
        let repository = remotes
            .next()
            .flatten()
            .ok_or("所选远端需要指向 GitHub 仓库")?;
        if remotes.any(|repo| repo.as_ref() != Some(&repository)) {
            return Err("这个 Git 远端有多个仓库，请选择指向单个 GitHub 仓库的远端".into());
        }
        let identity = gh.repository(&repository)?;
        let current = Base::parse(&repository.host, &identity).ok_or("无法读取 GitHub 仓库身份")?;
        let remote_head = gh.branch_head(&current.repo, &source.name)?;
        if remote_head != source.head {
            return Err("源分支与本地提交不一致，请先推送需要提交审阅的分支".into());
        }
        let mut bases = Vec::new();
        for candidate in [&identity["parent"], &identity["source"], &identity] {
            if let Some(base) = Base::parse(&repository.host, candidate)
                && !bases.iter().any(|existing: &Base| existing.id == base.id)
            {
                bases.push(base);
            }
        }
        let (message, _) = git(
            &source.root,
            &["log", "-1", "--format=%s%n%b", &source.head],
            STATUS_LIMIT,
        )?;
        let message = String::from_utf8_lossy(&message);
        let (title, body) = message.split_once('\n').unwrap_or((&message, ""));
        Ok(Self {
            gh,
            source: current,
            branch: source.name,
            head: source.head,
            bases,
            title: title.trim().into(),
            body: body.trim_end().into(),
        })
    }

    fn find_open(&self, base: &Base, branch: &str) -> Result<Option<ResultLink>, String> {
        let result = self.gh.api(
            &base.repo.host,
            &format!("{}/pulls", base.repo.endpoint()),
            &[
                "state=open".into(),
                format!("head={}:{}", self.source.repo.owner, self.branch),
                format!("base={branch}"),
                "per_page=100".into(),
            ],
            None,
        )?;
        let records = result.as_array().ok_or("GitHub 未返回 PR 列表")?;
        Ok(records
            .iter()
            .find(|record| {
                record["head"]["repo"]["id"].as_u64() == Some(self.source.id)
                    && record["base"]["repo"]["id"].as_u64() == Some(base.id)
                    && record["head"]["ref"] == self.branch
                    && record["base"]["ref"] == branch
            })
            .and_then(|record| result_link(base, record, true)))
    }

    fn create(&self, form: &Form) -> Result<ResultLink, String> {
        let base = self
            .bases
            .iter()
            .find(|base| base.repo.slug() == form.repo)
            .ok_or("请选择目标仓库")?;
        if form.branch.is_empty() || form.title.is_empty() {
            return Err("请填写目标分支和 PR 标题".into());
        }
        if base.id == self.source.id && form.branch == self.branch {
            return Err("请选择与源分支不同的目标分支".into());
        }
        if self.gh.repository(&base.repo)?["id"].as_u64() != Some(base.id) {
            return Err("目标仓库已变化，请重新读取".into());
        }
        if self.gh.branch_head(&self.source.repo, &self.branch)? != self.head {
            return Err("源分支在 GitHub 上已变化，请重新读取并确认提交".into());
        }
        if let Some(existing) = self.find_open(base, &form.branch)? {
            return Ok(existing);
        }
        self.gh.branch_head(&base.repo, &form.branch)?;
        let payload = create_payload(&self.source, base, &self.branch, form);
        match self.gh.api(
            &base.repo.host,
            &format!("{}/pulls", base.repo.endpoint()),
            &[],
            Some(&payload),
        ) {
            Ok(result) => result_link(base, &result, false).ok_or("GitHub 未返回 PR 编号".into()),
            Err(error) => self
                .find_open(base, &form.branch)
                .ok()
                .flatten()
                .ok_or(error),
        }
    }
}

fn create_payload(source: &Base, base: &Base, branch: &str, form: &Form) -> Value {
    let mut payload = json!({"title":form.title,"body":form.body,"base":form.branch,"head":branch,"draft":form.draft});
    if source.id != base.id {
        payload["head"] = json!(format!("{}:{branch}", source.repo.owner));
        payload["head_repo"] = json!(source.repo.name);
    }
    payload
}

fn result_link(base: &Base, record: &Value, existing: bool) -> Option<ResultLink> {
    let number = record["number"].as_u64().filter(|number| *number > 0)?;
    Some(ResultLink {
        url: format!(
            "https://{}/{}/pull/{number}",
            base.repo.host,
            base.repo.slug()
        ),
        existing,
    })
}

type PendingPlan = Rc<RefCell<Option<(WorkspaceScope, Plan)>>>;
type Pushed = Option<(String, String)>;

fn select_base(ui: &mut AgentWorkbench, plan: &Plan) {
    let repository = ui.get_git_pr_repo();
    if let Some(base) = plan
        .bases
        .iter()
        .find(|base| base.repo.slug() == repository)
    {
        ui.set_git_pr_branch(base.default_branch.clone());
    }
}

fn prepare(
    ui: &mut AgentWorkbench,
    pending: PendingPlan,
    generation: Rc<Cell<u64>>,
    pushed: Pushed,
    preserve: bool,
) {
    let scope = WorkspaceScope::current(ui);
    pending.take();
    let ticket = generation.get().wrapping_add(1);
    generation.set(ticket);
    ui.set_git_pr_loading(true);
    ui.set_git_pr_error(String::new());
    ui.set_git_pr_source(String::new());
    ui.set_git_pr_repositories(Vec::new());
    let weak = ui.as_weak();
    let workspace = PathBuf::from(&scope.workspace);
    if let Err(error) = xrack_platform_winit::spawn_local(async move {
        let result = read_in_background("workbench-pr-preview", move || {
            Plan::read(
                &workspace,
                pushed
                    .as_ref()
                    .map(|(remote, branch)| (remote.as_str(), branch.as_str())),
                GitHub::find()?,
            )
        })
        .await;
        let Some(mut ui) = weak.upgrade() else { return };
        if generation.get() != ticket {
            return;
        }
        ui.set_git_pr_loading(false);
        if !scope.matches(&ui) || !ui.get_git_pr_open() {
            return;
        }
        match result {
            Ok(plan) => {
                ui.set_git_pr_source(format!(
                    "{}/{}:{} · {}",
                    plan.source.repo.host,
                    plan.source.repo.slug(),
                    plan.branch,
                    plan.head.chars().take(12).collect::<String>()
                ));
                let choices = plan
                    .bases
                    .iter()
                    .map(|base| base.repo.slug())
                    .collect::<Vec<_>>();
                if !preserve || !choices.contains(&ui.get_git_pr_repo()) {
                    ui.set_git_pr_repo(choices.first().cloned().unwrap_or_default());
                    select_base(&mut ui, &plan);
                }
                ui.set_git_pr_repositories(choices);
                if !preserve || ui.get_git_pr_title().is_empty() {
                    ui.set_git_pr_title(plan.title.clone());
                }
                if !preserve {
                    ui.set_git_pr_body(plan.body.clone());
                }
                pending.replace(Some((scope, plan)));
            }
            Err(error) => ui.set_git_pr_error(error),
        }
    }) {
        ui.set_git_pr_loading(false);
        ui.set_git_pr_error(error.to_string());
    }
}

pub(super) fn attach(ui: &mut AgentWorkbench) {
    let pending = Rc::new(RefCell::new(None));
    let generation = Rc::new(Cell::new(0u64));
    let selected_source = Rc::new(RefCell::new(None::<(String, String)>));
    let weak = ui.as_weak();
    let open_plan = pending.clone();
    let open_gen = generation.clone();
    let last_source = selected_source.clone();
    ui.on_open_git_pr(move |from_push| {
        let Some(mut ui) = weak.upgrade() else { return };
        if ui.get_workspace().is_empty()
            || ui.get_switching()
            || ui.get_git_pr_open()
            || ui.get_git_pr_pending()
            || from_push && !ui.get_git_push_done()
        {
            return;
        }
        let source = from_push.then(|| (ui.get_git_push_remote(), ui.get_git_push_target()));
        last_source.replace(source.clone());
        ui.set_git_pr_open(true);
        ui.set_git_pr_url(String::new());
        ui.set_git_pr_existing(false);
        ui.set_git_pr_copied(false);
        ui.set_git_pr_draft(false);
        ui.set_git_pr_repo(String::new());
        ui.set_git_pr_branch(String::new());
        ui.set_git_pr_title(String::new());
        ui.set_git_pr_body(String::new());
        prepare(&mut ui, open_plan.clone(), open_gen.clone(), source, false);
    });
    let weak = ui.as_weak();
    let reload_plan = pending.clone();
    ui.on_reload_git_pr(move || {
        if let Some(mut ui) = weak.upgrade()
            && ui.get_git_pr_open()
            && !ui.get_git_pr_loading()
            && !ui.get_git_pr_pending()
            && ui.get_git_pr_url().is_empty()
        {
            prepare(
                &mut ui,
                reload_plan.clone(),
                generation.clone(),
                selected_source.borrow().clone(),
                true,
            );
        }
    });
    let weak = ui.as_weak();
    let choose_plan = pending.clone();
    ui.on_select_git_pr_repo(move || {
        if let Some(mut ui) = weak.upgrade()
            && let Some((scope, plan)) = choose_plan.borrow().as_ref()
            && scope.matches(&ui)
        {
            select_base(&mut ui, plan);
        }
    });
    let weak = ui.as_weak();
    ui.on_confirm_git_pr(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        if !ui.get_git_pr_open() || !ui.get_can_confirm_git_pr() {
            return;
        }
        let Some((scope, plan)) = pending.take().filter(|(scope, _)| scope.matches(&ui)) else {
            return;
        };
        let form = Form {
            repo: ui.get_git_pr_repo(),
            branch: ui.get_git_pr_branch().trim().into(),
            title: ui.get_git_pr_title().trim().into(),
            body: ui.get_git_pr_body(),
            draft: ui.get_git_pr_draft(),
        };
        ui.set_git_pr_pending(true);
        ui.set_git_pr_error(String::new());
        let weak_result = weak.clone();
        let retry = pending.clone();
        if let Err(error) = xrack_platform_winit::spawn_local(async move {
            let outcome = read_in_background("workbench-pr-create", move || {
                let result = plan.create(&form);
                Ok((plan, result))
            })
            .await;
            let Some(mut ui) = weak_result.upgrade() else {
                return;
            };
            ui.set_git_pr_pending(false);
            if !scope.matches(&ui) {
                return;
            }
            match outcome {
                Ok((_, Ok(result))) => {
                    ui.set_git_pr_url(result.url);
                    ui.set_git_pr_existing(result.existing);
                }
                Ok((plan, Err(error))) => {
                    retry.replace(Some((scope, plan)));
                    ui.set_git_pr_error(error);
                }
                Err(error) => ui.set_git_pr_error(error),
            }
        }) {
            ui.set_git_pr_pending(false);
            ui.set_git_pr_error(error.to_string());
        }
    });
    let weak = ui.as_weak();
    let mut clipboard = None;
    ui.on_copy_git_pr_link(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        let url = ui.get_git_pr_url();
        if url.is_empty() {
            return;
        }
        let result = (|| {
            let clipboard = match &mut clipboard {
                Some(clipboard) => clipboard,
                None => clipboard.insert(arboard::Clipboard::new()?),
            };
            clipboard.set_text(url)
        })();
        ui.set_git_pr_copied(result.is_ok());
        if let Err(error) = result {
            ui.set_git_pr_error(error.to_string());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_configured_git_remotes_and_encodes_branch_path_segments() {
        let source = Repository::remote("git@github.com:hub-owner/ui.git").unwrap();
        assert_eq!(source.slug(), "hub-owner/ui");
        assert_eq!(source.host, "github.com");
        assert_eq!(
            Repository::remote("ssh://git@ssh.github.com:443/hub-owner/ui.git").unwrap(),
            source
        );
        assert_eq!(
            Repository::remote("https://test:placeholder@github.com/hub-owner/ui.git").unwrap(),
            source
        );
        assert!(Repository::remote("/private/local/repo").is_none());
        assert!(Repository::remote("https://github.com/owner/../repos").is_none());
        assert!(Repository::remote("https://github.com/owner/ui?key=secret").is_none());
        assert_eq!(segment("topic/界面"), "topic%2F%E7%95%8C%E9%9D%A2");
    }

    #[cfg(unix)]
    mod cli_fixture {
        use super::*;
        use crate::review::{commit::revision, git_command};
        use std::{fs, os::unix::fs::PermissionsExt};

        struct Fixture {
            directory: tempfile::TempDir,
            workspace: PathBuf,
            head: String,
        }

        impl Fixture {
            fn write(&self, name: &str, value: Value) -> std::io::Result<()> {
                fs::write(self.directory.path().join(name), value.to_string())
            }

            fn plan(&self) -> Result<Plan, String> {
                Plan::read(
                    &self.workspace,
                    Some(("origin", "topic/ui")),
                    GitHub(self.directory.path().join("fake-gh")),
                )
            }

            fn payload(&self) -> PathBuf {
                self.directory.path().join("post.json")
            }
        }

        fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
            let directory = tempfile::tempdir()?;
            let workspace = directory.path().join("repo");
            fs::create_dir(&workspace)?;
            let hooks = directory.path().join("empty-hooks");
            fs::create_dir(&hooks)?;
            let hooks_config = format!("core.hooksPath={}", hooks.display());
            for args in [
                vec!["init", "-q", "-b", "topic/ui"],
                vec![
                    "remote",
                    "add",
                    "origin",
                    "git@github.com:contributor/ui.git",
                ],
                vec![
                    "-c",
                    "user.name=XRack fixture",
                    "-c",
                    "user.email=xrack@example.invalid",
                    "-c",
                    "commit.gpgSign=false",
                    "-c",
                    hooks_config.as_str(),
                    "commit",
                    "--allow-empty",
                    "-m",
                    "UI changes",
                    "-m",
                    "Keyboard navigation.",
                ],
            ] {
                let output = git_command(&workspace).args(args).output()?;
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            let head = revision(&workspace).unwrap();
            let fixture = Fixture {
                directory,
                workspace,
                head,
            };
            let base = json!({"id":827,"full_name":"team/ui","default_branch":"main"});
            fixture.write("base.json", base.clone())?;
            fixture.write(
                "head-repo.json",
                json!({
                    "id":245, "full_name":"contributor/ui", "default_branch":"main",
                    "parent":base, "source":base,
                }),
            )?;
            fixture.write("source.json", json!({"commit":{"sha":fixture.head}}))?;
            fixture.write("existing.json", json!([]))?;
            let program = fixture.directory.path().join("fake-gh");
            fs::write(
                &program,
                r#"#!/bin/sh
root=${0%/*}
printf '%s %s\n' "$5" "$6" >> "$root/calls"
if [ "$3" != github.com ]; then exit 11; fi
case "$5 $6" in
  "GET repos/contributor/ui") cat "$root/head-repo.json" ;;
  "GET repos/team/ui") cat "$root/base.json" ;;
  "GET repos/contributor/ui/branches/topic%2Fui") cat "$root/source.json" ;;
  "GET repos/team/ui/branches/release%2Fmain") printf '%s\n' '{"commit":{"sha":"base"}}' ;;
  "GET repos/team/ui/pulls")
     printf '%s\n' "$@" > "$root/search-args"
     cat "$root/existing.json"
     ;;
  "POST repos/team/ui/pulls")
     [ "$7" = "--input" ] && [ "$8" = "-" ] || exit 12
     cat > "$root/post.json"
     if [ -f "$root/after-post.json" ]; then
       cp "$root/after-post.json" "$root/existing.json"
       printf '%s\n' 'gh: Validation Failed (HTTP 422)' >&2
       exit 1
     fi
     printf '%s\n' '{"number":84}'
     ;;
  *) exit 13 ;;
esac
"#,
            )?;
            fs::set_permissions(program, fs::Permissions::from_mode(0o700))?;
            Ok(fixture)
        }

        fn form() -> Form {
            Form {
                repo: "team/ui".into(),
                branch: "release/main".into(),
                title: "Improve keyboard navigation".into(),
                body: "## Changes\n\n- Preserve focus.".into(),
                draft: true,
            }
        }

        fn existing(number: u64) -> Value {
            json!([{
                "number":number,
                "head":{"ref":"topic/ui","repo":{"id":245}},
                "base":{"ref":"release/main","repo":{"id":827}},
            }])
        }

        #[test]
        fn sends_exact_fork_source_base_and_draft_data_to_the_cli()
        -> Result<(), Box<dyn std::error::Error>> {
            let fixture = fixture()?;
            let plan = fixture.plan()?;
            assert_eq!(plan.head, fixture.head);
            assert_eq!(plan.title, "UI changes");
            assert_eq!(plan.body, "Keyboard navigation.");
            assert_eq!(plan.bases[0].repo.slug(), "team/ui");
            let result = plan.create(&form())?;
            assert_eq!(result.url, "https://github.com/team/ui/pull/84");
            assert!(!result.existing);
            let body: Value = serde_json::from_slice(&fs::read(fixture.payload())?)?;
            assert_eq!(
                body,
                json!({
                    "title":"Improve keyboard navigation","body":"## Changes\n\n- Preserve focus.",
                    "base":"release/main","head":"contributor:topic/ui","head_repo":"ui","draft":true,
                })
            );
            let args = fs::read_to_string(fixture.directory.path().join("search-args"))?;
            assert!(
                args.lines()
                    .any(|value| value == "head=contributor:topic/ui")
            );
            assert!(args.lines().any(|value| value == "base=release/main"));
            Ok(())
        }

        #[test]
        fn finds_an_existing_pr_and_rejects_a_changed_remote_head_before_posting()
        -> Result<(), Box<dyn std::error::Error>> {
            let fixture = fixture()?;
            let plan = fixture.plan()?;
            fixture.write("existing.json", existing(17))?;
            let result = plan.create(&form())?;
            assert!(result.existing);
            assert!(result.url.ends_with("/17"));
            assert!(!fixture.payload().exists());

            fixture.write("source.json", json!({"commit":{"sha":"new SHA"}}))?;
            assert!(
                plan.create(&form())
                    .err()
                    .unwrap()
                    .contains("源分支在 GitHub 上已变化")
            );
            assert!(!fixture.payload().exists());
            Ok(())
        }

        #[test]
        fn checks_for_an_open_pr_after_a_failed_create_without_posting_again()
        -> Result<(), Box<dyn std::error::Error>> {
            let fixture = fixture()?;
            let plan = fixture.plan()?;
            fixture.write("after-post.json", existing(51))?;
            let result = plan.create(&form())?;
            assert!(result.existing);
            assert!(result.url.ends_with("/51"));
            let calls = fs::read_to_string(fixture.directory.path().join("calls"))?;
            assert_eq!(
                calls
                    .lines()
                    .filter(|line| *line == "POST repos/team/ui/pulls")
                    .count(),
                1
            );
            Ok(())
        }
    }
}
