mod editor;
pub(crate) use editor::{read_text, save_text};

use crate::{
    attachments::{Attachments, PNG_PREVIEW_LIMIT, preview_png},
    file_references::{FileReference, LineRange},
    read_in_background,
    review::{git_root, read_limited},
};
use session::{ExcerptOrigin, FileExcerpt};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fmt::Write,
    fs::{self, File},
    io::{self, BufRead, BufReader, Read},
    ops::Range,
    path::{Component, Path, PathBuf},
    rc::Rc,
    time::SystemTime,
};
use workbench_ui::{AgentWorkbench, WorkspaceFile, WorkspacePreviewTab};
use xrack_runtime::{Image, Length, StyledRun, StyledText, TextStyle};

const DIRECTORY_LIMIT: usize = 4096;
const TEXT_LIMIT: usize = 96 * 1024;
const LINE_LIMIT: usize = 1200;
const SOURCE_SCAN_LIMIT: u64 = 8 * 1024 * 1024;
const EXCERPT_LINES: usize = 80;
const PREVIEW_CACHE_SIZE: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
enum FileOrigin {
    Workspace,
    Diff,
    Reference,
}

struct FileRequest {
    path: String,
    origin: FileOrigin,
    lines: Option<LineRange>,
    column: Option<usize>,
}

struct LocatedFile {
    absolute: PathBuf,
    relative: String,
    stamp: (u64, Option<SystemTime>),
}

struct FileTab {
    id: i64,
    file: LocatedFile,
    lines: Option<LineRange>,
    column: Option<usize>,
    offset: Length,
    preview: Option<Result<Preview, String>>,
    reading: bool,
    revision: u64,
    last_used: u64,
    editor: editor::Editor,
}

impl FileTab {
    fn row(&self) -> WorkspacePreviewTab {
        let name = Path::new(&self.file.relative)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&self.file.relative);
        WorkspacePreviewTab {
            id: self.id.to_string(),
            title: name.into(),
            path: self.file.relative.clone(),
            dirty: self.editor.dirty(),
        }
    }

    fn invalidate(&mut self) {
        self.preview = None;
        self.reading = false;
        self.revision = self.revision.wrapping_add(1);
    }

    fn position(&mut self, lines: Option<LineRange>, column: Option<usize>) {
        self.column = column;
        if self.lines != lines {
            self.lines = lines;
            self.offset = Length(0.0);
            self.invalidate();
        }
    }
}

#[derive(Default)]
struct OpenFiles {
    tabs: Vec<FileTab>,
    active: Option<i64>,
}

impl OpenFiles {
    fn index(&self, id: i64) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == id)
    }

    fn get_mut(&mut self, id: i64) -> Option<&mut FileTab> {
        self.tabs.iter_mut().find(|tab| tab.id == id)
    }

    fn active(&self) -> Option<&FileTab> {
        self.tabs.iter().find(|tab| Some(tab.id) == self.active)
    }

    fn remember_offset(&mut self, offset: Length) {
        if let Some(id) = self.active
            && let Some(tab) = self.get_mut(id)
            && matches!(tab.preview, Some(Ok(Preview::Text { .. })))
        {
            tab.offset = offset;
        }
    }

    fn close(&mut self, id: i64) -> Option<(usize, FileTab)> {
        let index = self.index(id)?;
        let closed = self.tabs.remove(index);
        if self.active == Some(id) {
            self.active = self
                .tabs
                .get(index)
                .or_else(|| self.tabs.last())
                .map(|tab| tab.id);
        }
        Some((index, closed))
    }

    fn reorder(&mut self, from: usize, to: usize) {
        if from < self.tabs.len() && to < self.tabs.len() && from != to {
            let tab = self.tabs.remove(from);
            self.tabs.insert(to, tab);
        }
    }

    fn prune_previews(&mut self) {
        let mut cached = self.tabs.iter().filter(|tab| tab.preview.is_some()).count();
        let active = self.active;
        while cached > PREVIEW_CACHE_SIZE {
            let Some(tab) = self
                .tabs
                .iter_mut()
                .filter(|tab| Some(tab.id) != active && tab.preview.is_some())
                .min_by_key(|tab| tab.last_used)
            else {
                break;
            };
            tab.preview = None;
            cached -= 1;
        }
    }
}

#[derive(Default)]
struct Browser {
    workspace: String,
    generation: u64,
    intent: u64,
    files: OpenFiles,
    previous: HashMap<String, OpenFiles>,
    closed_at: HashMap<PathBuf, u64>,
    next_key: i64,
    loading: HashSet<i64>,
    loaded: HashSet<i64>,
    edit_token: u64,
    pending_edit_close: Option<(i64, editor::CloseTarget)>,
}

impl Browser {
    fn key(&mut self) -> i64 {
        let key = self.next_key;
        self.next_key += 1;
        key
    }

    fn next_intent(&mut self) -> u64 {
        self.intent = self.intent.wrapping_add(1);
        self.intent
    }

    fn reset(&mut self, ui: &mut AgentWorkbench) {
        let workspace = ui.get_workspace();
        self.remember_offset(ui);
        for tab in &mut self.files.tabs {
            tab.invalidate();
        }
        if workspace != self.workspace {
            let old = std::mem::replace(&mut self.workspace, workspace);
            let current = self.previous.remove(&self.workspace).unwrap_or_default();
            let previous = std::mem::replace(&mut self.files, current);
            if !previous.tabs.is_empty() {
                self.previous.insert(old, previous);
            }
        }
        self.generation = self.generation.wrapping_add(1);
        self.pending_edit_close = None;
        ui.set_workspace_edit_confirm_open(false);
        self.loaded.clear();
        self.loading.clear();
        self.closed_at.clear();
        ui.model_workspace_files().clear();
        ui.set_workspace_file_key(-1);
        ui.set_workspace_tree_offset(Length(0.0));
        ui.set_workspace_tree_status(String::new());
        let model = ui.model_workspace_preview_tabs();
        model.clear();
        for tab in &self.files.tabs {
            model.push(tab.row());
        }
        show_active(ui, &self.files);
    }

    fn remember_offset(&mut self, ui: &AgentWorkbench) {
        if !ui.get_workspace_file_scroll_pending() && !ui.get_workspace_file_editing() {
            self.files.remember_offset(ui.get_workspace_file_offset());
        }
    }
}

