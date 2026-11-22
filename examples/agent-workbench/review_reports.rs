use crate::{
    dispatch,
    file_references::LineRange,
    read_in_background,
    review::{git, git_root},
    workspace_files::numbered_excerpt,
};
use session::{Client, Entry, Input, ReviewTarget, Snapshot};
use std::{
    cell::Cell,
    fs::{self, File},
    path::Path,
    rc::Rc,
};
use tokio::sync::watch;
use workbench_ui::{AgentWorkbench, GenerationChoice};
use xrack_runtime::{StyledLink, StyledText};

struct Location<'a> {
    label: &'a str,
    path: &'a str,
    start: usize,
    end: usize,
}

fn location(line: &str) -> Option<Location<'_>> {
    let (_, label) = line.trim_end().rsplit_once(" — ")?;
    let (path, numbers) = label.rsplit_once(':')?;
    let (start, end) = numbers.split_once('-').unwrap_or((numbers, numbers));
    let (start, end) = (start.parse().ok()?, end.parse().ok()?);
    if start == 0 || end < start || path.is_empty() || path.contains("://") {
        return None;
    }
    Some(Location {
        label,
        path,
        start,
        end,
    })
}

pub fn report_summary(report: &str) -> (&str, usize) {
    let summary = report.split("\n\n").next().unwrap_or(report);
    let locations = report.lines().filter_map(location).count();
    (summary, locations)
}

fn report_document(report: &str) -> StyledText {
    let base =
        StyledText::from_markdown(report).unwrap_or_else(|_| StyledText::from_plain_text(report));
    let mut offset = 0;
    let links = report
        .lines()
        .filter_map(location)
        .filter_map(|reference| {
            let start = offset + base.text()[offset..].find(reference.label)?;
            offset = start + reference.label.len();
            Some(StyledLink {
                start,
                end: offset,
                href: format!(
                    "review-source:{}:{}:{}",
                    reference.start, reference.end, reference.path
                )
                .into(),
            })
        })
        .collect();
    StyledText::from_parts(base.text(), base.runs().to_vec(), links).unwrap_or(base)
}

struct SourceExcerpt {
    label: String,
    document: StyledText,
}

fn source_excerpt(workspace: &Path, target: &str) -> Result<SourceExcerpt, String> {
    let target = target
        .strip_prefix("review-source:")
        .ok_or("无法识别审阅位置")?;
    let (from, rest) = target.split_once(':').ok_or("缺少起始行号")?;
    let (to, path) = rest.split_once(':').ok_or("缺少文件路径")?;
    let first = from.parse::<usize>().map_err(|_| "起始行号无效")?;
    let last = to.parse::<usize>().map_err(|_| "结束行号无效")?;
    if first == 0 || first > last {
        return Err("审阅行号无效".into());
    }

    let root = git_root(workspace)?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let file = root
        .join(path)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let relative = file
        .strip_prefix(&root)
        .map_err(|_| "审阅位置超出当前 Git 仓库")?;
    if !fs::metadata(&file)
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("审阅位置需要指向普通文件".into());
    }
    let reader = File::open(&file).map_err(|error| error.to_string())?;
    let document = numbered_excerpt(reader, LineRange { first, last })?.document;
    Ok(SourceExcerpt {
        label: if first == last {
            format!("{} · 第 {first} 行", relative.display())
        } else {
            format!("{} · 第 {first}–{last} 行", relative.display())
        },
        document,
    })
}

fn branches(workspace: &Path) -> Result<Vec<String>, String> {
    let root = git_root(workspace)?;
    let (bytes, truncated) = git(
        &root,
        &[
            "for-each-ref",
            "--format=%(refname:short)%09%(symref)",
            "refs/heads",
            "refs/remotes",
        ],
        1024 * 1024,
    )?;
    if truncated {
        return Err("Git 分支列表过大".into());
    }
    let refs = String::from_utf8(bytes).map_err(|_| "Git 分支名必须是 UTF-8")?;
    Ok(refs
        .lines()
        .filter_map(|line| {
            let (name, symbolic) = line.split_once('\t')?;
            (symbolic.is_empty() && !name.is_empty()).then(|| name.to_owned())
        })
        .collect())
}

fn commit(workspace: &Path, revision: &str) -> Result<ReviewTarget, String> {
    let root = git_root(workspace)?;
    resolve_commit(&root, revision).map(ReviewTarget::Commit)
}

