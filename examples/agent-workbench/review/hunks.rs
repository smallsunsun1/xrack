use super::{Change, Hunk, IndexAction, changes, git_command, git_patch, git_root};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::Stdio,
};

pub(super) struct Selection {
    pub(super) action: IndexAction,
    pub(super) expected: String,
    pub(super) patch: String,
}

pub(super) fn editable(diff: &str) -> bool {
    let mut lines = diff.lines();
    let regular_file = lines
        .next()
        .is_some_and(|line| line.starts_with("diff --git "))
        && lines.next().is_some_and(|line| {
            line.starts_with("index ") && (line.ends_with(" 100644") || line.ends_with(" 100755"))
        })
        && lines
            .next()
            .is_some_and(|line| line.starts_with("--- ") && line != "--- /dev/null")
        && lines
            .next()
            .is_some_and(|line| line.starts_with("+++ ") && line != "+++ /dev/null");
    regular_file
        && lines.next().is_some_and(|line| Hunk::parse(line).is_some())
        && lines.all(|line| !line.starts_with("diff --git "))
}

fn section(line: &str) -> Option<IndexAction> {
    match line {
        "已暂存\n" => Some(IndexAction::Unstage),
        "未暂存\n" => Some(IndexAction::Stage),
        _ => None,
    }
}

pub(super) fn select(body: &str, row: usize) -> Option<Selection> {
    let lines: Vec<_> = body.split_inclusive('\n').collect();
    if !lines.get(row)?.starts_with("@@ ") {
        return None;
    }
    let (heading, action) = lines[..row]
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, line)| section(line).map(|action| (index, action)))?;
    let begin = heading + 1;
    let end = lines[row + 1..]
        .iter()
        .position(|line| section(line).is_some())
        .map_or(lines.len(), |offset| row + offset);
    // The display inserts one newline between the two raw side patches.
    let expected = lines[begin..end].concat();
    if !editable(&expected) || row < begin + 4 || row >= end {
        return None;
    }
    let hunk_end = lines[row + 1..end]
        .iter()
        .position(|line| line.starts_with("@@ "))
        .map_or(end, |offset| row + 1 + offset);
    let patch = [lines[begin], lines[begin + 2], lines[begin + 3]]
        .into_iter()
        .chain(lines[row..hunk_end].iter().copied())
        .collect();
    Some(Selection {
        action,
        expected,
        patch,
    })
}

pub(super) fn validated_root(
    workspace: &Path,
    path: &str,
    action: IndexAction,
    expected: &str,
) -> Result<PathBuf, String> {
    let root = git_root(workspace)?;
    let current = changes(&root)?;
    let file: &Change = current
        .iter()
        .find(|file| file.path == path && action.allowed(file))
        .ok_or("差异已变化，请重新选择差异块")?;
    let (bytes, truncated) = git_patch(&root, file, action.side())?;
    if truncated || bytes != expected.as_bytes() {
        return Err("差异已变化，请重新选择差异块".into());
    }
    Ok(root)
}

pub(super) fn apply(workspace: &Path, path: &str, selected: &Selection) -> Result<(), String> {
    let root = validated_root(workspace, path, selected.action, &selected.expected)?;
    let mut command = git_command(&root);
    command.args([
        "-c",
        "apply.ignoreWhitespace=no",
        "apply",
        "--cached",
        "--whitespace=nowarn",
    ]);
    if matches!(selected.action, IndexAction::Unstage) {
        command.arg("--reverse");
    }
    let mut child = command
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("无法运行 Git：{error}"))?;
    let written = child
        .stdin
        .take()
        .ok_or("Git 未打开输入管道")
        .and_then(|mut stdin| {
            stdin
                .write_all(selected.patch.as_bytes())
                .map_err(|_| "无法写入 Git 补丁")
        });
    let status = child
        .wait()
        .map_err(|error| format!("Git 执行失败：{error}"))?;
    written?;
    if !status.success() {
        return Err("无法将该差异块应用到索引，请刷新后重试".into());
    }
    Ok(())
}