struct ListedFile {
    path: String,
    name: String,
    directory: bool,
}

pub(super) fn allowed_relative(path: &Path) -> bool {
    path.components().all(|component| {
        matches!(component, Component::Normal(name) if !name.as_encoded_bytes().eq_ignore_ascii_case(b".git"))
    })
}

fn locate(workspace: &str, path: &str, origin: FileOrigin) -> Result<LocatedFile, String> {
    let path = Path::new(path);
    let allowed = if path.is_absolute() {
        origin == FileOrigin::Reference
            && !path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
    } else {
        allowed_relative(path)
    };
    if workspace.is_empty() || !allowed {
        return Err("路径超出当前工作目录".into());
    }
    let scope = Path::new(workspace)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let base = if origin == FileOrigin::Diff {
        git_root(&scope)?
    } else {
        scope.clone()
    };
    let target = base
        .join(path)
        .canonicalize()
        .or_else(|error| {
            if origin == FileOrigin::Reference
                && error.kind() == io::ErrorKind::NotFound
                && let Ok(relative) = path.strip_prefix("a").or_else(|_| path.strip_prefix("b"))
                && !relative.as_os_str().is_empty()
            {
                return base.join(relative).canonicalize();
            }
            Err(error)
        })
        .map_err(|error| error.to_string())?;
    let path = target
        .strip_prefix(&scope)
        .map_err(|_| "路径超出当前工作目录")?;
    if !allowed_relative(path) {
        return Err("路径超出当前工作目录".into());
    }
    let metadata = fs::metadata(&target).map_err(|error| error.to_string())?;
    Ok(LocatedFile {
        relative: path
            .to_str()
            .ok_or("文件路径需要使用 UTF-8")?
            .replace(std::path::MAIN_SEPARATOR, "/"),
        absolute: target,
        stamp: (metadata.len(), metadata.modified().ok()),
    })
}

pub(super) fn locate_external_file(
    workspace: &str,
    path: &str,
    from_diff: bool,
) -> Result<PathBuf, String> {
    let origin = if from_diff {
        FileOrigin::Diff
    } else {
        FileOrigin::Workspace
    };
    let target = locate(workspace, path, origin)?.absolute;
    if !target.is_file() {
        return Err("所选路径不是普通文件".into());
    }
    Ok(target)
}

fn read_directory(workspace: &str, relative: &str) -> Result<(Vec<ListedFile>, bool), String> {
    let folder = locate(workspace, relative, FileOrigin::Workspace)?.absolute;
    let mut children = fs::read_dir(folder).map_err(|error| error.to_string())?;
    let mut entries = Vec::new();
    for child in children.by_ref().take(DIRECTORY_LIMIT) {
        let entry = child.map_err(|error| error.to_string())?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if name.eq_ignore_ascii_case(".git") || (!kind.is_dir() && !kind.is_file()) {
            continue;
        }
        let path = if relative.is_empty() {
            name.clone()
        } else {
            format!("{relative}/{name}")
        };
        entries.push(ListedFile {
            path,
            name,
            directory: kind.is_dir(),
        });
    }
    entries.sort_by(|left, right| {
        right
            .directory
            .cmp(&left.directory)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok((entries, children.next().is_some()))
}

enum Preview {
    Text {
        document: StyledText,
        note: String,
        markdown: bool,
        lines: Vec<PreviewLine>,
    },
    Image(Image),
}

fn read_preview(path: &Path, lines: Option<LineRange>) -> Result<Preview, String> {
    if !fs::metadata(path)
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("预览需要普通文件".into());
    }
    let file = File::open(path).map_err(|error| error.to_string())?;
    if let Some(range) = lines {
        let NumberedExcerpt { document, lines } = numbered_excerpt(file, range)?;
        let mut note = format!("已定位{}", range.label());
        if let Some(shown) = lines.last().map(|line| line.number)
            && shown < range.last
        {
            note.push_str(&format!(" · 预览至第 {shown} 行"));
        }
        return Ok(Preview::Text {
            document,
            note,
            markdown: false,
            lines,
        });
    }
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    if extension.eq_ignore_ascii_case("png") {
        let (bytes, truncated) =
            read_limited(file, PNG_PREVIEW_LIMIT as usize).map_err(|error| error.to_string())?;
        if truncated {
            return Err("PNG 超过 4 MiB，无法预览".into());
        }
        let image = preview_png(bytes)?.ok_or("PNG 超过 1600 万像素，无法预览")?;
        return Ok(Preview::Image(Image::Loaded(image)));
    }
    let (bytes, truncated) = read_limited(file, TEXT_LIMIT).map_err(|error| error.to_string())?;
    if bytes.contains(&0) {
        return Err("此文件是二进制文件".into());
    }
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        Err(error) if truncated && error.error_len().is_none() => {
            std::str::from_utf8(&bytes[..error.valid_up_to()]).map_err(|error| error.to_string())?
        }
        Err(_) => return Err("此文件不是 UTF-8 文本".into()),
    };
    if extension.eq_ignore_ascii_case("md")
        && !truncated
        && text.lines().take(LINE_LIMIT + 1).count() <= LINE_LIMIT
        && let Ok(markdown) = StyledText::from_markdown(text)
    {
        let document =
            StyledText::from_parts(markdown.text(), markdown.runs().to_vec(), Vec::new())
                .map_err(|error| error.to_string())?;
        return Ok(Preview::Text {
            document,
            note: String::new(),
            markdown: true,
            lines: Vec::new(),
        });
    }
    let mut document = String::new();
    let mut lines = text.split_inclusive('\n');
    let mut displayed = Vec::new();
    for (index, line) in lines.by_ref().take(LINE_LIMIT).enumerate() {
        let number = index + 1;
        let (line, ending) = split_line_ending(line);
        let _ = write!(document, "{number:>4}  ");
        displayed.push(append_preview_line(&mut document, number, line, "", ending));
    }
    let note = if truncated || lines.next().is_some() {
        "预览仅显示文件开头（最多 96 KiB、1200 行）".into()
    } else {
        String::new()
    };
    Ok(Preview::Text {
        document: StyledText::from_plain_text(document),
        note,
        markdown: false,
        lines: displayed,
    })
}

struct PreviewLine {
    number: usize,
    content: Range<usize>,
    newline: usize,
    ending: &'static str,
}

