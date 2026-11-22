use session::{Entry, Snapshot, TurnDiff};
use std::{
    cell::{Cell, RefCell},
    fs::{self, File},
    io::{self, Read},
    ops::Range,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
    thread,
};
use tokio::sync::{oneshot, watch};
use workbench_ui::{AgentWorkbench, ReviewFile, ReviewLine};
use xrack_runtime::{Color, Length, StyledRun, StyledText, TextStyle};

mod branches;
mod commit;
mod comparison;
mod find;
mod hunks;
mod line_edit;
mod notes;
mod pull_request;
mod push;
mod revert;
mod split;
pub(super) use branches::worktree_branches;
pub(super) use notes::{capture_notes, restore_notes};

const STATUS_LIMIT: usize = 1024 * 1024;
const PATCH_LIMIT: usize = 32 * 1024;

#[derive(Clone)]
struct WorkspaceScope {
    workspace: String,
    conversation: i64,
}

impl WorkspaceScope {
    fn current(ui: &AgentWorkbench) -> Self {
        Self {
            workspace: ui.get_workspace(),
            conversation: ui.get_conversation_id(),
        }
    }

    fn matches(&self, ui: &AgentWorkbench) -> bool {
        self.workspace == ui.get_workspace() && self.conversation == ui.get_conversation_id()
    }
}

struct Hunk {
    old: Range<i64>,
    new: Range<i64>,
}

impl Hunk {
    fn parse(header: &str) -> Option<Self> {
        let mut parts = header.strip_prefix("@@ ")?.split_ascii_whitespace();
        Some(Self {
            old: line_range(parts.next()?.strip_prefix('-')?)?,
            new: line_range(parts.next()?.strip_prefix('+')?)?,
        })
    }

    fn row(&mut self, marker: u8) -> Option<(i64, i64, &'static str)> {
        match marker {
            b'+' => Some((0, self.new.next()?, "add")),
            b'-' => Some((self.old.next()?, 0, "remove")),
            b' ' if !self.old.is_empty() && !self.new.is_empty() => {
                Some((self.old.next()?, self.new.next()?, "context"))
            }
            _ => None,
        }
    }
}

fn line_range(value: &str) -> Option<Range<i64>> {
    let (start, count) = value.split_once(',').unwrap_or((value, "1"));
    let start: i64 = start.parse().ok()?;
    let count: i64 = count.parse().ok()?;
    if start < 0 || count < 0 || start == 0 && count != 0 {
        return None;
    }
    Some(start..start.checked_add(count)?)
}

fn numbered_lines(patch: &str) -> Vec<ReviewLine> {
    let mut section = "";
    let mut hunk = None;
    let mut untracked = 0;
    patch
        .lines()
        .enumerate()
        .map(|(index, text)| {
            let (old_line, new_line, kind, content) = match text {
                "已暂存" | "未暂存" | "未跟踪文件" => {
                    section = text;
                    untracked = i64::from(text == "未跟踪文件");
                    hunk = None;
                    (0, 0, "hunk", text)
                }
                text if text.starts_with("@@ ") => {
                    untracked = 0;
                    hunk = Hunk::parse(text);
                    (0, 0, "hunk", text)
                }
                text if untracked > 0 && text.starts_with('+') => {
                    let line = untracked;
                    untracked += 1;
                    (0, line, "add", &text[1..])
                }
                text => {
                    if text.starts_with("diff --git ") {
                        hunk = None;
                    }
                    let location = text
                        .as_bytes()
                        .first()
                        .and_then(|marker| hunk.as_mut()?.row(*marker));
                    match location {
                        Some((old, new, kind)) => (old, new, kind, &text[1..]),
                        None => (0, 0, "meta", text),
                    }
                }
            };
            ReviewLine {
                id: index as i64,
                old_line,
                new_line,
                kind: kind.into(),
                content: content.replace('\t', "    "),
                section: section.into(),
            }
        })
        .collect()
}

#[derive(Default)]
pub struct NoteDraft {
    reference: String,
    text: String,
    queued: Option<i64>,
}

impl NoteDraft {
    pub fn capture(ui: &AgentWorkbench) -> Self {
        let queued = ui.get_review_note_queued_id();
        Self {
            reference: ui.get_review_note_reference(),
            text: ui.get_review_note_text(),
            queued: (queued >= 0).then_some(queued),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.reference.is_empty()
    }

    pub fn restore(self, ui: &mut AgentWorkbench) {
        ui.set_review_note_reference(self.reference);
        ui.set_review_note_text(self.text);
        ui.set_review_note_queued_id(self.queued.unwrap_or(-1));
        ui.set_review_line_id(-1);
    }
}

fn select_line(ui: &mut AgentWorkbench, id: i64) {
    select_line_side(ui, id, None);
}

fn select_line_side(ui: &mut AgentWorkbench, id: i64, old_side: Option<bool>) {
    if !ui.get_review_open() || ui.get_switching() || !ui.get_review_note_text().is_empty() {
        return;
    }
    let Some(line) = usize::try_from(id)
        .ok()
        .and_then(|index| ui.model_review_lines().row(index))
        .filter(|row| row.id == id)
    else {
        return;
    };
    let old = old_side.unwrap_or(line.new_line <= 0);
    let number = if old { line.old_line } else { line.new_line };
    if number <= 0 {
        return;
    }
    let location = if old {
        format!("旧行 {number}")
    } else {
        format!("新行 {number}")
    };
    let selected = ui.get_review_selection();
    if selected.is_empty() {
        return;
    }
    let base = ui.get_review_base();
    let path = if base.is_empty() || base == ui.get_workspace() {
        selected
    } else {
        Path::new(&base)
            .join(selected)
            .to_string_lossy()
            .into_owned()
    }
    .replace(['\n', '\r'], " ");
    let section = if !line.section.is_empty() {
        line.section
    } else {
        let caption = ui.get_review_comparison_label();
        if caption.is_empty() {
            "本轮改动".into()
        } else {
            caption
        }
    };
    ui.set_review_line_id(id);
    ui.set_review_line_old(old);
    ui.set_review_note_queued_id(-1);
    ui.set_review_note_reference(format!("{path} · {location} · {section}"));
}

fn add_note(ui: &mut AgentWorkbench) {
    if !ui.get_review_open() || ui.get_switching() {
        return;
    }
    let note = NoteDraft::capture(ui);
    let text = note.text.trim();
    if note.is_empty() || text.is_empty() {
        return;
    }
    notes::append(ui, [(&*note.reference, text)]);
    if let Some(id) = note.queued {
        notes::remove(ui, id);
    }
    NoteDraft::default().restore(ui);
}

#[derive(Debug, PartialEq, Eq)]
struct Change {
    status: String,
    path: String,
    previous_path: Option<String>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Filter {
    #[default]
    All,
    Unstaged,
    Staged,
}

impl Filter {
    fn from_ui(value: &str) -> Self {
        match value {
            "unstaged" => Self::Unstaged,
            "staged" => Self::Staged,
            _ => Self::All,
        }
    }

    fn includes(self, change: &Change) -> bool {
        match self {
            Self::All => true,
            Self::Unstaged => change.unstaged(),
            Self::Staged => change.staged(),
        }
    }

    fn empty_label(self) -> &'static str {
        match self {
            Self::All => "工作区没有文件改动",
            Self::Unstaged => "没有未暂存的改动",
            Self::Staged => "暂存区没有改动",
        }
    }
}

impl Change {
    fn conflict(&self) -> bool {
        matches!(
            self.status.as_str(),
            "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU"
        )
    }

