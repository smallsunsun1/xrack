use portable_pty::CommandBuilder;
use serde::Deserialize;
use std::{
    cell::RefCell,
    collections::HashSet,
    fs::File,
    io,
    path::{Component, Path, PathBuf},
    rc::Rc,
};
use workbench_ui::{AgentWorkbench, GenerationChoice};

const CONFIG_PATH: &str = ".xrack/actions.toml";
const CONFIG_LIMIT: usize = 64 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    id: String,
    name: String,
    argv: Vec<String>,
    cwd: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    #[serde(default, rename = "action")]
    entries: Vec<Definition>,
}

#[derive(Clone, Debug)]
pub(super) struct Action {
    id: String,
    pub(super) name: String,
    invocation: Invocation,
    cwd: PathBuf,
}

#[derive(Clone, Debug)]
enum Invocation {
    Argv(Vec<String>),
    Script(String),
}

impl Action {
    pub(super) fn command(&self) -> CommandBuilder {
        let argv = match &self.invocation {
            Invocation::Argv(argv) => argv,
            Invocation::Script(script) => {
                return crate::environment::action_command(script, &self.cwd);
            }
        };
        let mut command = CommandBuilder::new(&argv[0]);
        command.args(&argv[1..]);
        command.cwd(&self.cwd);
        command
    }