fn split_line_ending(line: &str) -> (&str, &'static str) {
    if let Some(text) = line.strip_suffix("\r\n") {
        (text, "\r\n")
    } else if let Some(text) = line.strip_suffix('\n') {
        (text, "\n")
    } else {
        (line, "")
    }
}

fn append_preview_line(
    document: &mut String,
    number: usize,
    text: &str,
    suffix: &str,
    ending: &'static str,
) -> PreviewLine {
    let start = document.len();
    document.push_str(text);
    let end = document.len();
    document.push_str(suffix);
    let newline = document.len();
    document.push('\n');
    PreviewLine {
        number,
        content: start..end,
        newline,
        ending,
    }
}

pub(super) struct NumberedExcerpt {
    pub document: StyledText,
    lines: Vec<PreviewLine>,
}

pub(super) fn numbered_excerpt(
    reader: impl Read,
    range: LineRange,
) -> Result<NumberedExcerpt, String> {
    let begin = range.first.saturating_sub(5).max(1);
    let end = range
        .last
        .min(range.first.saturating_add(EXCERPT_LINES - 1))
        .saturating_add(8);
    let mut document = String::new();
    let mut runs = Vec::new();
    let mut lines = Vec::new();
    let mut source = BufReader::new(reader.take(SOURCE_SCAN_LIMIT));
    let mut bytes = Vec::new();
    for number in 1..=end {
        bytes.clear();
        if source
            .read_until(b'\n', &mut bytes)
            .map_err(|error| error.to_string())?
            == 0
        {
            break;
        }
        if bytes.contains(&0) {
            return Err("此文件是二进制文件".into());
        }
        if number < begin {
            continue;
        }
        let decoded = String::from_utf8_lossy(&bytes);
        let (row, ending) = split_line_ending(&decoded);
        let marked = (range.first..=range.last).contains(&number);
        let start = document.len();
        let clip = row.floor_char_boundary(600);
        let suffix = if clip < row.len() { "…" } else { "" };
        let marker = if marked { "›" } else { " " };
        let _ = write!(document, "{marker} {number:>5}  ");
        lines.push(append_preview_line(
            &mut document,
            number,
            &row[..clip],
            suffix,
            ending,
        ));
        runs.push(StyledRun {
            start,
            end: document.len(),
            style: TextStyle {
                monospace: true,
                bold: marked,
                ..TextStyle::default()
            },
        });
    }
    if lines.last().is_none_or(|line| line.number < range.first) {
        return Err("找不到引用行（最多扫描文件开头 8 MiB）".into());
    }
    let text = StyledText::from_runs(&document, runs)
        .unwrap_or_else(|_| StyledText::from_plain_text(document));
    Ok(NumberedExcerpt {
        document: text,
        lines,
    })
}

fn checked_selection(text: &str, start: i64, end: i64, selected: &str) -> Option<Range<usize>> {
    let start = usize::try_from(start).ok()?;
    let end = usize::try_from(end).ok()?;
    let range = start.min(end)..start.max(end);
    (!selected.is_empty() && text.get(range.clone()) == Some(selected)).then_some(range)
}

fn source_excerpt(text: &str, range: Range<usize>, origin: ExcerptOrigin) -> FileExcerpt {
    let contents = &text[range.clone()];
    let first = text.as_bytes()[..range.start]
        .iter()
        .filter(|&&byte| byte == b'\n')
        .count()
        + 1;
    let newlines = contents.bytes().filter(|&byte| byte == b'\n').count();
    let last = first + newlines - usize::from(contents.ends_with('\n'));
    FileExcerpt {
        contents: contents.into(),
        lines: Some((first, last)),
        origin,
    }
}

fn preview_excerpt(preview: &Preview, start: i64, end: i64, selected: &str) -> Option<FileExcerpt> {
    let Preview::Text {
        document,
        markdown,
        lines,
        ..
    } = preview
    else {
        return None;
    };
    let selection = checked_selection(document.text(), start, end, selected)?;
    if *markdown {
        return Some(FileExcerpt {
            contents: selected.into(),
            lines: None,
            origin: ExcerptOrigin::MarkdownPreview,
        });
    }
    let mut contents = String::new();
    let mut bounds = None;
    for line in lines {
        let first = line.content.start.max(selection.start);
        let last = line.content.end.min(selection.end);
        let includes_end = !line.ending.is_empty() && selection.contains(&line.newline);
        if first >= last && !includes_end {
            continue;
        }
        if first < last {
            contents.push_str(document.text().get(first..last)?);
        }
        if includes_end {
            contents.push_str(line.ending);
        }
        let interval: &mut (usize, usize) = bounds.get_or_insert((line.number, line.number));
        interval.1 = line.number;
    }
    (!contents.trim().is_empty()).then_some(FileExcerpt {
        contents,
        lines: bounds,
        origin: ExcerptOrigin::Source,
    })
}

fn row(ui: &AgentWorkbench, key: i64) -> Option<WorkspaceFile> {
    ui.model_workspace_files()
        .snapshot()
        .iter()
        .find(|row| row.id == key)
        .cloned()
}

fn clear_preview(ui: &mut AgentWorkbench) {
    ui.set_workspace_file_path(String::new());
    ui.set_workspace_file_status(String::new());
    ui.set_workspace_file_text(StyledText::from_plain_text(""));
    ui.set_workspace_file_image(Image::Empty);
    ui.set_workspace_file_kind(String::new());
    ui.set_workspace_file_offset(Length(0.0));
    ui.set_workspace_file_scroll_pending(false);
    ui.set_workspace_file_positioned(false);
}