    fn staged(&self) -> bool {
        !self.conflict()
            && !matches!(self.status.as_str(), "??" | "!!")
            && self.status.as_bytes()[0] != b' '
    }

    fn unstaged(&self) -> bool {
        self.status != "!!" && self.status.as_bytes()[1] != b' '
    }
}

#[derive(Clone, Copy)]
enum IndexAction {
    Stage,
    Unstage,
}

impl IndexAction {
    fn side(self) -> Filter {
        match self {
            Self::Stage => Filter::Unstaged,
            Self::Unstage => Filter::Staged,
        }
    }

    fn allowed(self, change: &Change) -> bool {
        match self {
            Self::Stage => change.unstaged() && !change.conflict(),
            Self::Unstage => change.staged(),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Stage => "暂存",
            Self::Unstage => "取消暂存",
        }
    }
}

pub(super) fn git_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .args([
            "--no-pager",
            "--no-optional-locks",
            "--literal-pathspecs",
            "-c",
            "core.fsmonitor=false",
            "-C",
        ])
        .arg(root)
        .env("GIT_TERMINAL_PROMPT", "0");
    command
}

pub(super) fn git(root: &Path, args: &[&str], limit: usize) -> Result<(Vec<u8>, bool), String> {
    let mut child = git_command(root)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("无法运行 Git：{error}"))?;
    let data = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("Git stdout unavailable"))
        .and_then(|stdout| read_limited(stdout, limit));
    let status = child
        .wait()
        .map_err(|error| format!("Git 执行失败：{error}"))?;
    let (bytes, truncated) = data.map_err(|error| format!("读取 Git 输出失败：{error}"))?;
    if status.success() || truncated {
        Ok((bytes, truncated))
    } else {
        Err(format!("Git {} 执行失败（{status}）", args[0]))
    }
}

pub(super) fn read_limited(reader: impl Read, limit: usize) -> io::Result<(Vec<u8>, bool)> {
    let mut bytes = Vec::new();
    reader.take((limit + 1) as u64).read_to_end(&mut bytes)?;
    let truncated = bytes.len() > limit;
    bytes.truncate(limit);
    Ok((bytes, truncated))
}

pub(super) fn git_root(workspace: &Path) -> Result<PathBuf, String> {
    let (bytes, truncated) = git(workspace, &["rev-parse", "--show-toplevel"], STATUS_LIMIT)?;
    if truncated {
        return Err("Git 仓库路径过长".into());
    }
    let path = String::from_utf8(bytes).map_err(|_| "Git 仓库路径必须是 UTF-8")?;
    // Remove only the line ending emitted by Git. Spaces are valid path characters.
    let path = path
        .strip_suffix("\r\n")
        .or_else(|| path.strip_suffix('\n'))
        .unwrap_or(&path);
    Ok(path.into())
}

fn parse_changes(bytes: &[u8]) -> Result<Vec<Change>, String> {
    let status = std::str::from_utf8(bytes).map_err(|_| "变更路径必须是 UTF-8")?;
    let mut records = status.split_terminator('\0');
    let mut changes = Vec::new();
    while let Some(record) = records.next() {
        let (code, path) = record
            .get(..2)
            .zip(record.get(3..))
            .filter(|(code, path)| {
                code.is_ascii() && !path.is_empty() && record.as_bytes()[2] == b' '
            })
            .ok_or("Git 返回了无效的状态记录")?;
        let previous_path = if code.contains(['R', 'C']) {
            Some(records.next().ok_or("Git 未返回重命名的原路径")?.to_owned())
        } else {
            None
        };
        changes.push(Change {
            status: code.to_owned(),
            path: path.to_owned(),
            previous_path,
        });
    }
    Ok(changes)
}

fn changes(root: &Path) -> Result<Vec<Change>, String> {
    let (bytes, truncated) = git(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=dirty",
        ],
        STATUS_LIMIT,
    )?;
    if truncated {
        return Err("变更列表过大，无法显示".into());
    }
    parse_changes(&bytes)
}

fn update_index(workspace: &Path, selected: &str, action: IndexAction) -> Result<(), String> {
    let root = git_root(workspace)?;
    let current = changes(&root)?;
    let change = current
        .iter()
        .find(|change| change.path == selected && action.allowed(change))
        .ok_or("选中文件的 Git 状态已变化")?;
    let mut args = match action {
        IndexAction::Stage => vec!["add", "-A", "--"],
        // Pathspec mode updates only the index, including on an unborn branch.
        IndexAction::Unstage => vec!["reset", "--quiet", "--no-refresh", "--"],
    };
    args.push(&change.path);
    let column = match action {
        IndexAction::Stage => 1,
        IndexAction::Unstage => 0,
    };
    if change.status.as_bytes()[column] == b'R'
        && let Some(previous_path) = &change.previous_path
    {
        args.push(previous_path);
    }
    git(&root, &args, PATCH_LIMIT).map(|_| ())
}

fn untracked_patch(root: &Path, path: &str) -> Result<String, String> {
    let relative = Path::new(path);
    if !relative
        .components()
        .all(|segment| matches!(segment, Component::Normal(_)))
    {
        return Err("无效的 Git 文件路径".into());
    }
    let path = root.join(relative);
    let metadata = fs::symlink_metadata(&path).map_err(|error| format!("无法查看文件：{error}"))?;
    if !metadata.is_file() {
        return Ok("此条目不是普通文件。".into());
    }
    let resolved = path
        .canonicalize()
        .map_err(|error| format!("无法解析文件路径：{error}"))?;
    let root = root
        .canonicalize()
        .map_err(|error| format!("无法解析 Git 工作区：{error}"))?;
    if !resolved.starts_with(root) {
        return Err("文件路径已移出 Git 工作区，请刷新列表".into());
    }
    let file = File::open(resolved).map_err(|error| format!("无法打开文件：{error}"))?;
    let (bytes, truncated) =
        read_limited(file, PATCH_LIMIT).map_err(|error| format!("无法读取文件：{error}"))?;
    if bytes.contains(&0) {
        return Ok("新增二进制文件".into());
    }
    let text = String::from_utf8_lossy(&bytes);
    let mut patch = String::from("未跟踪文件\n");
    for line in text.split_inclusive('\n') {
        patch.push('+');
        patch.push_str(line);
    }
    if truncated {
        patch.push_str("\n预览已截断。\n");
    }
    Ok(patch)
}

fn git_patch(root: &Path, change: &Change, side: Filter) -> Result<(Vec<u8>, bool), String> {
    let mut args = vec![
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--no-color",
        "--no-relative",
        "--src-prefix=a/",
        "--dst-prefix=b/",
        "--unified=3",
        "--find-renames",
        "--ignore-submodules=dirty",
        "--",
        change.path.as_str(),
    ];
    if let Some(previous) = &change.previous_path {
        args.push(previous);
    }
    if side == Filter::Staged {
        args.insert(1, "--cached");
    }
    git(root, &args, PATCH_LIMIT)
}

