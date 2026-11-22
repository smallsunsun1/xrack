use crate::{
    file_references::LineRange, read_in_background, review::git, workspace_files::allowed_relative,
};
use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};
use workbench_ui::{AgentWorkbench, PaletteItem};

const MAX_FILES: usize = 50_000;
const MAX_VISITED: usize = 200_000;
const INDEX_BYTES: usize = 8 * 1024 * 1024;
const RESULT_LIMIT: usize = 60;
const SLASH_COMMANDS: &[(&str, &str)] = &[
    ("model", "generation-settings"),
    ("plan", "plan-mode"),
    ("review", "review-code"),
    ("fork", "fork-current"),
    ("diff", "workspace-changes"),
    ("terminal", "terminal"),
    ("find", "find-chat"),
];

struct IndexedFile {
    path: String,
    folded: String,
}

pub(super) struct FileIndex {
    root: PathBuf,
    files: Vec<IndexedFile>,
    limited: bool,
}

impl FileIndex {
    pub(super) fn build(root: &Path) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|error| error.to_string())?;
        let (mut paths, mut limited) = match git(
            &root,
            &[
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ],
            INDEX_BYTES,
        ) {
            Ok((data, limited)) => {
                let paths = data
                    .split_inclusive(|&byte| byte == 0)
                    .filter_map(|record| std::str::from_utf8(record.strip_suffix(&[0])?).ok())
                    .filter(|path| !path.is_empty() && allowed_relative(Path::new(path)))
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                (paths, limited)
            }
            Err(_) => walk_workspace(&root)?,
        };
        paths.sort_unstable();
        paths.dedup();
        limited |= paths.len() > MAX_FILES;
        paths.truncate(MAX_FILES);
        let files = paths
            .into_iter()
            .map(|path| IndexedFile {
                folded: path.to_lowercase(),
                path,
            })
            .collect();
        Ok(Self {
            root,
            files,
            limited,
        })
    }

    pub(super) fn search(&self, query: &str, limit: usize) -> Vec<String> {
        let (text, _) = file_query(query);
        let query = text.to_lowercase();
        let terms: Vec<_> = query.split_whitespace().collect();
        let mut ranked = Vec::new();
        for item in &self.files {
            let name = item.folded.rsplit('/').next().unwrap_or(&item.folded);
            let rank = terms.iter().try_fold(0, |score, term| {
                Some(score + file_match(name, &item.folded, term)?)
            });
            if let Some(rank) = rank {
                ranked.push((rank, item.path.as_str()));
            }
        }
        ranked.sort_unstable_by_key(|(rank, path)| (*rank, path.len(), *path));
        ranked
            .into_iter()
            .filter(|(_, path)| self.exists(path))
            .take(limit)
            .map(|(_, path)| path.to_owned())
            .collect()
    }

    fn exists(&self, relative: &str) -> bool {
        let path = self.root.join(relative);
        fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file())
            && path.canonicalize().is_ok_and(|resolved| {
                resolved
                    .strip_prefix(&self.root)
                    .is_ok_and(allowed_relative)
            })
    }
}

fn walk_workspace(root: &Path) -> Result<(Vec<String>, bool), String> {
    let mut folders = vec![PathBuf::new()];
    let mut files = Vec::new();
    let mut visited = 0;
    while let Some(folder) = folders.pop() {
        let entries = match fs::read_dir(root.join(&folder)) {
            Ok(entries) => entries,
            Err(error) if folder.as_os_str().is_empty() => return Err(error.to_string()),
            Err(_) => continue,
        };
        for entry in entries {
            if visited == MAX_VISITED || files.len() >= MAX_FILES {
                return Ok((files, true));
            }
            visited += 1;
            let Ok(entry) = entry else { continue };
            let relative = folder.join(entry.file_name());
            let Some(relative_text) = relative.to_str() else {
                continue;
            };
            if !allowed_relative(&relative) {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                folders.push(relative);
            } else if kind.is_file() {
                files.push(relative_text.replace(std::path::MAIN_SEPARATOR, "/"));
            }
        }
    }
    Ok((files, false))
}