pub(super) fn resolve_commit(root: &Path, revision: &str) -> Result<String, String> {
    let revision = format!("{}^{{commit}}", revision.trim());
    let (bytes, truncated) = git(
        root,
        &["rev-parse", "--verify", "--end-of-options", &revision],
        128,
    )
    .map_err(|_| "找不到该 Git 提交")?;
    let sha = std::str::from_utf8(&bytes)
        .map_err(|_| "Git 提交 ID 无效")?
        .trim();
    if truncated
        || !matches!(sha.len(), 40 | 64)
        || !sha.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("Git 提交 ID 无效".into());
    }
    Ok(sha.into())
}

pub fn attach(ui: &mut AgentWorkbench, client: &Client, output: watch::Receiver<Snapshot>) {
    let scopes = Rc::new(Cell::new(0u64));
    let scope_revision = scopes.clone();
    let weak = ui.as_weak();
    ui.on_load_review_branches(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        let workspace = ui.get_workspace();
        let revision = scope_revision.get().wrapping_add(1);
        scope_revision.set(revision);
        ui.set_review_scope_loading(true);
        ui.set_review_scope_error(String::new());
        let weak = weak.clone();
        let scope = scope_revision.clone();
        if let Err(error) = xrack_platform_winit::spawn_local(async move {
            let folder = workspace.clone();
            let result = read_in_background("workbench-code-review", move || {
                branches(Path::new(&folder))
            })
            .await;
            let Some(mut ui) = weak.upgrade() else { return };
            if scope.get() != revision
                || ui.get_workspace() != workspace
                || !ui.get_review_scope_open()
            {
                return;
            }
            ui.set_review_scope_loading(false);
            let choices = ui.model_review_branch_choices();
            choices.clear();
            match result {
                Ok(branches) => {
                    choices.push(GenerationChoice {
                        key: String::new(),
                        label: "选择分支".into(),
                    });
                    for name in branches {
                        choices.push(GenerationChoice {
                            key: name.clone(),
                            label: name,
                        });
                    }
                }
                Err(error) => ui.set_review_scope_error(error),
            }
        }) {
            ui.set_review_scope_loading(false);
            ui.set_review_scope_error(error.to_string());
        }
    });

    let weak = ui.as_weak();
    let commands = client.commands();
    ui.on_start_code_review(move |mode, branch, revision| {
        let Some(mut ui) = weak.upgrade() else { return };
        if ui.get_busy()
            || ui.get_switching()
            || !ui.get_review_scope_open()
            || ui.get_review_scope_compare()
        {
            return;
        }
        let target = match mode.as_str() {
            "working" => Some(ReviewTarget::WorkingTree),
            "branch"
                if ui
                    .model_review_branch_choices()
                    .snapshot()
                    .iter()
                    .any(|choice| !choice.key.is_empty() && choice.key == branch) =>
            {
                Some(ReviewTarget::Branch(branch))
            }
            "commit" if !revision.trim().is_empty() => None,
            _ => return,
        };
        if let Some(target) = target {
            begin_review(&mut ui, &commands, target);
            return;
        }
        let workspace = ui.get_workspace();
        let conversation = ui.get_conversation_id();
        let weak = weak.clone();
        let commands = commands.clone();
        let scope = scopes.clone();
        let request = scope.get().wrapping_add(1);
        scope.set(request);
        ui.set_review_scope_loading(true);
        ui.set_review_scope_error(String::new());
        if let Err(error) = xrack_platform_winit::spawn_local(async move {
            let folder = workspace.clone();
            let result = read_in_background("workbench-code-review", move || {
                commit(Path::new(&folder), &revision)
            })
            .await;
            let Some(mut ui) = weak.upgrade() else { return };
            if scope.get() != request
                || ui.get_conversation_id() != conversation
                || ui.get_workspace() != workspace
                || !ui.get_review_scope_open()
            {
                return;
            }
            ui.set_review_scope_loading(false);
            match result {
                Ok(target) => {
                    begin_review(&mut ui, &commands, target);
                }
                Err(error) => ui.set_review_scope_error(error),
            }
        }) {
            ui.set_review_scope_loading(false);
            ui.set_review_scope_error(error.to_string());
        }
    });

    let documents = Rc::new(Cell::new(0u64));
    let document_revision = documents.clone();
    let weak = ui.as_weak();
    ui.on_show_review_report(move |index| {
        let Some(mut ui) = weak.upgrade() else { return };
        let snapshot = output.borrow();
        let Some(Entry::ReviewReport(report)) = usize::try_from(index)
            .ok()
            .filter(|_| {
                !ui.get_switching() && snapshot.view.conversation == ui.get_conversation_id()
            })
            .and_then(|index| snapshot.view.entries.get(index))
            .map(AsRef::as_ref)
        else {
            return;
        };
        document_revision.set(document_revision.get().wrapping_add(1));
        ui.set_review_report_document(report_document(report));
        ui.set_review_source_open(false);
        ui.set_review_report_open(true);
    });

    let weak = ui.as_weak();
    ui.on_open_review_source(move |href| {
        let Some(mut ui) = weak.upgrade() else { return };
        if !ui.get_review_report_open() || !href.starts_with("review-source:") {
            return;
        }
        let workspace = ui.get_workspace();
        let request = documents.get().wrapping_add(1);
        documents.set(request);
        ui.set_review_source_open(true);
        ui.set_review_source_title("正在读取源码…".into());
        ui.set_review_source_document(StyledText::from_plain_text(""));
        let weak = weak.clone();
        let document = documents.clone();
        if let Err(error) = xrack_platform_winit::spawn_local(async move {
            let folder = workspace.clone();
            let result = read_in_background("workbench-code-review", move || {
                source_excerpt(Path::new(&folder), &href)
            })
            .await;
            let Some(mut ui) = weak.upgrade() else { return };
            if document.get() != request
                || workspace != ui.get_workspace()
                || !ui.get_review_report_open()
                || !ui.get_review_source_open()
            {
                return;
            }
            match result {
                Ok(source) => {
                    ui.set_review_source_title(source.label);
                    ui.set_review_source_document(source.document);
                }
                Err(error) => ui.set_review_source_title(error),
            }
        }) {
            ui.set_review_source_title(error.to_string());
        }
    });
}