fn patch(
    root: &Path,
    change: &Change,
    filter: Filter,
    index: &mut IndexSummary,
) -> Result<String, String> {
    if change.status == "??" {
        return untracked_patch(root, &change.path);
    }
    let mut patch = String::new();
    for (heading, action) in [
        ("已暂存", IndexAction::Unstage),
        ("未暂存", IndexAction::Stage),
    ] {
        let side = action.side();
        if (filter != Filter::All && filter != side) || !side.includes(change) {
            continue;
        }
        let (bytes, truncated) = git_patch(root, change, side)?;
        if bytes.is_empty() {
            continue;
        }
        if !patch.is_empty() {
            patch.push('\n');
        }
        patch.push_str(heading);
        patch.push('\n');
        patch.push_str(&String::from_utf8_lossy(&bytes));
        let editable = !truncated
            && action.allowed(change)
            && std::str::from_utf8(&bytes).is_ok_and(hunks::editable);
        match action {
            IndexAction::Stage => index.can_stage_hunks = editable,
            IndexAction::Unstage => index.can_unstage_hunks = editable,
        }
        if truncated {
            patch.push_str("\n预览已截断。\n");
        }
    }
    if patch.is_empty() {
        patch.push_str("当前没有可显示的文本差异。");
    }
    Ok(patch)
}

fn highlight(patch: &str, dark: bool) -> StyledText {
    let mut offset = 0;
    let runs = patch
        .split_inclusive('\n')
        .map(|line| {
            let start = offset;
            offset += line.len();
            let (color, bold) = if ["+++ ", "--- ", "diff "]
                .iter()
                .any(|prefix| line.starts_with(prefix))
            {
                (if dark { 0xb9caffff } else { 0x3a4a78ff }, true)
            } else if line.starts_with('+') {
                (if dark { 0x7ed9acff } else { 0x1b6b47ff }, false)
            } else if line.starts_with('-') {
                (if dark { 0xffa8b6ff } else { 0xa23b4dff }, false)
            } else if line.starts_with("@@") {
                (if dark { 0xc5b7ffff } else { 0x5449a8ff }, true)
            } else {
                (if dark { 0xe8eefcff } else { 0x43516fff }, false)
            };
            StyledRun {
                start,
                end: offset,
                style: TextStyle {
                    monospace: true,
                    bold,
                    color: Some(Color(color)),
                    ..Default::default()
                },
            }
        })
        .collect();
    StyledText::from_runs(patch, runs).unwrap_or_else(|_| StyledText::from_plain_text(patch))
}

pub(super) fn recolor_raw_diff(ui: &mut AgentWorkbench) {
    let body = ui.get_review_body();
    if !body.text().is_empty() {
        ui.set_review_body(highlight(body.text(), ui.get_dark_appearance()));
        ui.invoke_refresh_review_find();
    }
}

fn set_patch(ui: &mut AgentWorkbench, body: String) {
    if ui.get_review_body().text() == body {
        return;
    }
    let model = ui.model_review_lines();
    let rows = numbered_lines(&body);
    split::sync_pairs(ui, &rows);
    let count = rows.len();
    for (index, row) in rows.into_iter().enumerate() {
        if index < model.len() {
            model.set_row(index, row);
        } else {
            model.push(row);
        }
    }
    while model.len() > count {
        model.remove(model.len() - 1);
    }
    ui.set_review_line_id(-1);
    ui.set_review_body(highlight(&body, ui.get_dark_appearance()));
    ui.invoke_refresh_review_find();
}

struct Review {
    files: Vec<ReviewFile>,
    selected: String,
    base: String,
    status: String,
    body: Result<String, String>,
    index: IndexSummary,
    comparison: Option<comparison::Metadata>,
}

#[derive(Default)]
struct IndexSummary {
    all: i64,
    staged: i64,
    unstaged: i64,
    can_stage: bool,
    can_unstage: bool,
    can_stage_hunks: bool,
    can_unstage_hunks: bool,
}

impl IndexSummary {
    fn publish(self, ui: &mut AgentWorkbench) {
        ui.set_review_all_count(self.all);
        ui.set_review_staged_count(self.staged);
        ui.set_review_unstaged_count(self.unstaged);
        ui.set_review_can_stage(self.can_stage);
        ui.set_review_can_unstage(self.can_unstage);
        ui.set_review_can_stage_hunks(self.can_stage_hunks);
        ui.set_review_can_unstage_hunks(self.can_unstage_hunks);
    }
}

pub fn turn_summary(diff: &TurnDiff) -> String {
    if diff.files.is_empty() {
        return "本轮无净改动".into();
    }
    format!(
        "{} 个文件 · +{} −{}{}",
        diff.files.len(),
        diff.additions,
        diff.deletions,
        if diff.truncated { " · 已截断" } else { "" }
    )
}

fn turn_review(diff: &TurnDiff, selected: &str) -> Review {
    let active = diff
        .files
        .iter()
        .find(|file| file.path == selected)
        .or_else(|| diff.files.first());
    let (selected, body) = active.map_or_else(
        || (String::new(), String::new()),
        |file| {
            let patch = diff.patch(file);
            let end = patch.floor_char_boundary(PATCH_LIMIT);
            let mut body = patch[..end].to_owned();
            if end < patch.len() {
                body.push_str("\n预览已截断。\n");
            }
            (file.path.clone(), body)
        },
    );
    Review {
        files: diff
            .files
            .iter()
            .map(|file| ReviewFile {
                path: file.path.clone(),
                status: file.status.to_string(),
            })
            .collect(),
        selected,
        base: String::new(),
        status: turn_summary(diff),
        body: Ok(body),
        index: IndexSummary::default(),
        comparison: None,
    }
}

fn load(workspace: &Path, selected: &str, filter: Filter) -> Result<Review, String> {
    let root = git_root(workspace).map_err(|error| format!("无法定位 Git 工作区。{error}"))?;
    let mut files = changes(&root)?;
    let mut index = IndexSummary {
        all: files.len() as i64,
        staged: files.iter().filter(|file| file.staged()).count() as i64,
        unstaged: files.iter().filter(|file| file.unstaged()).count() as i64,
        ..IndexSummary::default()
    };
    files.retain(|file| filter.includes(file));
    let active = files
        .iter()
        .find(|file| file.path == selected)
        .or_else(|| files.first());
    let (selected, body) = match active {
        Some(file) => {
            index.can_stage = IndexAction::Stage.allowed(file);
            index.can_unstage = IndexAction::Unstage.allowed(file);
            (file.path.clone(), patch(&root, file, filter, &mut index))
        }
        None => (String::new(), Ok(String::new())),
    };
    Ok(Review {
        base: root.to_string_lossy().into_owned(),
        status: if files.is_empty() {
            filter.empty_label().into()
        } else {
            format!("{} 个变更文件", files.len())
        },
        files: files
            .into_iter()
            .map(|file| {
                let status = match filter {
                    Filter::All => file.status.trim(),
                    Filter::Staged => &file.status[..1],
                    Filter::Unstaged if file.conflict() || file.status == "??" => &file.status,
                    Filter::Unstaged => &file.status[1..],
                }
                .to_owned();
                ReviewFile {
                    path: file.path,
                    status,
                }
            })
            .collect(),
        selected,
        body,
        index,
        comparison: None,
    })
}