fn file_match(name: &str, path: &str, term: &str) -> Option<usize> {
    if name == term {
        return Some(0);
    }
    if name.starts_with(term) {
        return Some(1);
    }
    if name.contains(term) {
        return Some(2);
    }
    if path.contains(term) {
        return Some(3);
    }
    let mut chars = name.chars();
    for wanted in term.chars() {
        chars.find(|&letter| letter == wanted)?;
    }
    Some(4)
}

fn file_query(query: &str) -> (&str, Option<LineRange>) {
    let query = query.trim();
    if let Some((path, range)) = query.rsplit_once("#L").or_else(|| query.rsplit_once(':'))
        && let Some(lines) = LineRange::parse(range)
    {
        return (path.trim(), Some(lines));
    }
    (query, None)
}

#[derive(Default)]
struct Search {
    workspace: String,
    generation: u64,
    query_version: u64,
    indexing: bool,
    index: Option<Arc<FileIndex>>,
}

impl Search {
    fn current_workspace(&self, ui: &AgentWorkbench, generation: u64) -> bool {
        ui.get_palette_open()
            && ui.get_palette_file_mode()
            && ui.get_workspace() == self.workspace
            && self.generation == generation
    }
}

fn publish(ui: &mut AgentWorkbench, items: Vec<PaletteItem>, query: String, status: &str) {
    let model = ui.model_palette_items();
    model.clear();
    let initial = if items.is_empty() { -1 } else { 0 };
    for item in items {
        model.push(item);
    }
    ui.set_palette_index(initial);
    ui.set_palette_result_query(query);
    ui.set_palette_status(status.into());
    ui.set_palette_ready(true);
    ui.invoke_reveal_palette_item();
}

fn update(ui: &mut AgentWorkbench, search: Rc<RefCell<Search>>) {
    if !ui.get_palette_open() {
        return;
    }
    let query = ui.get_palette_query();
    if !ui.get_palette_file_mode() {
        let normalized = query.trim().to_lowercase();
        let items: Vec<_> = ui
            .get_palette_commands()
            .into_iter()
            .filter(|item| {
                item.enabled
                    && (item.title.to_lowercase().contains(&normalized)
                        || item.detail.to_lowercase().contains(&normalized)
                        || item.key.contains(&normalized))
            })
            .collect();
        let status = if items.is_empty() {
            "没有匹配的命令"
        } else {
            ""
        };
        publish(ui, items, query, status);
        return;
    }

    let (generation, version, workspace, index) = {
        let mut search = search.borrow_mut();
        search.query_version = search.query_version.wrapping_add(1);
        (
            search.generation,
            search.query_version,
            search.workspace.clone(),
            search.index.clone(),
        )
    };
    let Some(index) = index else {
        build_index(ui, search, generation, workspace);
        return;
    };
    ui.set_palette_ready(false);
    ui.set_palette_status("正在搜索…".into());
    let weak = ui.as_weak();
    if let Err(error) = xrack_platform_winit::spawn_local(async move {
        let text = query.clone();
        let limited = index.limited;
        let result = read_in_background("workbench-file-search", move || {
            Ok(index.search(&text, RESULT_LIMIT))
        })
        .await;
        let Some(mut ui) = weak.upgrade() else { return };
        let current = search.borrow();
        if !current.current_workspace(&ui, generation)
            || current.query_version != version
            || ui.get_palette_query() != query
        {
            return;
        }
        match result {
            Ok(paths) => {
                let items: Vec<_> = paths.into_iter().map(file_item).collect();
                let status = match (items.is_empty(), limited) {
                    (true, true) => "扫描范围内没有匹配的文件",
                    (true, false) => "没有匹配的文件",
                    (false, true) => "结果来自部分工作区文件",
                    _ => "",
                };
                publish(&mut ui, items, query, status);
            }
            Err(error) => ui.set_palette_status(error),
        }
    }) {
        ui.set_palette_status(error.to_string());
    }
}

pub(super) fn file_item(path: String) -> PaletteItem {
    let (parent, title) = path.rsplit_once('/').unwrap_or(("", &path));
    PaletteItem {
        title: title.into(),
        detail: parent.into(),
        key: path,
        enabled: true,
    }
}

