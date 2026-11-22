use crate::{
    read_in_background,
    settings::{read_text, write_text},
    workspace_files::locate_external_file,
};
use serde::{Deserialize, Serialize};
use session::Snapshot;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    env,
    ffi::OsString,
    io::{Read, Seek},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
    thread,
    time::{Duration, Instant},
};
use tokio::sync::watch;
use workbench_ui::AgentWorkbench;

const PREFERENCES_FILE: &str = "external-open.toml";

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Destination {
    #[default]
    Vscode,
    Cursor,
    Files,
}

impl Destination {
    fn key(self) -> &'static str {
        match self {
            Self::Vscode => "vscode",
            Self::Cursor => "cursor",
            Self::Files => "files",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        [Self::Vscode, Self::Cursor, Self::Files]
            .into_iter()
            .find(|candidate| candidate.key() == value)
    }

    #[cfg(not(windows))]
    fn executable(self) -> &'static str {
        match self {
            Self::Vscode => "code",
            Self::Cursor => "cursor",
            Self::Files => "xdg-open",
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Preferences {
    projects: BTreeMap<String, Destination>,
}

impl Preferences {
    fn load(directory: &Path) -> Result<Self, String> {
        read_text(directory, PREFERENCES_FILE, 256 * 1024)
            .map_err(|error| error.to_string())?
            .map_or_else(
                || Ok(Self::default()),
                |text| toml::from_str(&text).map_err(|error| error.to_string()),
            )
    }

    fn save(&self, directory: &Path) -> Result<(), String> {
        let text = toml::to_string(self).map_err(|error| error.to_string())?;
        write_text(directory, PREFERENCES_FILE, &text).map_err(|error| error.to_string())
    }
}

#[derive(Clone, Copy)]
enum Origin {
    Project,
    File,
    Diff,
}

#[derive(Clone)]
struct Target {
    workspace: String,
    project: String,
    conversation: i64,
    origin: Origin,
    path: String,
    line: Option<usize>,
    column: Option<usize>,
}

impl Target {
    fn resolve(&self) -> Result<PathBuf, String> {
        match self.origin {
            Origin::Project => Path::new(&self.workspace)
                .canonicalize()
                .map_err(|error| error.to_string())
                .and_then(|folder| {
                    folder
                        .is_dir()
                        .then_some(folder)
                        .ok_or_else(|| "工作目录已不可用".into())
                }),
            Origin::File | Origin::Diff => locate_external_file(
                &self.workspace,
                &self.path,
                matches!(self.origin, Origin::Diff),
            ),
        }
    }

    fn is_current(&self, ui: &AgentWorkbench) -> bool {
        ui.get_workspace() == self.workspace && ui.get_conversation_id() == self.conversation
    }

    fn label(&self) -> String {
        match self.line {
            Some(line) => match self.column {
                Some(column) => format!("{}:{line}:{column}", self.path),
                None => format!("{}:{line}", self.path),
            },
            None => self.path.clone(),
        }
    }
}

pub(super) fn find_program(name: &str) -> Option<PathBuf> {
    let paths = env::var_os("PATH")?;
    env::split_paths(&paths)
        .filter(|path| path.is_absolute())
        .map(|path| path.join(name))
        .find(|path| path.is_file())
}

#[cfg(not(windows))]
fn editor_command(destination: Destination) -> Result<Command, String> {
    let name = destination.executable();
    #[cfg(target_os = "macos")]
    {
        let bundle = match destination {
            Destination::Vscode => "Visual Studio Code.app",
            Destination::Cursor => "Cursor.app",
            Destination::Files => unreachable!("file manager uses a separate command"),
        };
        let relative = Path::new(bundle)
            .join("Contents/Resources/app/bin")
            .join(name);
        let folders = [
            Some(PathBuf::from("/Applications")),
            env::home_dir().map(|home| home.join("Applications")),
        ];
        if let Some(launcher) = folders
            .into_iter()
            .flatten()
            .map(|folder| folder.join(&relative))
            .find(|path| path.is_file())
        {
            return Ok(Command::new(launcher));
        }
    }
    find_program(name)
        .map(Command::new)
        .ok_or_else(|| format!("{name} is unavailable"))
}

#[cfg(windows)]
fn editor_command(destination: Destination) -> Result<Command, String> {
    let (folder, executable) = match destination {
        Destination::Vscode => ("Microsoft VS Code", "Code.exe"),
        Destination::Cursor => ("cursor", "Cursor.exe"),
        Destination::Files => unreachable!("file manager uses a separate command"),
    };
    let user_programs =
        env::var_os("LOCALAPPDATA").map(|local| PathBuf::from(local).join("Programs"));
    let system_programs = env::var_os("ProgramFiles").map(PathBuf::from);
    let from_install = [user_programs, system_programs]
        .into_iter()
        .flatten()
        .map(|root| root.join(folder).join(executable));
    for binary in from_install.chain(find_program(executable)) {
        let Some(root) = binary.parent() else {
            continue;
        };
        let cli = root.join("resources/app/out/cli.js");
        if binary.is_file() && cli.is_file() {
            let mut command = Command::new(&binary);
            command.env("ELECTRON_RUN_AS_NODE", "1").arg(cli);
            return Ok(command);
        }
    }
    Err(format!("{executable} is unavailable"))
}

fn editor_args(path: &Path, line: Option<usize>, column: Option<usize>) -> Vec<OsString> {
    let mut location = path.as_os_str().to_owned();
    if let Some(line) = line {
        location.push(format!(":{line}"));
        if let Some(column) = column {
            location.push(format!(":{column}"));
        }
        return vec!["--goto".into(), location];
    }
    vec![location]
}

#[cfg(target_os = "macos")]
fn file_manager(path: &Path, directory: bool) -> Result<Command, String> {
    let mut command = Command::new("/usr/bin/open");
    if !directory {
        command.arg("-R");
    }
    command.arg(path);
    Ok(command)
}

#[cfg(windows)]
fn file_manager(path: &Path, directory: bool) -> Result<Command, String> {
    use std::os::windows::process::CommandExt;
    let windows = env::var_os("SystemRoot").ok_or("SystemRoot is unavailable")?;
    let mut command = Command::new(Path::new(&windows).join("explorer.exe"));
    if directory {
        command.arg(path);
    } else {
        let mut select = OsString::from("/select,\"");
        select.push(path);
        select.push("\"");
        command.raw_arg(select);
    }
    Ok(command)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn file_manager(path: &Path, directory: bool) -> Result<Command, String> {
    let executable = find_program("xdg-open").ok_or("xdg-open is unavailable")?;
    let folder = if directory {
        path
    } else {
        path.parent().ok_or("missing parent directory")?
    };
    let mut command = Command::new(executable);
    command.arg(folder);
    Ok(command)
}

fn launch(target: &Target, destination: Destination) -> Result<(), String> {
    let path = target.resolve()?;
    let mut command = if destination == Destination::Files {
        file_manager(&path, matches!(target.origin, Origin::Project))?
    } else {
        let mut editor = editor_command(destination)?;
        editor.args(editor_args(&path, target.line, target.column));
        editor
    };
    let mut log = tempfile::tempfile().map_err(|error| error.to_string())?;
    let stderr = log.try_clone().map_err(|error| error.to_string())?;
    let mut child = command
        .current_dir(&target.workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .map_err(|error| error.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            if status.success() || cfg!(windows) && destination == Destination::Files {
                return Ok(());
            }
            let mut bytes = Vec::new();
            if log.rewind().is_ok() {
                let _ = log.take(2048).read_to_end(&mut bytes);
            }
            let detail = String::from_utf8_lossy(&bytes);
            return Err(detail
                .lines()
                .find(|line| !line.trim().is_empty())
                .map_or_else(|| status.to_string(), ToOwned::to_owned));
        }
        if Instant::now() >= deadline {
            // Some desktop openers run until the destination window is closed.
            thread::spawn(move || {
                let _ = child.wait();
            });
            return Ok(());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

pub(super) fn attach(
    ui: &mut AgentWorkbench,
    directory: Option<&Path>,
    snapshots: watch::Receiver<Snapshot>,
) {
    let directory = directory.map(Path::to_owned);
    let loaded = directory.as_deref().map(Preferences::load).transpose();
    let (preferences, load_error) = match loaded {
        Ok(preferences) => (preferences.unwrap_or_default(), String::new()),
        Err(error) => (Preferences::default(), error),
    };
    let preferences = Rc::new(RefCell::new(preferences));
    let proposal = Rc::new(RefCell::new(None::<Target>));
    let weak = ui.as_weak();
    ui.on_open_diff_external(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        let line = if ui.get_review_line_old() {
            0
        } else {
            ui.model_review_lines()
                .snapshot()
                .iter()
                .find(|line| line.id == ui.get_review_line_id())
                .map_or(0, |line| line.new_line.max(0))
        };
        ui.invoke_present_external("diff".into(), ui.get_review_selection(), line, 0);
    });
    let weak = ui.as_weak();
    let proposed = proposal.clone();
    let current = preferences.clone();
    let can_persist = directory.is_some();
    ui.on_present_external(move |origin, path, line, column| {
        let Some(mut ui) = weak.upgrade() else { return };
        let workspace = ui.get_workspace();
        if workspace.is_empty() || ui.get_switching() || ui.get_external_open_pending() {
            return;
        }
        let (origin, path) = match origin.as_str() {
            "project" => (Origin::Project, workspace.clone()),
            "file" => (Origin::File, path),
            "diff" if ui.get_review_comparison_mode().is_empty() => (Origin::Diff, path),
            _ => return,
        };
        let conversation = ui.get_conversation_id();
        let snapshot = snapshots.borrow();
        let project = if snapshot.view.conversation == conversation
            && snapshot.view.workspace == workspace
            && !snapshot.project_workspace().is_empty()
        {
            snapshot.project_workspace().into()
        } else {
            workspace.clone()
        };
        let line = usize::try_from(line).ok().filter(|line| *line > 0);
        let column = line.and(usize::try_from(column).ok().filter(|column| *column > 0));
        let target = Target {
            workspace,
            project,
            conversation,
            origin,
            path,
            line,
            column,
        };
        let preferred = current
            .borrow()
            .projects
            .get(&target.project)
            .copied()
            .unwrap_or_default();
        ui.set_external_open_path(target.label());
        ui.set_external_destination(preferred.key().into());
        ui.set_external_remember(false);
        ui.set_external_can_remember(can_persist);
        ui.set_external_open_error(load_error.clone());
        proposed.replace(Some(target));
        ui.set_external_open(true);
    });

    let weak = ui.as_weak();
    ui.on_confirm_external_open(move || {
        let Some(mut ui) = weak.upgrade() else { return };
        if !ui.get_external_open() || ui.get_external_open_pending() || ui.get_switching() {
            return;
        }
        let Some(target) = proposal
            .borrow()
            .as_ref()
            .filter(|target| target.is_current(&ui))
            .cloned()
        else {
            ui.set_external_open(false);
            return;
        };
        let Some(destination) = Destination::parse(&ui.get_external_destination()) else {
            return;
        };
        let settings =
            if let Some(directory) = directory.as_ref().filter(|_| ui.get_external_remember()) {
                let mut next = preferences.borrow().clone();
                next.projects.insert(target.project.clone(), destination);
                Some((directory.clone(), next))
            } else {
                None
            };
        ui.set_external_open_pending(true);
        ui.set_external_open_error(String::new());
        let preferences = preferences.clone();
        let completion = weak.clone();
        let captured = target.clone();
        if let Err(error) = xrack_platform_winit::spawn_local(async move {
            let outcome = read_in_background("workbench-open", move || {
                launch(&captured, destination)?;
                Ok(settings.map(|(directory, settings)| {
                    let error = settings.save(&directory).err();
                    (settings, error)
                }))
            })
            .await;
            let Some(mut ui) = completion.upgrade() else {
                return;
            };
            ui.set_external_open_pending(false);
            if !target.is_current(&ui) {
                ui.set_external_open(false);
                return;
            }
            match outcome {
                Ok(saved) => {
                    if let Some((settings, error)) = saved {
                        preferences.replace(settings);
                        if let Some(error) = error {
                            crate::show_status(
                                &mut ui,
                                format!("已打开，保存首选项失败（{error}）"),
                            );
                        }
                    }
                    ui.set_external_open(false);
                }
                Err(error) => ui.set_external_open_error(error),
            }
        }) {
            ui.set_external_open_pending(false);
            ui.set_external_open_error(error.to_string());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn revalidates_a_file_inside_the_selected_workspace_and_keeps_exact_argv()
    -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let workspace = temporary.path().join("work");
        fs::create_dir(&workspace)?;
        let file = workspace.join(if cfg!(windows) {
            "界 面 & literal.rs"
        } else {
            "界 面 & $(literal).rs"
        });
        fs::write(&file, "source")?;
        let outside = temporary.path().join("outside.rs");
        fs::write(&outside, "elsewhere")?;
        let mut target = Target {
            workspace: workspace.to_string_lossy().into(),
            project: workspace.to_string_lossy().into(),
            conversation: 0,
            origin: Origin::File,
            path: file.file_name().unwrap().to_string_lossy().into(),
            line: Some(12),
            column: Some(5),
        };
        let resolved = target.resolve()?;
        assert_eq!(resolved, file.canonicalize()?);
        let mut location = resolved.as_os_str().to_os_string();
        location.push(":12:5");
        assert_eq!(
            editor_args(&resolved, target.line, target.column),
            ["--goto".into(), location]
        );
        assert_eq!(editor_args(&resolved, None, None), [resolved.as_os_str()]);

        target.path = outside.to_string_lossy().into();
        assert!(target.resolve().is_err());
        target.path = "../outside.rs".into();
        assert!(target.resolve().is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside, workspace.join("link.rs"))?;
            target.path = "link.rs".into();
            assert!(target.resolve().is_err());
        }
        Ok(())
    }

    #[test]
    fn restores_the_project_preference_and_discards_stale_targets_before_launch()
    -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let project = temporary
            .path()
            .join("A project")
            .to_string_lossy()
            .into_owned();
        let other = temporary
            .path()
            .join("B project")
            .to_string_lossy()
            .into_owned();
        Preferences {
            projects: BTreeMap::from([(project.clone(), Destination::Cursor)]),
        }
        .save(temporary.path())?;
        let mut ui = AgentWorkbench::new();
        ui.set_workspace(project.clone());
        let (_sender, snapshots) = watch::channel(Snapshot::connecting(project));
        attach(&mut ui, Some(temporary.path()), snapshots);
        ui.invoke_present_external("file".into(), "a file.rs".into(), 22, 4);
        assert!(ui.get_external_open());
        assert_eq!(ui.get_external_open_path(), "a file.rs:22:4");
        assert_eq!(ui.get_external_destination(), "cursor");

        ui.set_workspace(other);
        ui.invoke_confirm_external_open();
        assert!(!ui.get_external_open());
        assert!(!ui.get_external_open_pending());
        ui.invoke_present_external("project".into(), String::new(), 0, 0);
        assert!(ui.get_external_open());
        assert_eq!(ui.get_external_destination(), "vscode");
        Ok(())
    }
}