fn apply(ui: &mut AgentWorkbench, result: Result<Review, String>) {
    let model = ui.model_review_files();
    match result {
        Ok(review) => {
            if let Some(metadata) = review.comparison {
                ui.set_review_comparison_label(metadata.caption);
                if let Some(target) = metadata.fixed_target {
                    ui.set_review_comparison_target(target);
                }
                ui.set_review_comparison_source_tree(metadata.source_tree);
            } else {
                ui.set_review_comparison_label(String::new());
                ui.set_review_comparison_source_tree(String::new());
            }
            review.index.publish(ui);
            ui.set_review_base(review.base);
            let count = review.files.len();
            for (index, row) in review.files.into_iter().enumerate() {
                if index < model.len() {
                    model.set_row(index, row);
                } else {
                    model.push(row);
                }
            }
            while model.len() > count {
                model.remove(model.len() - 1);
            }
            if ui.get_review_selection() != review.selected {
                ui.set_review_selection(review.selected);
                ui.set_review_offset(Length(0.0));
                ui.set_review_line_id(-1);
            }
            match review.body {
                Ok(body) => {
                    ui.set_review_status(review.status);
                    set_patch(ui, body);
                }
                Err(error) => {
                    ui.set_review_status(error);
                    set_patch(ui, String::new());
                }
            }
        }
        Err(error) => {
            ui.set_review_comparison_label(String::new());
            ui.set_review_comparison_source_tree(String::new());
            IndexSummary::default().publish(ui);
            model.clear();
            ui.set_review_selection(String::new());
            ui.set_review_base(String::new());
            ui.set_review_status(error);
            set_patch(ui, String::new());
        }
    }
}

enum Request {
    Read,
    ChangeView,
    Mutate(Mutation),
}

enum Mutation {
    FileIndex(IndexAction),
    HunkIndex(hunks::Selection),
    Revert(revert::Selection),
}

impl Mutation {
    fn label(&self) -> &'static str {
        match self {
            Self::FileIndex(action) => action.label(),
            Self::HunkIndex(selection) => selection.action.label(),
            Self::Revert(_) => "回退",
        }
    }

    fn apply(&self, root: &Path, path: &str) -> Result<(), String> {
        match self {
            Self::FileIndex(action) => update_index(root, path, *action),
            Self::HunkIndex(selection) => hunks::apply(root, path, selection),
            Self::Revert(selection) => selection.apply(root, path),
        }
    }
}

fn request(
    ui: &mut AgentWorkbench,
    revision: Rc<Cell<u64>>,
    output: &watch::Receiver<Snapshot>,
    selection: String,
    request: Request,
) {
    let workspace = ui.get_workspace();
    if !ui.get_review_open() || workspace.is_empty() {
        return;
    }
    let id = revision.get().wrapping_add(1);
    revision.set(id);
    if let Ok(index) = usize::try_from(ui.get_review_turn()) {
        let entry = {
            let snapshot = output.borrow();
            if snapshot.view.conversation != ui.get_conversation_id() {
                None
            } else {
                snapshot.view.entries.get(index).cloned()
            }
        };
        let result = match entry.as_deref() {
            Some(Entry::Changes(diff)) => Ok(turn_review(diff, &selection)),
            _ => Err("本轮改动已不可用".into()),
        };
        apply(ui, result);
        return;
    }
    let filter = Filter::from_ui(&ui.get_review_filter());
    let comparison = comparison::Scope::read(ui);
    let (change_view, change) = match request {
        Request::Mutate(change) => (false, Some(change)),
        Request::ChangeView => (true, None),
        Request::Read => (false, None),
    };
    let action = change.as_ref().map(Mutation::label);
    if action.is_none() {
        ui.set_review_can_stage(false);
        ui.set_review_can_unstage(false);
        ui.set_review_can_stage_hunks(false);
        ui.set_review_can_unstage_hunks(false);
    }
    ui.set_review_comparison_source_tree(String::new());
    if change_view {
        ui.model_review_files().clear();
        ui.set_review_line_id(-1);
        set_patch(ui, String::new());
    } else if selection != ui.get_review_selection() {
        ui.set_review_selection(selection.clone());
        set_patch(ui, String::new());
    }
    if let Some(action) = action {
        ui.set_review_index_busy(true);
        ui.set_review_status(format!("正在{action}…"));
    } else {
        ui.set_review_status("正在读取 Git 改动…".into());
    }
    let weak = ui.as_weak();
    if let Err(error) = xrack_platform_winit::spawn_local(async move {
        let (sender, receiver) = oneshot::channel();
        let workdir = PathBuf::from(&workspace);
        let scope = comparison.clone();
        let worker = thread::Builder::new()
            .name("workbench-review".into())
            .spawn(move || {
                let error = change.and_then(|change| {
                    change
                        .apply(&workdir, &selection)
                        .err()
                        .map(|error| format!("{}失败（{error}）", change.label()))
                });
                let review = if let Some(scope) = scope {
                    comparison::load(&workdir, &selection, &scope)
                } else {
                    load(&workdir, &selection, filter)
                };
                let _ = sender.send((review, error));
            });
        let (result, mutation_error) = match worker {
            Ok(_) => receiver
                .await
                .unwrap_or_else(|_| (Err("Git 操作任务已退出".into()), None)),
            Err(error) => (Err(format!("无法启动 Git 操作：{error}")), None),
        };
        let Some(mut ui) = weak.upgrade() else { return };
        if action.is_some() {
            ui.set_review_index_busy(false);
        }
        if revision.get() == id
            && ui.get_review_open()
            && ui.get_review_turn() < 0
            && ui.get_workspace() == workspace
            && Filter::from_ui(&ui.get_review_filter()) == filter
            && comparison::Scope::read(&ui) == comparison
        {
            apply(&mut ui, result);
            if let Some(error) = mutation_error {
                ui.set_review_status(error);
            }
        } else if action.is_some()
            && ui.get_review_open()
            && !ui.get_files_active()
            && ui.get_review_turn() < 0
            && ui.get_workspace() == workspace
        {
            ui.invoke_refresh_review();
        }
    }) {
        if action.is_some() {
            ui.set_review_index_busy(false);
        }
        ui.set_review_status(error.to_string());
    }
}

struct RevertProposal {
    workspace: String,
    conversation: i64,
    path: String,
    document: StyledText,
    selection: revert::Selection,
}

impl RevertProposal {
    fn visible_patch_is_current(&self, ui: &AgentWorkbench) -> bool {
        self.workspace == ui.get_workspace()
            && self.conversation == ui.get_conversation_id()
            && self.path == ui.get_review_selection()
            && self.document.text() == ui.get_review_body().text()
    }
}

fn propose_revert(ui: &mut AgentWorkbench, row: Option<i64>) -> Option<RevertProposal> {
    if !can_update_index(ui)
        || !ui.get_review_can_stage_hunks()
        || ui.get_review_filter() == "staged"
        || ui.get_review_revert_open()
    {
        return None;
    }
    let path = ui.get_review_selection();
    if path.is_empty() || !crate::workspace_files::allowed_relative(Path::new(&path)) {
        return None;
    }
    let document = ui.get_review_body();
    let selection = if let Some(row) = row {
        if ui.get_review_raw() {
            return None;
        }
        revert::Selection::hunk(document.text(), usize::try_from(row).ok()?)?
    } else {
        revert::Selection::file(document.text())?
    };
    ui.set_review_revert_path(path.clone());
    ui.set_review_revert_file(row.is_none());
    ui.set_review_revert_preview(highlight(selection.preview(), ui.get_dark_appearance()));
    ui.set_review_revert_error(String::new());
    ui.set_review_revert_open(true);
    Some(RevertProposal {
        workspace: ui.get_workspace(),
        conversation: ui.get_conversation_id(),
        path,
        document,
        selection,
    })
}