fn list(ui: &mut AgentWorkbench, state: Rc<RefCell<Browser>>, key: i64, relative: String) {
    let (generation, workspace) = {
        let mut state = state.borrow_mut();
        if state.workspace.is_empty() || state.loaded.contains(&key) || !state.loading.insert(key) {
            return;
        }
        (state.generation, state.workspace.clone())
    };
    let weak = ui.as_weak();
    let browser = state.clone();
    if key == -1 {
        ui.set_workspace_tree_status("正在读取目录…".into());
    }
    if let Err(error) = xrack_platform_winit::spawn_local(async move {
        let result = read_in_background("workbench-files", move || {
            read_directory(&workspace, &relative)
        })
        .await;
        let Some(mut ui) = weak.upgrade() else { return };
        let mut state = browser.borrow_mut();
        if state.generation != generation || state.workspace != ui.get_workspace() {
            return;
        }
        state.loading.remove(&key);
        let model = ui.model_workspace_files();
        match result {
            Ok((entries, truncated)) => {
                state.loaded.insert(key);
                let marker = model
                    .snapshot()
                    .iter()
                    .position(|row| row.parent_id == key && row.kind == "status");
                for entry in entries {
                    let id = state.key();
                    model.push(WorkspaceFile {
                        id,
                        parent_id: key,
                        path: entry.path,
                        name: entry.name,
                        kind: if entry.directory { "directory" } else { "file" }.into(),
                    });
                    if entry.directory {
                        model.push(WorkspaceFile {
                            id: state.key(),
                            parent_id: id,
                            path: String::new(),
                            name: "正在读取…".into(),
                            kind: "status".into(),
                        });
                    }
                }
                if let Some(marker) = marker {
                    model.remove(marker);
                }
                if ui.get_workspace_file_key() < 0
                    && let Some(tab) = state.files.active()
                {
                    select_tree_file(&mut ui, &tab.file.relative);
                }
                let note = if truncated {
                    format!("目录只显示前 {DIRECTORY_LIMIT} 项")
                } else if key == -1 && model.is_empty() {
                    "当前目录为空".into()
                } else {
                    String::new()
                };
                ui.set_workspace_tree_status(note);
            }
            Err(error) => {
                if let Some((index, marker)) = model
                    .snapshot()
                    .iter()
                    .enumerate()
                    .find(|(_, row)| row.parent_id == key && row.kind == "status")
                {
                    let mut marker = marker.clone();
                    marker.name = "读取失败，重新展开可重试".into();
                    model.set_row(index, marker);
                }
                ui.set_workspace_tree_status(format!("读取目录失败（{error}）"));
            }
        }
    }) {
        state.borrow_mut().loading.remove(&key);
        ui.set_workspace_tree_status(error.to_string());
    }
}

fn select_tree_file(ui: &mut AgentWorkbench, relative: &str) {
    let key = ui
        .model_workspace_files()
        .snapshot()
        .iter()
        .find(|entry| entry.kind == "file" && entry.path == relative)
        .map_or(-1, |entry| entry.id);
    ui.set_workspace_file_key(key);
}

fn show_active(ui: &mut AgentWorkbench, files: &OpenFiles) {
    clear_preview(ui);
    ui.set_workspace_file_edit_focus(false);
    editor::show(ui, files.active());
    let Some(tab) = files.active() else {
        ui.set_workspace_preview_tab(String::new());
        ui.set_workspace_file_key(-1);
        return;
    };
    select_tree_file(ui, &tab.file.relative);
    ui.set_workspace_preview_tab(tab.id.to_string());
    ui.set_workspace_file_path(tab.file.relative.clone());
    ui.set_workspace_file_positioned(tab.lines.is_some());
    match &tab.preview {
        Some(Ok(Preview::Text {
            document,
            note,
            markdown,
            ..
        })) => {
            ui.set_workspace_file_text(document.clone());
            ui.set_workspace_file_status(note.clone());
            ui.set_workspace_file_kind(if *markdown { "markdown" } else { "text" }.into());
        }
        Some(Ok(Preview::Image(image))) => {
            ui.set_workspace_file_image(image.clone());
            ui.set_workspace_file_kind("image".into());
        }
        Some(Err(error)) => ui.set_workspace_file_status(format!("无法预览（{error}）")),
        None => ui.set_workspace_file_status("正在读取文件…".into()),
    }
    ui.set_workspace_file_offset(tab.offset);
    if matches!(tab.preview, Some(Ok(Preview::Text { .. }))) {
        ui.set_workspace_file_restore_y(tab.offset);
        ui.set_workspace_file_scroll_pending(true);
    }
}

fn ensure_preview(ui: &mut AgentWorkbench, state: Rc<RefCell<Browser>>) {
    let (generation, workspace, id, revision, path, lines) = {
        let mut state = state.borrow_mut();
        let generation = state.generation;
        let workspace = state.workspace.clone();
        let Some(id) = state.files.active else { return };
        let Some(tab) = state.files.get_mut(id) else {
            return;
        };
        if tab.reading || tab.preview.is_some() || matches!(tab.editor, editor::Editor::Ready(_)) {
            return;
        }
        tab.reading = true;
        tab.revision = tab.revision.wrapping_add(1);
        (
            generation,
            workspace,
            id,
            tab.revision,
            tab.file.absolute.clone(),
            tab.lines,
        )
    };
    let weak = ui.as_weak();
    let browser = state.clone();
    if let Err(error) = xrack_platform_winit::spawn_local(async move {
        let result = read_in_background("workbench-files", move || {
            let path = path.to_str().ok_or("文件路径需要使用 UTF-8")?;
            let file = locate(&workspace, path, FileOrigin::Reference)?;
            read_preview(&file.absolute, lines)
        })
        .await;
        let Some(mut ui) = weak.upgrade() else { return };
        finish_preview(
            &mut ui,
            &mut browser.borrow_mut(),
            generation,
            id,
            revision,
            result,
        );
    }) {
        finish_preview(
            ui,
            &mut state.borrow_mut(),
            generation,
            id,
            revision,
            Err(error.to_string()),
        );
    }
}

fn finish_preview(
    ui: &mut AgentWorkbench,
    state: &mut Browser,
    generation: u64,
    id: i64,
    revision: u64,
    result: Result<Preview, String>,
) {
    if state.generation != generation || state.workspace != ui.get_workspace() {
        return;
    }
    let Some(tab) = state.files.get_mut(id) else {
        return;
    };
    if tab.revision != revision {
        return;
    }
    tab.reading = false;
    tab.preview = Some(result);
    state.files.prune_previews();
    if state.files.active == Some(id) {
        show_active(ui, &state.files);
    }
}

fn select(ui: &mut AgentWorkbench, state: Rc<RefCell<Browser>>, id: i64) {
    {
        let mut state = state.borrow_mut();
        state.remember_offset(ui);
        let used = state.next_intent();
        let Some(tab) = state.files.get_mut(id) else {
            return;
        };
        tab.last_used = used;
        state.files.active = Some(id);
        show_active(ui, &state.files);
    }
    ensure_preview(ui, state);
}

