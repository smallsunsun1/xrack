use super::{IndexSummary, PATCH_LIMIT, Review, ReviewFile, STATUS_LIMIT, git, git_root};
use crate::review_reports::resolve_commit;
use std::path::Path;
use workbench_ui::AgentWorkbench;

#[derive(Clone, PartialEq, Eq)]
pub(super) enum Scope {
    Branch(String),
    Commit(String),
}

impl Scope {
    pub(super) fn read(ui: &AgentWorkbench) -> Option<Self> {
        let target = ui.get_review_comparison_target();
        match ui.get_review_comparison_mode().as_str() {
            "branch" => Some(Self::Branch(target)),
            "commit" => Some(Self::Commit(target)),
            _ => None,
        }
    }

    fn resolve(&self, root: &Path) -> Result<Range, String> {
        match self {
            Self::Branch(branch) => {
                let base = resolve_branch(root, branch)?;
                let head = resolve_commit(root, "HEAD")?;
                let (bytes, truncated) = git(root, &["merge-base", &base, &head], 128)
                    .map_err(|_| "当前提交与所选分支没有可用的共同祖先")?;
                let common = object_id(&bytes, truncated)?;
                Ok(Range {
                    parent: common,
                    caption: format!("相对 {branch} · {}", &head[..8]),
                    head,
                    fixed_target: None,
                })
            }
            Self::Commit(revision) => {
                let head = resolve_commit(root, revision)?;
                let (bytes, _) = git(root, &["cat-file", "commit", &head], 4096)?;
                let parent = bytes
                    .split(|byte| *byte == b'\n')
                    .nth(1)
                    .and_then(|line| line.strip_prefix(b"parent "))
                    .map(|raw| object_id(raw, false))
                    .transpose()?
                    .unwrap_or_default();
                Ok(Range {
                    parent,
                    caption: format!("提交 {}", &head[..8]),
                    fixed_target: Some(head.clone()),
                    head,
                })
            }
        }
    }
}

fn resolve_branch(root: &Path, name: &str) -> Result<String, String> {
    if name.starts_with("refs/heads/") || name.starts_with("refs/remotes/") {
        return resolve_commit(root, name);
    }
    resolve_commit(root, &format!("refs/heads/{name}"))
        .or_else(|_| resolve_commit(root, &format!("refs/remotes/{name}")))
        .or_else(|error| {
            if name.starts_with("heads/") || name.starts_with("remotes/") {
                resolve_commit(root, &format!("refs/{name}"))
            } else {
                Err(error)
            }
        })
        .map_err(|_| format!("找不到分支 {name}"))
}

fn object_id(bytes: &[u8], truncated: bool) -> Result<String, String> {
    let id = std::str::from_utf8(bytes)
        .map_err(|_| "Git 对象 ID 无效")?
        .trim();
    if truncated || !matches!(id.len(), 40 | 64) || !id.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("Git 对象 ID 无效".into());
    }
    Ok(id.into())
}

struct Range {
    parent: String,
    head: String,
    caption: String,
    fixed_target: Option<String>,
}

impl Range {
    fn diff_args(&self) -> Vec<&str> {
        let mut args = if self.parent.is_empty() {
            vec!["diff-tree", "--root", "--no-commit-id", "-r", &self.head]
        } else {
            vec!["diff", &self.parent, &self.head]
        };
        args.extend([
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--find-renames",
        ]);
        args
    }
}

struct File {
    row: ReviewFile,
    previous: Option<String>,
}

fn parse_files(bytes: &[u8]) -> Result<Vec<File>, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "变更路径必须是 UTF-8")?;
    let mut fields = text.split_terminator('\0');
    let mut files = Vec::new();
    while let Some(status) = fields.next() {
        let code = status
            .as_bytes()
            .first()
            .copied()
            .filter(u8::is_ascii_uppercase)
            .ok_or("Git 返回了无效的文件状态")?;
        let path = fields
            .next()
            .filter(|path| !path.is_empty())
            .ok_or("Git 未返回文件路径")?;
        let (path, previous) = if matches!(code, b'R' | b'C') {
            let renamed = fields
                .next()
                .filter(|path| !path.is_empty())
                .ok_or("Git 未返回目标路径")?;
            (renamed, Some(path.into()))
        } else {
            (path, None)
        };
        files.push(File {
            row: ReviewFile {
                path: path.into(),
                status: char::from(code).to_string(),
            },
            previous,
        });
    }
    Ok(files)
}

pub(super) struct Metadata {
    pub(super) caption: String,
    pub(super) fixed_target: Option<String>,
    pub(super) source_tree: String,
}

pub(super) fn load(workspace: &Path, selected: &str, scope: &Scope) -> Result<Review, String> {
    let root = git_root(workspace)?;
    let range = scope.resolve(&root)?;
    let mut args = range.diff_args();
    args.extend(["--name-status", "-z", "--"]);
    let (bytes, truncated) = git(&root, &args, STATUS_LIMIT)?;
    if truncated {
        return Err("变更列表过大，无法显示".into());
    }
    let files = parse_files(&bytes)?;
    let count = files.len();
    let active = files
        .iter()
        .find(|file| file.row.path == selected)
        .or_else(|| files.first());
    let (selected, body, source_tree) = if let Some(file) = active {
        let mut args = range.diff_args();
        args.extend(["-p", "--", &file.row.path]);
        if let Some(previous) = &file.previous {
            args.push(previous);
        }
        let body = git(&root, &args, PATCH_LIMIT).map(|(bytes, truncated)| {
            let mut text = String::from_utf8_lossy(&bytes).into_owned();
            if truncated {
                text.push_str("\n预览已截断。\n");
            }
            text
        });
        let tree = if file.row.status == "D" {
            &range.parent
        } else {
            &range.head
        };
        (file.row.path.clone(), body, tree.clone())
    } else {
        (String::new(), Ok(String::new()), String::new())
    };
    Ok(Review {
        files: files.into_iter().map(|file| file.row).collect(),
        selected,
        base: root.to_string_lossy().into_owned(),
        status: format!("{} · {} 个变更文件", range.caption, count),
        body,
        index: IndexSummary::default(),
        comparison: Some(Metadata {
            caption: range.caption,
            fixed_target: range.fixed_target,
            source_tree,
        }),
    })
}

pub(super) fn source(root: &Path, tree: &str, path: &str) -> Result<String, String> {
    let tree = object_id(tree.as_bytes(), false)?;
    let (bytes, truncated) = git(
        root,
        &["cat-file", "blob", &format!("{tree}:{path}")],
        PATCH_LIMIT,
    )
    .map_err(|_| "这个版本中无法打开该文件的源码")?;
    if bytes.contains(&0) {
        return Ok("二进制文件".into());
    }
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        text.push_str("\n预览已截断。\n");
    }
    Ok(text)
}