fn begin_review(ui: &mut AgentWorkbench, commands: &session::Commands, target: ReviewTarget) {
    if !ui.get_connected()
        || ui.get_busy()
        || ui.get_switching()
        || ui.get_archive_pending()
        || ui.get_conversation_archived()
        || ui.get_fork_pending()
        || ui.get_worktree_preparing()
        || ui.get_rename_pending()
        || ui.get_followup_pending()
        || ui.get_queue_pending()
        || ui.get_queue_loading()
        || !ui.model_queued_messages().is_empty()
    {
        ui.set_review_scope_error("请等当前操作完成后再审阅".into());
    } else if dispatch(ui, commands, Input::Review(target)) {
        ui.set_review_scope_open(false);
        ui.set_reviewing(true);
        ui.set_busy(true);
        crate::show_status(ui, session::Status::Reviewing);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_report_links_cover_paths_with_utf8_and_list_continuations() {
        let report = concat!(
            "计数器标签没有更新。\n\nReview comment:\n\n",
            "- [P2] Keep the counter label accurate — /repo/显示 text.rs:12-13\n",
            "  The label changes for every count.\n",
            "- [P3] Reset on reopen — src/main.rs:42-42\n",
        );
        assert_eq!(report_summary(report), ("计数器标签没有更新。", 2));
        let document = report_document(report);
        let links = document.links();
        assert_eq!(links.len(), 2);
        assert_eq!(
            &document.text()[links[0].start..links[0].end],
            "/repo/显示 text.rs:12-13"
        );
        assert_eq!(
            links[0].href.as_ref(),
            "review-source:12:13:/repo/显示 text.rs"
        );
        assert_eq!(links[1].href.as_ref(), "review-source:42:42:src/main.rs");
    }

    #[test]
    fn resolves_git_scopes_and_limits_source_preview_to_the_repository() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"], 1024).unwrap();
        fs::write(repo.join("src.rs"), "first\n当前行\nlast\n").unwrap();
        fs::write(temp.path().join("outside"), "outside the workspace").unwrap();
        git(&repo, &["add", "src.rs"], 1024).unwrap();
        git(
            &repo,
            &[
                "-c",
                "user.name=XRack",
                "-c",
                "user.email=xrack@example.invalid",
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "fixture",
            ],
            1024,
        )
        .unwrap();
        assert_eq!(branches(&repo).unwrap(), ["main"]);
        assert!(
            matches!(commit(&repo, "HEAD").unwrap(), ReviewTarget::Commit(sha) if sha.len() == 40)
        );
        assert!(commit(&repo, "--help").is_err());

        let preview = source_excerpt(&repo, "review-source:2:2:src.rs").unwrap();
        assert_eq!(preview.label, "src.rs · 第 2 行");
        let highlight = preview
            .document
            .runs()
            .iter()
            .find(|run| run.style.bold)
            .unwrap();
        assert!(preview.document.text()[highlight.start..highlight.end].contains("当前行"));
        assert!(source_excerpt(&repo, "review-source:500:500:src.rs").is_err());
        assert_eq!(
            source_excerpt(&repo, "review-source:1:1:../outside")
                .err()
                .unwrap(),
            "审阅位置超出当前 Git 仓库",
        );
    }
}