fn open(ui: &mut AgentWorkbench, state: Rc<RefCell<Browser>>, request: FileRequest) {
    let (generation, ticket, workspace) = {
        let mut state = state.borrow_mut();
        if state.workspace.is_empty() {
            return;
        }
        (
            state.generation,
            state.next_intent(),
            state.workspace.clone(),
        )
    };
    let weak = ui.as_weak();
    if let Err(error) = xrack_platform_winit::spawn_local(async move {
        let lines = request.lines;
        let located = read_in_background("workbench-files", move || {
            locate(&workspace, &request.path, request.origin)
        })
        .await;
        let Some(mut ui) = weak.upgrade() else { return };
        let id = {
            let mut browser = state.borrow_mut();
            if browser.generation != generation || browser.workspace != ui.get_workspace() {
                return;
            }
            let activate = browser.intent == ticket;
            let file = match located {
                Ok(file) => file,
                Err(error) => {
                    if activate {
                        ui.set_workspace_tree_status(format!("无法打开文件（{error}）"));
                    }
                    return;
                }
            };
            let id = insert_tab(
                &mut ui,
                &mut browser,
                file,
                (lines, request.column),
                ticket,
                activate,
            );
            if !activate {
                return;
            }
            id
        };
        if let Some(id) = id {
            select(&mut ui, state, id);
        }
    }) {
        ui.set_workspace_tree_status(error.to_string());
    }
}

fn insert_tab(
    ui: &mut AgentWorkbench,
    browser: &mut Browser,
    file: LocatedFile,
    position: (Option<LineRange>, Option<usize>),
    ticket: u64,
    activate: bool,
) -> Option<i64> {
    let (lines, column) = position;
    if browser
        .closed_at
        .get(&file.absolute)
        .is_some_and(|closed| *closed >= ticket)
    {
        return None;
    }
    if activate {
        browser.remember_offset(ui);
    }
    if let Some(tab) = browser
        .files
        .tabs
        .iter_mut()
        .find(|tab| tab.file.absolute == file.absolute)
    {
        if activate {
            if tab.file.stamp != file.stamp {
                tab.file.stamp = file.stamp;
                tab.invalidate();
            }
            if lines.is_some() {
                tab.position(lines, column);
            }
        }
        return Some(tab.id);
    }
    let id = browser.key();
    let tab = FileTab {
        id,
        file,
        lines,
        column,
        offset: Length(0.0),
        preview: None,
        reading: false,
        revision: 0,
        last_used: ticket,
        editor: editor::Editor::default(),
    };
    ui.model_workspace_preview_tabs().push(tab.row());
    browser.files.tabs.push(tab);
    Some(id)
}

fn close(ui: &mut AgentWorkbench, state: Rc<RefCell<Browser>>, id: i64) {
    let active_closed = {
        let mut state = state.borrow_mut();
        if !editor::allow_close(ui, &mut state, id, editor::CloseTarget::Tab) {
            return;
        }
        let active_closed = state.files.active == Some(id);
        let Some((index, tab)) = state.files.close(id) else {
            return;
        };
        let ticket = if active_closed {
            state.next_intent()
        } else {
            state.intent
        };
        state.closed_at.insert(tab.file.absolute, ticket);
        ui.model_workspace_preview_tabs().remove(index);
        if active_closed {
            if let Some(next) = state.files.active
                && let Some(tab) = state.files.get_mut(next)
            {
                tab.last_used = ticket;
            }
            show_active(ui, &state.files);
        }
        active_closed
    };
    if active_closed {
        ensure_preview(ui, state);
    }
}

fn context_tab<'a>(ui: &AgentWorkbench, browser: &'a Browser) -> Option<&'a FileTab> {
    let tab = browser.files.active()?;
    (ui.get_review_open()
        && ui.get_files_active()
        && ui.get_workspace() == browser.workspace
        && ui.get_workspace_preview_tab() == tab.id.to_string())
    .then_some(tab)
}

fn add_selected_excerpt(
    ui: &mut AgentWorkbench,
    browser: &Browser,
    attachments: &Attachments,
    start: i64,
    end: i64,
    selected: &str,
    editing: bool,
) {
    let Some(tab) = context_tab(ui, browser) else {
        return;
    };
    if ui.get_workspace_file_editing() != editing {
        return;
    }
    let excerpt = if editing {
        tab.editor.source_for_excerpt().and_then(|(text, origin)| {
            let range = checked_selection(text, start, end, selected)?;
            Some(source_excerpt(text, range, origin))
        })
    } else {
        tab.preview
            .as_ref()
            .and_then(|preview| preview.as_ref().ok())
            .and_then(|preview| preview_excerpt(preview, start, end, selected))
    };
    let Some(path) = tab.file.absolute.to_str() else {
        return;
    };
    if let Some(excerpt) = excerpt
        && attachments.add_excerpt(ui, path.into(), excerpt)
    {
        ui.invoke_focus_composer();
    }
}

fn attach_file_context(
    ui: &mut AgentWorkbench,
    state: &Rc<RefCell<Browser>>,
    attachments: &Attachments,
) {
    let weak = ui.as_weak();
    let browser = state.clone();
    let draft = attachments.clone();
    ui.on_attach_workspace_file(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        let browser = browser.borrow();
        let Some(path) = context_tab(&ui, &browser).and_then(|tab| tab.file.absolute.to_str())
        else {
            return;
        };
        if draft.add_workspace_file(&mut ui, path.into(), browser.workspace.clone()) {
            ui.invoke_focus_composer();
        }
    });
    let weak = ui.as_weak();
    let browser = state.clone();
    let draft = attachments.clone();
    ui.on_attach_workspace_preview_excerpt(move |start, end, text| {
        if let Some(mut ui) = weak.upgrade() {
            add_selected_excerpt(&mut ui, &browser.borrow(), &draft, start, end, &text, false);
        }
    });
    let weak = ui.as_weak();
    let browser = state.clone();
    let draft = attachments.clone();
    ui.on_attach_workspace_editor_excerpt(move |start, end, text| {
        if let Some(mut ui) = weak.upgrade() {
            add_selected_excerpt(&mut ui, &browser.borrow(), &draft, start, end, &text, true);
        }
    });
}