fn build_index(
    ui: &mut AgentWorkbench,
    search: Rc<RefCell<Search>>,
    generation: u64,
    workspace: String,
) {
    ui.set_palette_ready(false);
    ui.set_palette_status("正在索引工作区文件…".into());
    {
        let mut search = search.borrow_mut();
        if search.indexing {
            return;
        }
        search.indexing = true;
    }
    let weak = ui.as_weak();
    let state = search.clone();
    if let Err(error) = xrack_platform_winit::spawn_local(async move {
        let result = read_in_background("workbench-file-index", move || {
            FileIndex::build(Path::new(&workspace))
        })
        .await;
        let Some(mut ui) = weak.upgrade() else { return };
        {
            let mut search = state.borrow_mut();
            if !search.current_workspace(&ui, generation) {
                return;
            }
            search.indexing = false;
            match result {
                Ok(index) => search.index = Some(Arc::new(index)),
                Err(error) => {
                    ui.set_palette_status(format!("无法索引文件（{error}）"));
                    return;
                }
            }
        }
        update(&mut ui, state);
    }) {
        search.borrow_mut().indexing = false;
        ui.set_palette_status(error.to_string());
    }
}

fn activate(ui: &mut AgentWorkbench, workspace: &str, index: i64) {
    if !ui.get_palette_open()
        || !ui.get_palette_ready()
        || ui.get_switching()
        || ui.get_palette_result_query() != ui.get_palette_query()
    {
        return;
    }
    let Some(item) = usize::try_from(index)
        .ok()
        .and_then(|index| ui.model_palette_items().row(index))
    else {
        return;
    };
    if ui.get_palette_file_mode() {
        if workspace != ui.get_workspace() {
            return;
        }
        let (_, lines) = file_query(&ui.get_palette_query());
        let (first, last) = lines
            .and_then(|lines| {
                Some((
                    i64::try_from(lines.first).ok()?,
                    i64::try_from(lines.last).ok()?,
                ))
            })
            .unwrap_or((0, 0));
        ui.set_palette_open(false);
        ui.invoke_open_quick_file(item.key, first, last);
        return;
    }
    if !command_available(ui, &item.key) {
        return;
    }
    ui.set_palette_open(false);
    run_command(ui, &item.key);
}

fn command_available(ui: &AgentWorkbench, key: &str) -> bool {
    ui.get_palette_commands()
        .iter()
        .any(|entry| entry.enabled && entry.key == key)
}

fn run_command(ui: &mut AgentWorkbench, key: &str) {
    match key {
        "quick-open" => ui.invoke_open_workbench_palette(true),
        "open-folder" => ui.invoke_pick_workspace(),
        "new-conversation" => ui.invoke_new_conversation(),
        "reopen-conversation-tab" => ui.invoke_reopen_conversation_tab(),
        "notifications" => ui.invoke_open_notification_settings(),
        "activity" => ui.invoke_open_activity(),
        "next-attention" => ui.invoke_open_next_attention(),
        "back" => ui.invoke_navigate_conversation(false),
        "forward" => ui.invoke_navigate_conversation(true),
        "search-sessions" => ui.invoke_focus_session_search(),
        "search-all-projects" => ui.invoke_open_project_search(),
        "workspace-files" => ui.invoke_show_workspace_files(),
        "project-actions" => ui.invoke_open_project_actions(),
        "terminal" => ui.invoke_toggle_terminal(),
        "generation-settings" => ui.set_generation_picker_open(true),
        "plan-mode" => ui.invoke_choose_plan_mode(!ui.get_plan_mode()),
        "fork-current" => ui.invoke_open_fork(String::new()),
        "workspace-changes" => {
            ui.set_review_open(true);
            ui.set_review_turn(-1);
            ui.invoke_show_workspace_changes();
        }
        "review-code" => ui.invoke_open_review_scope(),
        "find-chat" => ui.invoke_open_chat_find(),
        "focus-composer" => ui.invoke_focus_composer(),
        "latest-message" => ui.invoke_reveal_latest(),
        "pin-session" => ui.invoke_set_session_pinned(!ui.get_conversation_pinned()),
        "composer-settings" => ui.set_composer_settings_open(true),
        "appearance" => ui.set_appearance_open(true),
        "stop" => ui.invoke_interrupt(),
        _ => {}
    }
}