pub fn workspace_changed(ui: &mut AgentWorkbench) {
    ui.set_review_comparison_mode(String::new());
    ui.set_review_comparison_target(String::new());
    ui.set_review_comparison_label(String::new());
    ui.set_review_comparison_source_tree(String::new());
    IndexSummary::default().publish(ui);
    ui.set_review_turn(-1);
    ui.model_review_files().clear();
    ui.set_review_selection(String::new());
    ui.set_review_base(String::new());
    set_patch(ui, String::new());
    ui.set_review_status(String::new());
    ui.set_review_offset(Length(0.0));
    if ui.get_review_open() && !ui.get_files_active() {
        ui.invoke_refresh_review();
    }
}

fn open_comparison_source(ui: &mut AgentWorkbench) {
    let tree = ui.get_review_comparison_source_tree();
    let path = ui.get_review_selection();
    let base = ui.get_review_base();
    if tree.is_empty()
        || path.is_empty()
        || base.is_empty()
        || !ui.get_review_open()
        || comparison::Scope::read(ui).is_none()
    {
        return;
    }
    ui.set_review_source_title(format!(
        "{path} · {}",
        tree.chars().take(8).collect::<String>()
    ));
    ui.set_review_source_document(StyledText::from_plain_text("正在读取 Git 源码…"));
    ui.set_review_source_open(true);
    let weak = ui.as_weak();
    if let Err(error) = xrack_platform_winit::spawn_local(async move {
        let target = tree.clone();
        let file = path.clone();
        let root = PathBuf::from(&base);
        let document = crate::read_in_background("workbench-git-source", move || {
            comparison::source(&root, &target, &file)
        })
        .await;
        if let Some(mut ui) = weak.upgrade()
            && ui.get_review_source_open()
            && ui.get_review_comparison_source_tree() == tree
            && ui.get_review_selection() == path
            && ui.get_review_base() == base
        {
            ui.set_review_source_document(StyledText::from_plain_text(
                document.unwrap_or_else(std::convert::identity),
            ));
        }
    }) {
        ui.set_review_source_document(StyledText::from_plain_text(error.to_string()));
    }
}

fn can_update_index(ui: &AgentWorkbench) -> bool {
    ui.get_review_open()
        && ui.get_review_turn() < 0
        && ui.get_review_comparison_mode().is_empty()
        && !ui.get_files_active()
        && !ui.get_review_index_busy()
        && !ui.get_worktree_preparing()
        && !ui.get_handoff_preparing()
        && !ui.get_handoff_pending()
        && !ui.get_switching()
}