pub fn attach(ui: &mut AgentWorkbench, attachments: &Attachments) {
    let state = Rc::new(RefCell::new(Browser::default()));
    state.borrow_mut().reset(ui);
    editor::attach(ui, &state);
    attach_file_context(ui, &state, attachments);
    let browser = state.clone();
    ui.on_can_switch_workspace_checkout(move || {
        !browser
            .borrow()
            .files
            .tabs
            .iter()
            .any(|tab| tab.editor.dirty() || tab.editor.saving())
    });
    let weak = ui.as_weak();
    let browser = state.clone();
    ui.on_workspace_checkout_changed(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        for tab in &mut browser.borrow_mut().files.tabs {
            if !tab.editor.dirty() && !tab.editor.saving() {
                tab.editor = editor::Editor::Closed;
            }
        }
        ui.invoke_reset_workspace_files();
    });
    let weak = ui.as_weak();
    let browser = state.clone();
    ui.on_open_preview_external(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        let browser = browser.borrow();
        if browser.workspace == ui.get_workspace()
            && let Some(tab) = browser.files.active()
        {
            ui.invoke_present_external(
                "file".into(),
                tab.file.relative.clone(),
                tab.lines.map_or(0, |lines| lines.first as i64),
                tab.column.unwrap_or(0) as i64,
            );
        }
    });
    let weak = ui.as_weak();
    let browser = state.clone();
    ui.on_open_quick_file(move |path, first, last| {
        if let Some(mut ui) = weak.upgrade()
            && !ui.get_switching()
        {
            let lines = usize::try_from(first)
                .ok()
                .zip(usize::try_from(last).ok())
                .filter(|(first, last)| *first > 0 && first <= last)
                .map(|(first, last)| LineRange { first, last });
            ui.invoke_show_workspace_files();
            open(
                &mut ui,
                browser.clone(),
                FileRequest {
                    path,
                    origin: FileOrigin::Workspace,
                    lines,
                    column: None,
                },
            );
        }
    });
    let weak = ui.as_weak();
    let browser = state.clone();
    ui.on_reset_workspace_files(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        browser.borrow_mut().reset(&mut ui);
        if ui.get_review_open() && ui.get_files_active() {
            list(&mut ui, browser.clone(), -1, String::new());
            ensure_preview(&mut ui, browser.clone());
        }
    });
    let weak = ui.as_weak();
    let browser = state.clone();
    ui.on_load_workspace_files(move || {
        if let Some(mut ui) = weak.upgrade() {
            list(&mut ui, browser.clone(), -1, String::new());
            ensure_preview(&mut ui, browser.clone());
        }
    });
    let weak = ui.as_weak();
    let browser = state.clone();
    ui.on_expand_workspace_directory(move |key| {
        if let Some(mut ui) = weak.upgrade()
            && let Some(row) = row(&ui, key)
            && row.kind == "directory"
        {
            list(&mut ui, browser.clone(), key, row.path);
        }
    });
    let weak = ui.as_weak();
    let browser = state.clone();
    ui.on_open_workspace_selection(move |key| {
        if let Some(mut ui) = weak.upgrade()
            && let Some(row) = row(&ui, key)
            && row.kind == "file"
        {
            open(
                &mut ui,
                browser.clone(),
                FileRequest {
                    path: row.path,
                    origin: FileOrigin::Workspace,
                    lines: None,
                    column: None,
                },
            );
        }
    });
    let weak = ui.as_weak();
    let browser = state.clone();
    ui.on_preview_diff_source(move |path| {
        if let Some(mut ui) = weak.upgrade() {
            open(
                &mut ui,
                browser.clone(),
                FileRequest {
                    path,
                    origin: FileOrigin::Diff,
                    lines: None,
                    column: None,
                },
            );
        }
    });
    let weak = ui.as_weak();
    let browser = state.clone();
    ui.on_open_workspace_reference(move |href| {
        if let Some(mut ui) = weak.upgrade()
            && !ui.get_switching()
            && let Some(reference) = FileReference::from_link(&href)
        {
            ui.invoke_show_workspace_files();
            open(
                &mut ui,
                browser.clone(),
                FileRequest {
                    path: reference.path,
                    origin: FileOrigin::Reference,
                    lines: reference.lines,
                    column: reference.column,
                },
            );
        }
    });
    let weak = ui.as_weak();
    let browser = state.clone();
    ui.on_load_workspace_file_from_start(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        {
            let mut state = browser.borrow_mut();
            state.next_intent();
            let Some(id) = state.files.active else { return };
            let Some(tab) = state.files.get_mut(id) else {
                return;
            };
            tab.position(None, None);
            show_active(&mut ui, &state.files);
        }
        ensure_preview(&mut ui, browser.clone());
    });
    let weak = ui.as_weak();
    let browser = state.clone();
    ui.on_select_workspace_preview_tab(move |id| {
        if let Some(mut ui) = weak.upgrade()
            && let Ok(id) = id.parse::<i64>()
        {
            select(&mut ui, browser.clone(), id);
        }
    });
    let weak = ui.as_weak();
    let browser = state.clone();
    ui.on_close_workspace_preview_tab(move |id| {
        if let Some(mut ui) = weak.upgrade()
            && let Ok(id) = id.parse::<i64>()
        {
            close(&mut ui, browser.clone(), id);
        }
    });
    ui.on_reorder_workspace_preview_tabs(move |from, to| {
        if let (Ok(from), Ok(to)) = (usize::try_from(from), usize::try_from(to)) {
            state.borrow_mut().files.reorder(from, to);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_excerpt_omits_number_gutters_and_clipped_suffixes_and_keeps_real_lines()
    -> Result<(), Box<dyn std::error::Error>> {
        let data = "首行\n".repeat(10) + &"中".repeat(209) + "\n  下一行\n";
        let preview = numbered_excerpt(
            data.as_bytes(),
            LineRange {
                first: 11,
                last: 12,
            },
        )?;
        let start = preview.document.text().find("中中").unwrap();
        let end = preview.document.text().find("下一行").unwrap() + "下一行".len();
        let selected = preview.document.text()[start..end].to_owned();
        assert!(selected.contains('…'));
        let preview = Preview::Text {
            document: preview.document,
            note: String::new(),
            markdown: false,
            lines: preview.lines,
        };
        let snippet = preview_excerpt(&preview, start as i64, end as i64, &selected).unwrap();
        assert_eq!(snippet.lines, Some((11, 12)));
        assert_eq!(snippet.contents, "中".repeat(200) + "\n  下一行");
        assert_eq!(snippet.origin, ExcerptOrigin::Source);
        assert!(preview_excerpt(&preview, start as i64, end as i64, "stale").is_none());
        assert!(preview_excerpt(&preview, (start + 1) as i64, end as i64, &selected).is_none());

        let draft = "首行\r\nlet 名 = 8;\r\n下一行";
        let start = "首行\r\n".len();
        let end = start + "let 名 = 8;\r\n".len();
        let selection =
            checked_selection(draft, end as i64, start as i64, "let 名 = 8;\r\n").unwrap();
        let snippet = source_excerpt(draft, selection, ExcerptOrigin::Draft);
        assert_eq!(snippet.lines, Some((2, 2)));
        assert_eq!(snippet.contents, "let 名 = 8;\r\n");

        let original = "hello\r\nworld";
        let numbered = numbered_excerpt(original.as_bytes(), LineRange { first: 1, last: 2 })?;
        let visible = numbered.document.text().to_owned();
        let preview = Preview::Text {
            document: numbered.document,
            note: String::new(),
            markdown: false,
            lines: numbered.lines,
        };
        let snippet = preview_excerpt(&preview, 0, visible.len() as i64, &visible).unwrap();
        assert_eq!(snippet.contents, original);
        assert_eq!(snippet.lines, Some((1, 2)));
        Ok(())
    }

    fn text_preview(text: &str) -> Result<Preview, String> {
        Ok(Preview::Text {
            document: StyledText::from_plain_text(text),
            note: String::new(),
            markdown: false,
            lines: Vec::new(),
        })
    }

    #[test]
    fn file_tabs_share_a_canonical_identity_keep_position_and_close_to_the_neighbor()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let workspace = temp.path().canonicalize()?;
        let root = workspace.to_str().unwrap();
        fs::write(workspace.join("first.rs"), "first")?;
        fs::write(workspace.join("second.rs"), "second")?;
        let mut ui = AgentWorkbench::new();
        ui.set_workspace(root.into());
        let browser = Rc::new(RefCell::new(Browser::default()));
        browser.borrow_mut().reset(&mut ui);
        let lines = LineRange {
            first: 31,
            last: 33,
        };
        let (first, second) = {
            let mut state = browser.borrow_mut();
            let first = insert_tab(
                &mut ui,
                &mut state,
                locate(root, "first.rs", FileOrigin::Workspace)?,
                (Some(lines), None),
                1,
                true,
            )
            .unwrap();
            let alias = workspace.join("first.rs");
            assert_eq!(
                insert_tab(
                    &mut ui,
                    &mut state,
                    locate(root, alias.to_str().unwrap(), FileOrigin::Reference)?,
                    (None, None),
                    2,
                    true
                ),
                Some(first)
            );
            let second = insert_tab(
                &mut ui,
                &mut state,
                locate(root, "second.rs", FileOrigin::Workspace)?,
                (None, None),
                3,
                true,
            )
            .unwrap();
            assert_eq!(ui.model_workspace_preview_tabs().len(), 2);
            let generation = state.generation;
            finish_preview(
                &mut ui,
                &mut state,
                generation,
                first,
                0,
                text_preview("first excerpt"),
            );
            finish_preview(
                &mut ui,
                &mut state,
                generation,
                second,
                0,
                text_preview("second document"),
            );
            (first, second)
        };
        select(&mut ui, browser.clone(), first);
        ui.set_workspace_file_scroll_pending(false);
        ui.set_workspace_file_offset(Length(128.0));
        select(&mut ui, browser.clone(), second);
        assert_eq!(ui.get_workspace_file_text().text(), "second document");
        assert!(!ui.get_workspace_file_positioned());

        assert!(ui.model_workspace_preview_tabs().move_row(0, 1));
        browser.borrow_mut().files.reorder(0, 1);
        select(&mut ui, browser.clone(), first);
        assert_eq!(ui.get_workspace_file_offset(), Length(128.0));
        assert!(ui.get_workspace_file_positioned());
        assert_eq!(ui.get_workspace_file_text().text(), "first excerpt");
        close(&mut ui, browser.clone(), first);
        assert_eq!(ui.get_workspace_preview_tab(), second.to_string());
        assert_eq!(ui.model_workspace_preview_tabs().len(), 1);
        close(&mut ui, browser.clone(), second);
        assert!(ui.get_workspace_file_path().is_empty());
        assert!(ui.model_workspace_preview_tabs().is_empty());
        Ok(())
    }

    #[test]
    fn switching_workspaces_parks_tab_positions_and_late_reads_cannot_change_the_view()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().canonicalize()?;
        let other = root.join("other");
        fs::create_dir(&other)?;
        fs::write(root.join("main.rs"), "first")?;
        fs::write(other.join("main.rs"), "second")?;
        let workspace = root.to_str().unwrap();
        let mut ui = AgentWorkbench::new();
        ui.set_workspace(workspace.into());
        let browser = Rc::new(RefCell::new(Browser::default()));
        browser.borrow_mut().reset(&mut ui);
        let first_generation = browser.borrow().generation;
        let file = locate(workspace, "main.rs", FileOrigin::Workspace)?;
        let first = insert_tab(
            &mut ui,
            &mut browser.borrow_mut(),
            file,
            (None, None),
            1,
            true,
        )
        .unwrap();
        finish_preview(
            &mut ui,
            &mut browser.borrow_mut(),
            first_generation,
            first,
            0,
            text_preview("first"),
        );
        select(&mut ui, browser.clone(), first);
        ui.set_workspace_file_scroll_pending(false);
        ui.set_workspace_file_offset(Length(192.0));

        ui.set_workspace(other.to_str().unwrap().into());
        browser.borrow_mut().reset(&mut ui);
        assert!(ui.model_workspace_preview_tabs().is_empty());
        finish_preview(
            &mut ui,
            &mut browser.borrow_mut(),
            first_generation,
            first,
            0,
            text_preview("late"),
        );
        assert!(ui.get_workspace_file_path().is_empty());

        ui.set_workspace(workspace.into());
        browser.borrow_mut().reset(&mut ui);
        assert_eq!(ui.get_workspace_preview_tab(), first.to_string());
        assert_eq!(ui.get_workspace_file_offset(), Length(192.0));
        let mut state = browser.borrow_mut();
        let generation = state.generation;
        let tab = state.files.get_mut(first).unwrap();
        let revision = tab.revision;
        tab.position(
            Some(LineRange {
                first: 12,
                last: 12,
            }),
            None,
        );
        finish_preview(
            &mut ui,
            &mut state,
            generation,
            first,
            revision,
            text_preview("old range"),
        );
        assert!(state.files.get_mut(first).unwrap().preview.is_none());
        let (index, tab) = state.files.close(first).unwrap();
        ui.model_workspace_preview_tabs().remove(index);
        state.closed_at.insert(tab.file.absolute, 4);
        assert!(
            insert_tab(
                &mut ui,
                &mut state,
                locate(workspace, "main.rs", FileOrigin::Workspace)?,
                (None, None),
                3,
                false
            )
            .is_none()
        );
        assert!(ui.model_workspace_preview_tabs().is_empty());
        Ok(())
    }

    fn preview(workspace: &str, path: &str, diff: bool) -> Result<Preview, String> {
        let file = locate(
            workspace,
            path,
            if diff {
                FileOrigin::Diff
            } else {
                FileOrigin::Workspace
            },
        )?;
        read_preview(&file.absolute, None)
    }

    fn preview_request(workspace: &str, request: &FileRequest) -> Result<Preview, String> {
        let file = locate(workspace, &request.path, request.origin)?;
        read_preview(&file.absolute, request.lines)
    }

    #[test]
    fn reads_one_level_in_directory_order_and_keeps_utf8_paths()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        fs::create_dir(root.join("源码"))?;
        fs::create_dir(root.join(".git"))?;
        fs::write(root.join(".git/index"), "internal")?;
        fs::write(root.join("源码/main.rs"), "fn main() {}")?;
        fs::write(root.join("README.md"), "**说明**")?;
        let workspace = root.to_str().unwrap();
        let (rows, truncated) = read_directory(workspace, "")?;
        assert!(!truncated);
        assert_eq!(
            rows.iter()
                .map(|row| (row.path.as_str(), row.directory))
                .collect::<Vec<_>>(),
            [("源码", true), ("README.md", false)]
        );
        let (rows, _) = read_directory(workspace, "源码")?;
        assert_eq!(rows[0].path, "源码/main.rs");
        assert!(preview(workspace, ".git/index", false).is_err());
        assert!(read_directory(workspace, "..").is_err());
        Ok(())
    }

    #[test]
    fn previews_png_markdown_and_line_numbered_utf8_with_a_bounded_prefix()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        let workspace = root.to_str().unwrap();
        fs::write(root.join("image.PNG"), crate::attachments::PNG_FIXTURE)?;
        assert!(matches!(
            preview(workspace, "image.PNG", false)?,
            Preview::Image(Image::Loaded(_))
        ));

        fs::write(
            root.join("README.md"),
            "**入口** [link](https://example.invalid)",
        )?;
        let Preview::Text {
            document,
            markdown: true,
            ..
        } = preview(workspace, "README.md", false)?
        else {
            panic!("expected formatted Markdown")
        };
        assert_eq!(document.text(), "入口 link");
        assert!(document.runs().iter().any(|run| run.style.bold));
        assert!(document.links().is_empty());

        fs::write(
            root.join("example.rs"),
            "let 名字 = 5;\nprintln!(\"{名字}\");",
        )?;
        let Preview::Text {
            document,
            markdown: false,
            note,
            ..
        } = preview(workspace, "example.rs", false)?
        else {
            panic!("expected source text")
        };
        assert!(note.is_empty());
        assert_eq!(
            document.text(),
            "   1  let 名字 = 5;\n   2  println!(\"{名字}\");\n"
        );

        fs::write(root.join("large.txt"), "a".repeat(TEXT_LIMIT - 1) + "界")?;
        let Preview::Text { document, note, .. } = preview(workspace, "large.txt", false)? else {
            panic!("expected a UTF-8 prefix")
        };
        assert!(note.contains("96 KiB"));
        assert!(document.text().ends_with("a\n"));
        fs::write(root.join("binary"), [0, 1, 2])?;
        assert!(preview(workspace, "binary", false).is_err());
        Ok(())
    }

    #[test]
    fn diff_paths_are_git_relative_but_remain_inside_the_selected_workspace()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        crate::review::git(root, &["init", "-q"], 1024)?;
        let nested = root.join("project");
        fs::create_dir(&nested)?;
        fs::write(root.join("outside.txt"), "outside")?;
        fs::write(nested.join("inside.txt"), "source")?;
        let workspace = nested.to_str().unwrap();
        assert!(preview(workspace, "project/inside.txt", true).is_ok());
        assert!(preview(workspace, "outside.txt", true).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("outside.txt"), nested.join("link"))?;
            assert!(preview(workspace, "link", false).is_err());
        }
        Ok(())
    }

    #[test]
    fn file_reference_reaches_deep_lines_and_highlights_only_the_requested_range()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("workspace");
        fs::create_dir_all(root.join("src"))?;
        let file = root.join("src/界 面.rs");
        let content = (1..=1620)
            .map(|line| format!("source {line}\n"))
            .collect::<String>();
        fs::write(&file, content)?;
        let outside = temp.path().join("outside.rs");
        fs::write(&outside, "outside")?;
        let workspace = root.to_str().unwrap();
        for path in ["b/src/界 面.rs", file.to_str().unwrap()] {
            let request = FileRequest {
                path: path.into(),
                origin: FileOrigin::Reference,
                lines: Some(LineRange {
                    first: 1531,
                    last: 1532,
                }),
                column: None,
            };
            let Preview::Text { document, note, .. } = preview_request(workspace, &request)? else {
                panic!("expected a source excerpt")
            };
            assert_eq!(note, "已定位第 1531–1532 行");
            assert!(!document.text().contains("source 1525"));
            assert!(document.text().contains("source 1526"));
            let highlighted: Vec<_> = document
                .runs()
                .iter()
                .filter(|run| run.style.bold)
                .map(|run| &document.text()[run.start..run.end])
                .collect();
            assert_eq!(
                highlighted,
                ["›  1531  source 1531\n", "›  1532  source 1532\n"]
            );
        }
        let request = FileRequest {
            path: outside.to_str().unwrap().into(),
            origin: FileOrigin::Reference,
            lines: Some(LineRange { first: 1, last: 1 }),
            column: None,
        };
        assert!(preview_request(workspace, &request).is_err());
        assert!(numbered_excerpt(&b"one\n"[..], LineRange { first: 8, last: 8 }).is_err());
        Ok(())
    }
}