fn slash_query(draft: &str) -> Option<String> {
    let word = draft.strip_prefix('/')?;
    (word.len() <= 32
        && word
            .bytes()
            .all(|byte| byte.is_ascii_alphabetic() || byte == b'-'))
    .then(|| word.to_ascii_lowercase())
}

fn update_slash(ui: &mut AgentWorkbench, draft: String) {
    let rows = ui.model_slash_command_items();
    rows.clear();
    let Some(query) = slash_query(&draft) else {
        ui.set_slash_commands_open(false);
        return;
    };
    let commands = ui.get_palette_commands();
    for &(name, key) in SLASH_COMMANDS {
        if name.starts_with(&query)
            && let Some(command) = commands
                .iter()
                .find(|command| command.enabled && command.key == key)
        {
            rows.push(PaletteItem {
                key: key.into(),
                title: format!("/{name}"),
                detail: command.title.clone(),
                enabled: true,
            });
        }
    }
    ui.set_slash_command_index(0);
    ui.set_slash_command_caret(draft.len() as i64);
    ui.set_slash_command_draft(draft);
    ui.set_slash_commands_open(!rows.is_empty());
}

fn activate_slash(ui: &mut AgentWorkbench, index: i64) {
    if !ui.get_slash_commands_current() {
        return;
    }
    let Some(item) = usize::try_from(index)
        .ok()
        .and_then(|index| ui.model_slash_command_items().row(index))
    else {
        return;
    };
    if !command_available(ui, &item.key) {
        let draft = ui.get_draft();
        update_slash(ui, draft);
        return;
    }
    ui.set_slash_commands_open(false);
    ui.set_draft(String::new());
    run_command(ui, &item.key);
}