    fn preview(&self) -> String {
        let argv = match &self.invocation {
            Invocation::Argv(argv) => argv,
            Invocation::Script(script) => return script.clone(),
        };
        argv.iter()
            .map(|arg| {
                if !arg.is_empty()
                    && arg
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"_./:@=+-".contains(&byte))
                {
                    arg.clone()
                } else {
                    format!("{arg:?}")
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn load(workspace: &Path) -> Result<Vec<Action>, String> {
    let root = workspace
        .canonicalize()
        .map_err(|error| format!("无法访问工作目录（{error}）"))?;
    let path = match root.join(CONFIG_PATH).canonicalize() {
        Ok(path) if path.starts_with(&root) => path,
        Ok(_) => return Err("操作清单须位于当前工作目录内".into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("无法打开操作清单（{error}）")),
    };
    if !path.is_file() {
        return Err("操作清单须是普通文件".into());
    }
    let file = File::open(path).map_err(|error| format!("无法打开操作清单（{error}）"))?;
    let (bytes, truncated) = crate::review::read_limited(file, CONFIG_LIMIT)
        .map_err(|error| format!("无法读取操作清单（{error}）"))?;
    if truncated {
        return Err("操作清单超过 64 KiB".into());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "操作清单须使用 UTF-8")?;
    let catalog: Catalog =
        toml::from_str(text).map_err(|error| format!("操作清单格式有误（{error}）"))?;
    let mut ids = HashSet::new();
    catalog
        .entries
        .into_iter()
        .map(|entry| {
            if entry.id.is_empty()
                || !entry
                    .id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                || !ids.insert(entry.id.clone())
            {
                return Err("每项操作须使用唯一 ID，允许字母、数字、横线和下划线".into());
            }
            let name = entry.name.trim();
            if name.is_empty() || name.chars().any(char::is_control) {
                return Err(format!("操作 {} 的名称无效", entry.id));
            }
            if entry.argv.is_empty()
                || entry.argv[0].trim().is_empty()
                || entry.argv.iter().any(|arg| arg.contains('\0'))
            {
                return Err(format!("操作 {} 的命令参数无效", entry.id));
            }
            let relative = entry.cwd.as_deref().unwrap_or_else(|| Path::new("."));
            if !relative
                .components()
                .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
            {
                return Err(format!("操作 {} 的运行目录须位于当前工作目录内", entry.id));
            }
            let cwd = root
                .join(relative)
                .canonicalize()
                .map_err(|error| format!("操作 {} 的运行目录无法打开（{error}）", entry.id))?;
            if !cwd.is_dir() || !cwd.starts_with(&root) {
                return Err(format!("操作 {} 的运行目录须位于当前工作目录内", entry.id));
            }
            Ok(Action {
                id: entry.id,
                name: name.into(),
                invocation: Invocation::Argv(entry.argv),
                cwd,
            })
        })
        .collect()
}

fn load_project(workspace: &Path) -> Result<(Vec<Action>, String), String> {
    let mut actions = load(workspace)?;
    let root = crate::environment::project_root(workspace)?;
    let mut warning = String::new();
    match crate::environment::profiles(&root) {
        Ok(profiles) => {
            for (profile_index, profile) in profiles.into_iter().enumerate() {
                for (index, action) in profile.actions.into_iter().enumerate() {
                    actions.push(Action {
                        id: format!("codex:{profile_index}:{index}"),
                        name: format!("{} · {}", action.name, profile.label),
                        invocation: Invocation::Script(action.command),
                        cwd: root.clone(),
                    });
                }
            }
        }
        Err(error) => warning = error,
    }
    Ok((actions, warning))
}

#[derive(Default)]
struct Loaded {
    revision: u64,
    workspace: String,
    items: Vec<Action>,
}

impl Loaded {
    fn selected(&self, ui: &AgentWorkbench) -> Option<&Action> {
        if !ui.get_project_actions_open() || self.workspace != ui.get_workspace() {
            return None;
        }
        let id = ui.get_project_action_id();
        self.items.iter().find(|item| item.id == id)
    }

    fn preview(&self, ui: &mut AgentWorkbench) {
        let current = self.selected(ui);
        ui.set_project_action_command(current.map_or_else(String::new, Action::preview));
        ui.set_project_action_directory(current.map_or_else(String::new, |action| {
            action.cwd.to_string_lossy().into_owned()
        }));
    }
}

pub(super) fn attach(ui: &mut AgentWorkbench, run: impl Fn(&mut AgentWorkbench, Action) + 'static) {
    let loaded = Rc::new(RefCell::new(Loaded::default()));
    let state = loaded.clone();
    let weak = ui.as_weak();
    ui.on_load_project_actions(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        let selected = ui.get_project_action_id();
        let workspace = ui.get_workspace();
        let revision = {
            let mut state = state.borrow_mut();
            state.revision = state.revision.wrapping_add(1);
            state.workspace.clear();
            state.items.clear();
            state.revision
        };
        ui.set_project_actions_loading(true);
        ui.set_project_actions_status(String::new());
        ui.model_project_actions().clear();
        state.borrow().preview(&mut ui);
        let state = state.clone();
        let weak = weak.clone();
        if let Err(error) = xrack_platform_winit::spawn_local(async move {
            let folder = workspace.clone();
            let result = crate::read_in_background("xrack-project-actions", move || {
                load_project(Path::new(&folder))
            })
            .await;
            let Some(mut ui) = weak.upgrade() else { return };
            if state.borrow().revision != revision
                || !ui.get_project_actions_open()
                || ui.get_workspace() != workspace
            {
                return;
            }
            ui.set_project_actions_loading(false);
            match result {
                Ok((items, warning)) => {
                    let mut state = state.borrow_mut();
                    state.workspace = workspace;
                    state.items = items;
                    if !warning.is_empty() {
                        ui.set_project_actions_status(warning);
                    } else if state.items.is_empty() {
                        ui.set_project_actions_status(format!(
                            "在 {CONFIG_PATH} 或 .codex/environments 下定义项目操作"
                        ));
                    }
                    let saved = state
                        .items
                        .iter()
                        .find(|action| action.id == selected)
                        .or_else(|| state.items.first());
                    for action in &state.items {
                        ui.model_project_actions().push(GenerationChoice {
                            key: action.id.clone(),
                            label: action.name.clone(),
                        });
                    }
                    ui.set_project_action_id(
                        saved.map_or_else(String::new, |action| action.id.clone()),
                    );
                    state.preview(&mut ui);
                }
                Err(error) => ui.set_project_actions_status(error),
            }
        }) {
            ui.set_project_actions_loading(false);
            ui.set_project_actions_status(error.to_string());
        }
    });
    let state = loaded.clone();
    let weak = ui.as_weak();
    ui.on_select_project_action(move || {
        if let Some(mut ui) = weak.upgrade() {
            state.borrow().preview(&mut ui);
        }
    });
    let weak = ui.as_weak();
    ui.on_run_project_action(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        if ui.get_switching() || ui.get_project_actions_loading() {
            return;
        }
        let action = loaded.borrow().selected(&ui).cloned();
        if let Some(action) = action {
            run(&mut ui, action);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn combines_xrack_actions_with_the_project_environment()
    -> Result<(), Box<dyn std::error::Error>> {
        let scratch = tempfile::tempdir()?;
        let root = scratch.path().canonicalize()?;
        let profiles = root.join(".codex/environments");
        fs::create_dir_all(&profiles)?;
        fs::create_dir(root.join(".xrack"))?;
        fs::write(
            root.join(CONFIG_PATH),
            r#"[[action]]
id = "check"
name = "Check"
argv = ["bazel", "test", "//..."]
"#,
        )?;
        fs::write(
            profiles.join("project.toml"),
            r#"version = 1
name = "Project"
[[actions]]
name = "Run"
command = 'printf "project run"'
"#,
        )?;
        let (actions, warning) = load_project(&root)?;
        assert!(warning.is_empty());
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].preview(), "bazel test //...");
        assert_eq!(actions[1].name, "Run · Project · project.toml");
        assert_eq!(actions[1].preview(), r#"printf "project run""#);
        assert_eq!(actions[1].command().get_cwd(), Some(&root.into_os_string()));
        Ok(())
    }

    #[test]
    fn loads_action_arguments_and_enforces_workspace_relative_directories()
    -> Result<(), Box<dyn std::error::Error>> {
        let scratch = tempfile::tempdir()?;
        let root = scratch.path().canonicalize()?;
        assert!(load(&root)?.is_empty());
        fs::create_dir_all(root.join(".xrack"))?;
        fs::create_dir(root.join("nested folder"))?;
        let config = root.join(CONFIG_PATH);
        let fixture = r#"
[[action]]
id = "check"
name = "测试"
argv = ["bazel", "test", "//tests:expressions_test", "--test_arg=with spaces"]
cwd = "nested folder"
"#;
        fs::write(&config, fixture)?;
        let loaded = load(&root)?;
        let action = &loaded[0];
        assert_eq!(action.name, "测试");
        assert_eq!(
            action.preview(),
            "bazel test //tests:expressions_test \"--test_arg=with spaces\""
        );
        assert_eq!(
            action.command().get_cwd(),
            Some(&root.join("nested folder").into_os_string())
        );
        fs::write(&config, format!("{fixture}{fixture}"))?;
        assert!(load(&root).unwrap_err().contains("唯一 ID"));
        fs::write(&config, fixture.replace("nested folder", ".."))?;
        assert!(load(&root).unwrap_err().contains("当前工作目录内"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn configured_action_uses_its_own_pty_working_directory_and_exit_status()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::terminal::process::Session;
        use portable_pty::PtySize;
        use std::{
            thread,
            time::{Duration, Instant},
        };
        let scratch = tempfile::tempdir()?;
        let root = scratch.path().canonicalize()?;
        fs::create_dir_all(root.join(".xrack"))?;
        fs::create_dir(root.join("build"))?;
        fs::write(
            root.join(CONFIG_PATH),
            r#"
[[action]]
id = "check"
name = "构建检查"
argv = ["/bin/sh", "-c", 'printf "%s\n" "$(pwd -P)" "$1"; exec 0<&- 1>&- 2>&-; sleep 0.05; exit 7', "xrack-action", "literal;$(false)"]
cwd = "build"
"#,
        )?;
        let loaded = load(&root)?;
        let action = &loaded[0];
        let session = Session::start(
            action.command(),
            PtySize {
                rows: 12,
                cols: 200,
                ..PtySize::default()
            },
        )?;
        let expected = root.join("build");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let frame = session.output.borrow();
            if !frame.running && frame.status.contains("已退出") {
                let text = frame.screen.text();
                assert!(text.contains(expected.to_str().unwrap()));
                assert!(text.contains("literal;$(false)"));
                assert!(frame.status.contains('7'));
                break;
            }
            assert!(
                Instant::now() < deadline,
                "{} {:?}",
                frame.status,
                frame.screen.text()
            );
            drop(frame);
            thread::sleep(Duration::from_millis(12));
        }
        Ok(())
    }
}