pub fn attach(ui: &mut AgentWorkbench, output: watch::Receiver<Snapshot>) {
    branches::attach(ui, output.clone());
    commit::attach(ui, output.clone());
    push::attach(ui);
    pull_request::attach(ui);
    find::attach(ui);
    line_edit::attach(ui);
    notes::attach(ui);
    let revision = Rc::new(Cell::new(0));
    let weak = ui.as_weak();
    let refresh_revision = revision.clone();
    let refresh_output = output.clone();
    ui.on_refresh_review(move || {
        if let Some(mut ui) = weak.upgrade() {
            let path = ui.get_review_selection();
            request(
                &mut ui,
                refresh_revision.clone(),
                &refresh_output,
                path,
                Request::Read,
            );
        }
    });
    let weak = ui.as_weak();
    let filter_revision = revision.clone();
    let filter_output = output.clone();
    ui.on_select_review_filter(move || {
        if let Some(mut ui) = weak.upgrade() {
            let path = ui.get_review_selection();
            request(
                &mut ui,
                filter_revision.clone(),
                &filter_output,
                path,
                Request::ChangeView,
            );
        }
    });
    let weak = ui.as_weak();
    let comparison_revision = revision.clone();
    let comparison_output = output.clone();
    ui.on_start_review_comparison(move |mode, target| {
        let Some(mut ui) = weak.upgrade() else { return };
        if ui.get_switching()
            || ui.get_review_index_busy()
            || !ui.get_review_open()
            || !ui.get_review_scope_open()
            || !ui.get_review_scope_compare()
            || !matches!(mode.as_str(), "branch" | "commit")
            || target.trim().is_empty()
        {
            return;
        }
        ui.set_review_scope_open(false);
        ui.set_review_turn(-1);
        ui.set_files_active(false);
        ui.set_review_comparison_mode(mode);
        ui.set_review_comparison_target(target);
        ui.set_review_comparison_label(String::new());
        ui.set_review_offset(Length(0.0));
        let path = ui.get_review_selection();
        request(
            &mut ui,
            comparison_revision.clone(),
            &comparison_output,
            path,
            Request::ChangeView,
        );
    });
    let weak = ui.as_weak();
    ui.on_open_comparison_source(move || {
        if let Some(mut ui) = weak.upgrade() {
            open_comparison_source(&mut ui);
        }
    });
    let weak = ui.as_weak();
    let action_revision = revision.clone();
    let action_output = output.clone();
    ui.on_update_review_index(move |stage| {
        let Some(mut ui) = weak.upgrade() else { return };
        let filter = Filter::from_ui(&ui.get_review_filter());
        let allowed = if stage {
            filter != Filter::Staged && ui.get_review_can_stage()
        } else {
            filter != Filter::Unstaged && ui.get_review_can_unstage()
        };
        let path = ui.get_review_selection();
        if allowed && !path.is_empty() && can_update_index(&ui) {
            let action = if stage {
                IndexAction::Stage
            } else {
                IndexAction::Unstage
            };
            request(
                &mut ui,
                action_revision.clone(),
                &action_output,
                path,
                Request::Mutate(Mutation::FileIndex(action)),
            );
        }
    });
    let weak = ui.as_weak();
    let hunk_revision = revision.clone();
    let hunk_output = output.clone();
    ui.on_update_review_hunk(move |id| {
        let Some(mut ui) = weak.upgrade() else { return };
        if !can_update_index(&ui) || ui.get_review_raw() {
            return;
        }
        let path = ui.get_review_selection();
        let Some(selection) = usize::try_from(id)
            .ok()
            .and_then(|index| hunks::select(ui.get_review_body().text(), index))
        else {
            return;
        };
        let allowed = match selection.action {
            IndexAction::Stage => {
                ui.get_review_filter() != "staged" && ui.get_review_can_stage_hunks()
            }
            IndexAction::Unstage => {
                ui.get_review_filter() != "unstaged" && ui.get_review_can_unstage_hunks()
            }
        };
        if allowed && !path.is_empty() {
            request(
                &mut ui,
                hunk_revision.clone(),
                &hunk_output,
                path,
                Request::Mutate(Mutation::HunkIndex(selection)),
            );
        }
    });
    let proposal = Rc::new(RefCell::new(None::<RevertProposal>));
    let weak = ui.as_weak();
    let pending = proposal.clone();
    ui.on_propose_review_file_revert(move || {
        if let Some(mut ui) = weak.upgrade()
            && let Some(selection) = propose_revert(&mut ui, None)
        {
            pending.replace(Some(selection));
        }
    });
    let weak = ui.as_weak();
    let pending = proposal.clone();
    ui.on_propose_review_hunk_revert(move |id| {
        if let Some(mut ui) = weak.upgrade()
            && let Some(selection) = propose_revert(&mut ui, Some(id))
        {
            pending.replace(Some(selection));
        }
    });
    let weak = ui.as_weak();
    let revert_revision = revision.clone();
    let revert_output = output.clone();
    ui.on_confirm_review_revert(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        if !ui.get_review_revert_open() || !ui.get_review_revert_error().is_empty() {
            return;
        }
        let Some(proposal) = proposal.borrow_mut().take() else {
            return;
        };
        if !can_update_index(&ui)
            || !ui.get_review_can_stage_hunks()
            || ui.get_review_filter() == "staged"
            || !proposal.visible_patch_is_current(&ui)
        {
            ui.set_review_revert_error("差异已变化，请刷新后重新选择".into());
            return;
        }
        ui.set_review_revert_open(false);
        request(
            &mut ui,
            revert_revision.clone(),
            &revert_output,
            proposal.path,
            Request::Mutate(Mutation::Revert(proposal.selection)),
        );
    });
    let weak = ui.as_weak();
    ui.on_select_review_file(move |path| {
        if let Some(mut ui) = weak.upgrade() {
            request(&mut ui, revision.clone(), &output, path, Request::Read);
        }
    });
    let weak = ui.as_weak();
    ui.on_select_review_line(move |id| {
        if let Some(mut ui) = weak.upgrade() {
            select_line(&mut ui, id);
        }
    });
    let weak = ui.as_weak();
    ui.on_select_review_side(move |id, old| {
        if let Some(mut ui) = weak.upgrade() {
            select_line_side(&mut ui, id, Some(old));
        }
    });
    let weak = ui.as_weak();
    ui.on_toggle_review_split(move || {
        if let Some(mut ui) = weak.upgrade() {
            split::toggle(&mut ui);
        }
    });
    let weak = ui.as_weak();
    ui.on_add_review_note(move || {
        if let Some(mut ui) = weak.upgrade() {
            add_note(&mut ui);
        }
    });
    let weak = ui.as_weak();
    ui.on_cancel_review_note(move || {
        if let Some(mut ui) = weak.upgrade() {
            NoteDraft::default().restore(&mut ui);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use xrack_runtime::ColorScheme;

    #[test]
    fn changing_appearance_recolors_a_loaded_diff_without_replacing_its_rows() {
        let mut ui = AgentWorkbench::new();
        crate::appearance::attach(&mut ui, None);
        set_patch(&mut ui, "@@ -1 +1 @@\n-old\n+new\n".into());
        let light = ui.get_review_body();
        let removed = ui.model_review_lines().row_key(1).unwrap();
        crate::appearance::update_system(&mut ui, ColorScheme::Dark);
        let dark = ui.get_review_body();
        assert_eq!(light.text(), dark.text());
        assert_ne!(light.runs()[1].style.color, dark.runs()[1].style.color);
        assert_eq!(dark.runs()[2].style.color, Some(Color(0x7ed9acff)));
        assert_eq!(ui.model_review_lines().row_key(1), Some(removed));

        ui.invoke_select_appearance(ColorScheme::Light);
        assert_eq!(ui.get_review_body(), light);
    }

    #[test]
    fn numbers_both_sides_and_keeps_patch_sections_distinct() {
        let rows = numbered_lines(concat!(
            "已暂存\ndiff --git a/path b/path\n--- a/path\n+++ b/path\n",
            "@@ -9,2 +20,2 @@\n-old\n+new\n kept\n\\ No newline at end of file\n",
            "未暂存\ndiff --git a/path b/path\n@@ -30 +40 @@\n-older\n+newer\n",
            "@@ -9223372036854775807 +5 @@\n-not numbered\n",
        ));
        let actual: Vec<_> = rows
            .iter()
            .filter(|line| line.old_line > 0 || line.new_line > 0)
            .map(|line| {
                (
                    line.section.as_str(),
                    line.content.as_str(),
                    line.old_line,
                    line.new_line,
                )
            })
            .collect();
        assert_eq!(
            actual,
            [
                ("已暂存", "old", 9, 0),
                ("已暂存", "new", 0, 20),
                ("已暂存", "kept", 10, 21),
                ("未暂存", "older", 30, 0),
                ("未暂存", "newer", 0, 40),
            ]
        );
        let untracked = numbered_lines("未跟踪文件\n+one\n++two\n");
        assert_eq!(untracked[1].new_line, 1);
        assert_eq!(untracked[2].new_line, 2);
        assert_eq!(untracked[2].content, "+two");
    }

    #[test]
    fn paired_context_line_can_obtain_an_old_or_new_location() {
        let mut ui = AgentWorkbench::new();
        ui.set_review_open(true);
        ui.set_review_selection("src/lib.rs".into());
        set_patch(&mut ui, "@@ -9,2 +20,2 @@\n-old\n+new\n kept\n".into());
        select_line_side(&mut ui, 3, Some(true));
        assert!(ui.get_review_line_old());
        assert_eq!(
            ui.get_review_note_reference(),
            "src/lib.rs · 旧行 10 · 本轮改动"
        );
        select_line_side(&mut ui, 3, Some(false));
        assert!(!ui.get_review_line_old());
        assert_eq!(
            ui.get_review_note_reference(),
            "src/lib.rs · 新行 21 · 本轮改动"
        );
        select_line_side(&mut ui, 1, Some(false));
        assert_eq!(ui.get_review_line_id(), 3);
    }

    #[test]
    fn opens_and_refreshes_the_selected_turn_without_reading_git() {
        let mut snapshot = Snapshot::connecting("/not-a-git-workspace".into());
        let mut window = AgentWorkbench::new();
        window.set_workspace(snapshot.view.workspace.clone());
        snapshot.view.entries.push(Arc::new(Entry::Changes(TurnDiff::parse(
            "diff --git a/before.txt b/before.txt\n--- a/before.txt\n+++ b/before.txt\n@@ -1 +1 @@\n-old\n+new\n",
        ))));
        let (sender, receiver) = watch::channel(snapshot.clone());
        attach(&mut window, receiver);
        window.invoke_show_turn_review(0);
        assert!(window.get_review_open());
        assert_eq!(window.get_review_selection(), "before.txt");
        assert_eq!(window.get_review_status(), "1 个文件 · +1 −1");
        assert!(window.get_review_body().text().contains("+new"));
        let added = window
            .model_review_lines()
            .snapshot()
            .into_iter()
            .find(|row| row.kind == "add")
            .unwrap()
            .id;
        window.invoke_select_review_line(added);
        assert_eq!(
            window.get_review_note_reference(),
            "before.txt · 新行 1 · 本轮改动"
        );
        window.set_review_note_text("请说明这个改动的原因".into());

        snapshot.view.entries[0] = Arc::new(Entry::Changes(TurnDiff::parse(concat!(
            "diff --git a/unrelated b/unrelated\nnew file mode 100644\n",
            "diff --git a/before.txt b/before.txt\n--- a/before.txt\n+++ b/before.txt\n",
            "@@ -1 +1 @@\n-old\n+updated\n",
        ))));
        sender.send_replace(snapshot.clone());
        window.invoke_refresh_review();
        assert_eq!(window.get_review_selection(), "before.txt");
        assert!(window.get_review_body().text().contains("+updated"));
        window.invoke_select_review_file("unrelated".into());
        assert_eq!(window.get_review_selection(), "unrelated");
        assert!(window.get_review_body().text().contains("new file mode"));
        assert!(!window.get_review_body().text().contains("+updated"));
        let pending = NoteDraft::capture(&window);
        window.invoke_cancel_review_note();
        assert!(window.get_review_note_reference().is_empty());
        pending.restore(&mut window);
        window.set_draft("现有草稿".into());
        window.invoke_add_review_note();
        assert_eq!(
            window.get_draft(),
            "现有草稿\n\n审阅批注（before.txt · 新行 1 · 本轮改动）\n请说明这个改动的原因"
        );
        assert!(window.get_review_note_text().is_empty());
        assert!(window.get_review_note_reference().is_empty());

        snapshot.view.conversation = 1;
        sender.send_replace(snapshot);
        window.invoke_refresh_review();
        assert!(window.get_review_body().text().is_empty());
        assert!(window.model_review_files().is_empty());
    }

    fn run_git(root: &Path, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    }

    fn commit_fixture(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
        run_git(
            root,
            &[
                "-c",
                "user.name=XRack",
                "-c",
                "user.email=example@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                "fixture",
            ],
        )
    }

    #[test]
    fn lists_real_changes_and_separates_the_index_from_the_worktree()
    -> Result<(), Box<dyn std::error::Error>> {
        let scratch = tempfile::tempdir()?;
        let root = scratch.path().canonicalize()?;
        run_git(&root, &["init", "--quiet"])?;
        fs::write(root.join("draft file.rs"), "before\n")?;
        run_git(&root, &["add", "--", "draft file.rs"])?;
        commit_fixture(&root)?;
        fs::write(root.join("draft file.rs"), "staged\n")?;
        run_git(&root, &["add", "--", "draft file.rs"])?;
        fs::write(root.join("draft file.rs"), "working\n")?;
        fs::write(root.join("new file.rs"), "new content\n")?;
        fs::create_dir(root.join("subfolder"))?;

        let workspace = root.join("subfolder");
        let review = load(&workspace, "draft file.rs", Filter::All)?;
        assert_eq!(review.files[0].status, "MM");
        assert_eq!(review.files.len(), 2);
        let staged = review.body?;
        assert!(staged.contains("-before\n+staged\n"));
        assert!(staged.contains("-staged\n+working\n"));
        let index = load(&workspace, "draft file.rs", Filter::Staged)?;
        assert_eq!(index.files.len(), 1);
        assert_eq!(index.files[0].status, "M");
        assert_eq!(
            (index.index.all, index.index.staged, index.index.unstaged),
            (2, 1, 2)
        );
        let index_patch = index.body?;
        assert!(index_patch.contains("-before\n+staged\n"));
        assert!(!index_patch.contains("+working"));
        let working = load(&workspace, "draft file.rs", Filter::Unstaged)?;
        assert_eq!(working.files.len(), 2);
        assert_eq!(working.files[0].status, "M");
        let working_patch = working.body?;
        assert!(working_patch.contains("-staged\n+working\n"));
        assert!(!working_patch.contains("-before"));
        let rows = numbered_lines(&staged);
        assert!(
            rows.iter()
                .any(|row| row.section == "已暂存" && row.content == "before" && row.old_line == 1)
        );
        assert!(
            rows.iter().any(|row| row.section == "未暂存"
                && row.content == "working"
                && row.new_line == 1)
        );
        let mut window = AgentWorkbench::new();
        window.set_workspace(workspace.to_string_lossy().into_owned());
        window.set_review_open(true);
        apply(&mut window, load(&workspace, "draft file.rs", Filter::All));
        let removed = rows.iter().find(|row| row.kind == "remove").unwrap();
        select_line(&mut window, removed.id);
        assert_eq!(
            window.get_review_note_reference(),
            format!("{} · 旧行 1 · 已暂存", root.join("draft file.rs").display())
        );
        assert_eq!(
            load(&root, "new file.rs", Filter::All)?.body?,
            "未跟踪文件\n+new content\n"
        );
        assert!(!highlight(&staged, false).runs().is_empty());
        fs::remove_file(root.join("new file.rs"))?;
        assert_eq!(
            load(&root, "new file.rs", Filter::All)?.selected,
            "draft file.rs"
        );
        Ok(())
    }

    #[test]
    fn index_actions_only_change_the_selected_literal_path_on_an_unborn_branch()
    -> Result<(), Box<dyn std::error::Error>> {
        let scratch = tempfile::tempdir()?;
        let root = scratch.path().canonicalize()?;
        run_git(&root, &["init", "--quiet"])?;
        let selected = "-literal[1].rs";
        fs::write(root.join(selected), "original contents\n")?;
        fs::write(root.join("-literal1.rs"), "another file\n")?;
        let workspace = root.to_string_lossy().into_owned();
        let mut ui = AgentWorkbench::new();
        ui.set_workspace(workspace.clone());
        ui.set_review_open(true);
        apply(&mut ui, load(&root, selected, Filter::All));
        let (_sender, output) = watch::channel(Snapshot::connecting(workspace));
        attach(&mut ui, output);
        // The platform queues the operation before starting the Git worker.
        ui.invoke_update_review_index(true);
        assert!(!ui.get_review_index_busy());
        assert!(load(&root, selected, Filter::Staged)?.files.is_empty());
        update_index(&root, selected, IndexAction::Stage)?;
        assert!(update_index(&root, selected, IndexAction::Stage).is_err());
        let staged = load(&root, selected, Filter::Staged)?;
        assert_eq!(staged.files.len(), 1);
        assert_eq!(staged.selected, selected);
        assert!(staged.index.can_unstage);
        assert_eq!(staged.index.unstaged, 1);
        update_index(&root, selected, IndexAction::Unstage)?;
        assert!(load(&root, selected, Filter::Staged)?.files.is_empty());
        assert_eq!(
            fs::read_to_string(root.join(selected))?,
            "original contents\n"
        );
        assert_eq!(load(&root, selected, Filter::Unstaged)?.files.len(), 2);
        Ok(())
    }

    #[test]
    fn compares_diverged_branches_and_reads_deleted_files_from_the_commit_parent()
    -> Result<(), Box<dyn std::error::Error>> {
        use comparison::Scope;
        let scratch = tempfile::tempdir()?;
        let root = scratch.path().canonicalize()?;
        run_git(&root, &["init", "--quiet", "-b", "base"])?;
        fs::write(root.join("note.txt"), "original\n")?;
        run_git(&root, &["add", "--", "note.txt"])?;
        commit_fixture(&root)?;
        let initial = crate::review_reports::resolve_commit(&root, "HEAD")?;

        run_git(&root, &["switch", "--quiet", "-c", "feature"])?;
        run_git(&root, &["mv", "--", "note.txt", "renamed.txt"])?;
        commit_fixture(&root)?;
        let renamed = crate::review_reports::resolve_commit(&root, "HEAD")?;
        run_git(&root, &["rm", "--", "renamed.txt"])?;
        commit_fixture(&root)?;
        let deletion = crate::review_reports::resolve_commit(&root, "HEAD")?;

        run_git(&root, &["switch", "--quiet", "base"])?;
        fs::write(root.join("base-only.txt"), "other branch\n")?;
        run_git(&root, &["add", "--", "base-only.txt"])?;
        commit_fixture(&root)?;
        run_git(&root, &["switch", "--quiet", "feature"])?;
        fs::write(root.join("working.txt"), "only on disk\n")?;

        let branch = comparison::load(&root, "", &Scope::Branch("base".into()))?;
        assert_eq!(branch.files.len(), 1);
        assert_eq!(branch.selected, "note.txt");
        assert_eq!(branch.files[0].status, "D");
        assert!(branch.body?.contains("-original"));
        let comparison = branch.comparison.unwrap();
        assert_eq!(comparison.source_tree, initial);
        assert_eq!(
            comparison::source(&root, &comparison.source_tree, "note.txt")?,
            "original\n"
        );

        let rename = comparison::load(&root, "renamed.txt", &Scope::Commit(renamed.clone()))?;
        assert_eq!(rename.files[0].path, "renamed.txt");
        assert_eq!(rename.files[0].status, "R");
        assert!(rename.body?.contains("rename from note.txt"));
        assert_eq!(rename.comparison.unwrap().source_tree, renamed);

        let removed = comparison::load(&root, "renamed.txt", &Scope::Commit("HEAD".into()))?;
        let comparison = removed.comparison.unwrap();
        assert_eq!(comparison.fixed_target.as_deref(), Some(deletion.as_str()));
        assert_eq!(comparison.source_tree, renamed);
        assert_eq!(
            comparison::source(&root, &comparison.source_tree, "renamed.txt")?,
            "original\n"
        );
        assert!(!removed.index.can_stage && !removed.index.can_unstage);
        assert_eq!(
            fs::read_to_string(root.join("working.txt"))?,
            "only on disk\n"
        );

        let root_commit = comparison::load(&root, "", &Scope::Commit(initial))?;
        assert_eq!(root_commit.files[0].status, "A");
        assert!(root_commit.body?.contains("+original"));
        assert!(comparison::load(&root, "", &Scope::Commit("--invalid".into())).is_err());

        run_git(
            &root,
            &[
                "-c",
                "user.name=XRack",
                "-c",
                "user.email=example@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "merge",
                "--no-edit",
                "--no-ff",
                "base",
            ],
        )?;
        let merge = comparison::load(&root, "", &Scope::Commit("HEAD".into()))?;
        assert_eq!(merge.files.len(), 1);
        assert_eq!(merge.files[0].path, "base-only.txt");
        assert!(merge.body?.contains("+other branch"));
        Ok(())
    }

    #[test]
    fn unstaging_a_rename_restores_both_index_paths_and_keeps_the_renamed_file()
    -> Result<(), Box<dyn std::error::Error>> {
        let scratch = tempfile::tempdir()?;
        let root = scratch.path().canonicalize()?;
        run_git(&root, &["init", "--quiet"])?;
        fs::write(root.join("old.rs"), "same contents\n")?;
        run_git(&root, &["add", "--", "old.rs"])?;
        commit_fixture(&root)?;
        run_git(&root, &["mv", "--", "old.rs", "renamed.rs"])?;
        let staged = load(&root, "renamed.rs", Filter::Staged)?;
        assert_eq!(staged.files[0].status, "R");
        update_index(&root, "renamed.rs", IndexAction::Unstage)?;
        run_git(&root, &["diff", "--cached", "--quiet"])?;
        assert!(!root.join("old.rs").exists());
        assert_eq!(
            fs::read_to_string(root.join("renamed.rs"))?,
            "same contents\n"
        );
        assert_eq!(load(&root, "", Filter::Unstaged)?.files.len(), 2);
        Ok(())
    }

    #[test]
    fn stages_and_unstages_individual_hunks_without_changing_other_lines()
    -> Result<(), Box<dyn std::error::Error>> {
        let scratch = tempfile::tempdir()?;
        let root = scratch.path().canonicalize()?;
        run_git(&root, &["init", "--quiet"])?;
        let path = "review draft.txt";
        let rows: Vec<_> = (1..=60)
            .map(|line| format!("original line {line:02}\n"))
            .collect();
        let original = rows.concat();
        fs::write(root.join(path), &original)?;
        run_git(&root, &["add", "--", path])?;
        commit_fixture(&root)?;
        run_git(&root, &["config", "apply.whitespace", "fix"])?;
        let mut working = rows.clone();
        working[2] = "first change   \n".into();
        working[23] = "second change\n".into();
        working[48] = "third change\n".into();
        let body = working.concat();
        fs::write(root.join(path), &body)?;
        let index_text =
            || git(&root, &["show", &format!(":{path}")], 4096).map(|(bytes, _)| bytes);
        let hunk = |patch: &str, section: &str, offset| {
            let row = numbered_lines(patch)
                .into_iter()
                .filter(|line| line.section == section && line.content.starts_with("@@ "))
                .nth(offset)
                .unwrap();
            hunks::select(patch, row.id as usize).unwrap()
        };
        let view = load(&root, path, Filter::Unstaged)?;
        assert!(view.index.can_stage_hunks && !view.index.can_unstage_hunks);
        let first = view.body?;
        let stale = hunk(&first, "未暂存", 1);
        hunks::apply(&root, path, &hunk(&first, "未暂存", 0))?;

        let mut expected = rows.clone();
        expected[2].clone_from(&working[2]);
        assert_eq!(index_text()?, expected.concat().as_bytes());
        assert!(
            hunks::apply(&root, path, &stale)
                .unwrap_err()
                .contains("差异已变化")
        );
        assert_eq!(index_text()?, expected.concat().as_bytes());

        let mixed = load(&root, path, Filter::All)?;
        assert!(mixed.index.can_stage_hunks && mixed.index.can_unstage_hunks);
        hunks::apply(&root, path, &hunk(&mixed.body?, "未暂存", 0))?;
        expected[23].clone_from(&working[23]);
        assert_eq!(index_text()?, expected.concat().as_bytes());
        let mixed = load(&root, path, Filter::All)?;
        let mut ui = AgentWorkbench::new();
        apply(&mut ui, Ok(mixed));
        let display = ui.get_review_body();
        hunks::apply(&root, path, &hunk(display.text(), "已暂存", 0))?;
        expected[2].clone_from(&rows[2]);
        assert_eq!(index_text()?, expected.concat().as_bytes());
        assert_eq!(fs::read_to_string(root.join(path))?, body);
        Ok(())
    }

    #[test]
    fn parses_porcelain_renames_and_caps_preview_bytes() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            parse_changes(b"R  new name\0old name\0?? folder/x\0")?,
            [
                Change {
                    status: "R ".into(),
                    path: "new name".into(),
                    previous_path: Some("old name".into())
                },
                Change {
                    status: "??".into(),
                    path: "folder/x".into(),
                    previous_path: None
                },
            ]
        );
        assert!(parse_changes(b"R  missing\0").is_err());
        assert_eq!(read_limited(&b"large"[..], 3)?, (b"lar".to_vec(), true));
        Ok(())
    }

    #[test]
    fn handles_binary_files_and_an_unborn_branch() -> Result<(), Box<dyn std::error::Error>> {
        let scratch = tempfile::tempdir()?;
        let root = scratch.path().canonicalize()?;
        run_git(&root, &["init", "--quiet"])?;
        assert!(load(&root, "", Filter::All)?.files.is_empty());
        fs::write(root.join("binary.data"), [0, 1, 2, 3])?;
        assert_eq!(
            load(&root, "binary.data", Filter::All)?.body?,
            "新增二进制文件"
        );
        fs::write(root.join("initial.txt"), "first\n")?;
        run_git(&root, &["add", "--", "initial.txt"])?;
        let review = load(&root, "initial.txt", Filter::All)?;
        assert_eq!(review.selected, "initial.txt");
        assert!(review.body?.contains("已暂存\n"));
        Ok(())
    }
}