pub fn attach(ui: &mut AgentWorkbench) {
    let search = Rc::new(RefCell::new(Search::default()));
    let weak = ui.as_weak();
    let state = search.clone();
    ui.on_begin_palette_search(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        {
            let mut search = state.borrow_mut();
            let generation = search.generation.wrapping_add(1);
            *search = Search {
                generation,
                workspace: ui.get_workspace(),
                ..Search::default()
            };
        }
        ui.model_palette_items().clear();
        update(&mut ui, state.clone());
    });
    let weak = ui.as_weak();
    let current = search.clone();
    ui.on_search_palette(move || {
        if let Some(mut ui) = weak.upgrade() {
            update(&mut ui, current.clone());
        }
    });
    let weak = ui.as_weak();
    ui.on_activate_palette_item(move |index| {
        if let Some(mut ui) = weak.upgrade() {
            let workspace = search.borrow().workspace.clone();
            activate(&mut ui, &workspace, index);
        }
    });
    let weak = ui.as_weak();
    ui.on_filter_slash_commands(move |text| {
        if let Some(mut ui) = weak.upgrade() {
            update_slash(&mut ui, text);
        }
    });
    let weak = ui.as_weak();
    ui.on_activate_slash_command(move |index| {
        if let Some(mut ui) = weak.upgrade() {
            activate_slash(&mut ui, index);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use xrack_layout::FocusObservers;
    use xrack_runtime::{Component, EventResult, KeyEvent, KeyPhase, Length};
    use xrack_testing::{Size, UiDriver};

    #[test]
    fn slash_picker_stays_with_the_editor_and_runs_the_selected_command() {
        let mut ui = AgentWorkbench::new();
        ui.set_workspace("/workspace".into());
        ui.set_connected(true);
        ui.set_renamable(true);
        ui.set_plan_available(true);
        ui.set_viewport_width(Length(680.0));
        ui.set_composer_enter_mode("enter".into());
        let weak = ui.as_weak();
        ui.on_choose_plan_mode(move |plan| weak.upgrade().unwrap().set_plan_mode(plan));
        let sent = Rc::new(RefCell::new(Vec::new()));
        let sent_messages = sent.clone();
        ui.on_send(move |text| sent_messages.borrow_mut().push(text));
        attach(&mut ui);

        let mut driver = UiDriver::headless(ui, Size::new(680.0, 620.0));
        let fill = |driver: &mut UiDriver<AgentWorkbench>, text: &str| {
            driver.fill_focused(text);
            FocusObservers::default().sync(driver.input(), driver.scene(), driver.layout(), true);
            driver.update(|_| {});
        };
        let press = |driver: &mut UiDriver<AgentWorkbench>, key: &str| {
            let scope = driver.path("composer-key-scope");
            let scene = driver.scene().clone();
            driver.update(|ui| {
                assert_eq!(
                    ui.key_event_at(
                        &scene,
                        &scope,
                        KeyPhase::CapturePressed,
                        &KeyEvent {
                            key: key.into(),
                            ..KeyEvent::default()
                        },
                    ),
                    EventResult::Accept,
                );
            });
        };

        driver.click("input");
        let original = driver.bounds("composer-panel");
        fill(&mut driver, "/");
        assert!(driver.node("slash-commands").is_visible());
        assert_eq!(driver.bounds("composer-panel"), original);
        assert_eq!(driver.focused(), Some("input"));
        let hint = driver.bounds("slash-command-hint");
        let list = driver.bounds("slash-command-results");
        assert!(hint.height >= 18.0 && hint.y + hint.height <= list.y);
        assert_eq!(
            driver
                .component()
                .model_slash_command_items()
                .row(0)
                .unwrap()
                .title,
            "/model"
        );
        press(&mut driver, "ArrowDown");
        press(&mut driver, "Enter");
        assert!(driver.component().get_plan_mode());
        assert_eq!(driver.component().get_draft(), "");
        assert!(!driver.node("slash-commands").is_visible());

        fill(&mut driver, "/MO");
        let row = driver.node("slash-command-results").children[0]
            .row_key
            .unwrap();
        let button = driver
            .find_row_path("slash-command-results", row, Some("palette-result-action"))
            .unwrap();
        driver.click_at_path(&button);
        assert!(driver.component().get_generation_picker_open());
        assert!(driver.component().get_draft().is_empty());
        driver.escape();

        driver.click("input");
        fill(&mut driver, "/review");
        assert!(driver.node("slash-commands").is_visible());
        press(&mut driver, "Escape");
        assert!(!driver.node("slash-commands").is_visible());
        assert_eq!(driver.component().get_draft(), "/review");
        assert!(!driver.component().get_review_scope_open());
        assert!(sent.borrow().is_empty());

        for text in ["/tmp", "/tmp/project", "/model with examples", "see /model"] {
            fill(&mut driver, text);
            assert!(!driver.node("slash-commands").is_visible());
        }
        fill(&mut driver, "/tmp/project");
        press(&mut driver, "Enter");
        assert_eq!(sent.borrow().as_slice(), ["/tmp/project"]);
        assert!(!driver.component().get_review_scope_open());
    }

    #[test]
    fn slash_filter_drops_commands_that_are_unavailable_during_a_turn() {
        let mut view = AgentWorkbench::new();
        view.set_workspace("/workspace".into());
        view.set_connected(true);
        update_slash(&mut view, "/re".into());
        assert_eq!(
            view.model_slash_command_items().row(0).unwrap().title,
            "/review"
        );
        view.set_busy(true);
        update_slash(&mut view, "/re".into());
        assert!(!view.get_slash_commands_open());
        assert!(view.model_slash_command_items().is_empty());
    }

    #[test]
    fn git_search_respects_ignored_files_and_the_selected_subdirectory()
    -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path();
        git(root, &["init", "-q"], 1024)?;
        fs::create_dir(root.join("src"))?;
        fs::write(root.join(".gitignore"), "*.log\n")?;
        fs::write(root.join("src/main.rs"), "fn main() {}")?;
        fs::write(root.join("src/main-old.rs"), "")?;
        fs::write(root.join("src/private.log"), "")?;
        fs::write(root.join("outside.rs"), "")?;
        git(root, &["add", "src/main.rs"], 1024)?;

        let index = FileIndex::build(root)?;
        assert_eq!(
            index
                .search("MAIN.RS", RESULT_LIMIT)
                .first()
                .map(String::as_str),
            Some("src/main.rs")
        );
        assert_eq!(
            index.search("mainrs", RESULT_LIMIT),
            ["src/main.rs", "src/main-old.rs"]
        );
        assert!(index.search("private", RESULT_LIMIT).is_empty());
        let child = FileIndex::build(&root.join("src"))?;
        assert_eq!(
            child.search("main.rs:8-10", RESULT_LIMIT),
            ["main.rs", "main-old.rs"]
        );
        assert!(child.search("outside", RESULT_LIMIT).is_empty());
        Ok(())
    }

    #[test]
    fn non_git_directories_are_searchable_and_symlinks_stay_out_of_results()
    -> Result<(), Box<dyn std::error::Error>> {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().join("workspace");
        fs::create_dir_all(root.join("文档"))?;
        fs::write(root.join("文档/接口.md"), "")?;
        #[cfg(unix)]
        {
            let external = tmp.path().join("other.txt");
            fs::write(&external, "other")?;
            std::os::unix::fs::symlink(external, root.join("linked.txt"))?;
        }
        let index = FileIndex::build(&root)?;
        assert_eq!(index.search("接口", RESULT_LIMIT), ["文档/接口.md"]);
        assert!(index.search("linked", RESULT_LIMIT).is_empty());
        assert_eq!(
            file_query("接口.md#L4-L6"),
            ("接口.md", Some(LineRange { first: 4, last: 6 }))
        );
        Ok(())
    }

    #[test]
    fn file_selection_waits_for_current_query_and_workspace() {
        let mut ui = AgentWorkbench::new();
        ui.set_palette_open(true);
        ui.set_palette_file_mode(true);
        ui.set_palette_ready(true);
        ui.set_workspace("/workspace".into());
        ui.set_palette_query("main.rs:20".into());
        ui.set_palette_result_query("old".into());
        ui.model_palette_items().push(PaletteItem {
            key: "src/main.rs".into(),
            title: "main.rs".into(),
            detail: "src".into(),
            enabled: true,
        });
        let events = Rc::new(RefCell::new(Vec::new()));
        let received = events.clone();
        ui.on_open_quick_file(move |path, first, last| {
            received.borrow_mut().push((path, first, last))
        });
        activate(&mut ui, "/workspace", 0);
        ui.set_palette_result_query("main.rs:20".into());
        activate(&mut ui, "/another", 0);
        assert!(events.borrow().is_empty());
        activate(&mut ui, "/workspace", 0);
        assert_eq!(events.borrow().as_slice(), [("src/main.rs".into(), 20, 20)]);
        assert!(!ui.get_palette_open());
    }

    #[test]
    fn commands_are_rechecked_before_running_and_quick_open_can_switch_modes() {
        let mut ui = AgentWorkbench::new();
        ui.set_workspace("/workspace".into());
        ui.set_connected(true);
        attach(&mut ui);
        ui.invoke_open_workbench_palette(false);
        ui.set_palette_query("审阅".into());
        ui.invoke_search_palette();
        assert_eq!(ui.model_palette_items().row(0).unwrap().key, "review-code");
        ui.set_busy(true);
        ui.invoke_activate_palette_item(0);
        assert!(ui.get_palette_open());
        assert!(!ui.get_review_scope_open());
        ui.set_busy(false);
        ui.invoke_activate_palette_item(0);
        assert!(!ui.get_palette_open());
        assert!(ui.get_review_scope_open());

        ui.set_review_scope_open(false);
        ui.set_review_scope_loading(false);
        ui.invoke_open_workbench_palette(false);
        ui.set_palette_query("快速打开".into());
        ui.invoke_search_palette();
        ui.invoke_activate_palette_item(0);
        assert!(ui.get_palette_open() && ui.get_palette_file_mode());
    }
}
