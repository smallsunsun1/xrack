use crate::review::{git_root, read_limited};
use portable_pty::CommandBuilder;
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

const DIRECTORY: &str = ".codex/environments";
const LIMIT: usize = 128 * 1024;
const OUTPUT_LIMIT: u64 = 32 * 1024;

#[derive(Default, Deserialize)]
struct Script {
    script: Option<String>,
}

#[derive(Default, Deserialize)]
struct Setup {
    script: Option<String>,
    #[serde(default)]
    darwin: Script,
    #[serde(default)]
    linux: Script,
    #[serde(default)]
    win32: Script,
}

impl Setup {
    fn selected(self) -> Option<String> {
        let local = match platform() {
            "darwin" => self.darwin.script,
            "linux" => self.linux.script,
            "win32" => self.win32.script,
            _ => None,
        };
        local
            .or(self.script)
            .filter(|script| !script.trim().is_empty())
    }
}

#[derive(Deserialize)]
pub(super) struct Action {
    pub name: String,
    pub command: String,
    platform: Option<String>,
}

#[derive(Deserialize)]
struct Manifest {
    version: u64,
    name: String,
    #[serde(default)]
    setup: Setup,
    #[serde(default)]
    actions: Vec<Action>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SetupSelection {
    pub label: String,
    pub script: String,
}

pub(super) struct Profile {
    pub label: String,
    pub setup: Option<SetupSelection>,
    pub actions: Vec<Action>,
}

pub(super) fn project_root(workspace: &Path) -> Result<PathBuf, String> {
    git_root(workspace)
        .unwrap_or_else(|_| workspace.into())
        .canonicalize()
        .map_err(|error| error.to_string())
}

pub(super) fn profiles(root: &Path) -> Result<Vec<Profile>, String> {
    let directory = match root.join(DIRECTORY).canonicalize() {
        Ok(path) if path.starts_with(root) => path,
        Ok(_) => return Err("环境配置须位于项目目录内".into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("无法读取环境配置目录（{error}）")),
    };
    let mut files = Vec::new();
    for entry in fs::read_dir(&directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "toml")
            && entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_file()
        {
            files.push(entry.path());
        }
    }
    files.sort();
    files.iter().map(|file| read_profile(root, file)).collect()
}

fn read_profile(root: &Path, file: &Path) -> Result<Profile, String> {
    let file = file.canonicalize().map_err(|error| error.to_string())?;
    if !file.starts_with(root) || !file.is_file() {
        return Err("环境配置须是项目目录内的普通文件".into());
    }
    let name = file
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("环境配置路径必须是 UTF-8")?;
    let input = File::open(&file).map_err(|error| format!("无法读取 {name}（{error}）"))?;
    let (bytes, truncated) = read_limited(input, LIMIT).map_err(|error| error.to_string())?;
    if truncated {
        return Err(format!("环境配置 {name} 过长"));
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "环境配置必须使用 UTF-8")?;
    let manifest: Manifest = toml::from_str(text)
        .map_err(|error| format!("环境配置 {name} 格式有误（{}）", error.message()))?;
    if manifest.version != 1
        || manifest.name.trim().is_empty()
        || manifest.name.chars().any(char::is_control)
    {
        return Err(format!("环境配置 {name} 需要版本 1 和有效的名称"));
    }
    let label = format!("{} · {name}", manifest.name.trim());
    let setup = manifest
        .setup
        .selected()
        .map(|script| {
            if script.contains('\0') {
                Err(format!("环境 {name} 的初始化命令包含无效字符"))
            } else {
                Ok(SetupSelection {
                    label: label.clone(),
                    script,
                })
            }
        })
        .transpose()?;
    let mut actions = Vec::<Action>::new();
    let mut indices = HashMap::<String, usize>::new();
    for action in manifest.actions {
        if action
            .platform
            .as_deref()
            .is_some_and(|target| target != platform())
        {
            continue;
        }
        if action.name.trim().is_empty()
            || action.name.chars().any(char::is_control)
            || action.command.trim().is_empty()
            || action.command.contains('\0')
        {
            return Err(format!("环境 {name} 有无效的项目操作"));
        }
        if let Some(&index) = indices.get(&action.name) {
            if action.platform.is_some() {
                actions[index] = action;
            }
        } else {
            indices.insert(action.name.clone(), actions.len());
            actions.push(action);
        }
    }
    Ok(Profile {
        label,
        setup,
        actions,
    })
}

fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(windows) {
        "win32"
    } else {
        std::env::consts::OS
    }
}

fn shell() -> (&'static str, &'static [&'static str]) {
    if cfg!(windows) {
        ("cmd.exe", &["/D", "/S", "/C"])
    } else {
        ("/bin/bash", &["-lc"])
    }
}

pub(super) fn action_command(script: &str, directory: &Path) -> CommandBuilder {
    let (shell, flags) = shell();
    let mut command = CommandBuilder::new(shell);
    command.args(flags.iter().copied());
    command.arg(script);
    command.cwd(directory);
    command
}

pub(super) struct SetupFailure {
    pub message: String,
    pub output: String,
}

impl From<io::Error> for SetupFailure {
    fn from(error: io::Error) -> Self {
        Self {
            message: error.to_string(),
            output: String::new(),
        }
    }
}

pub(super) fn run_setup(
    setup: &SetupSelection,
    source: &Path,
    checkout: &Path,
) -> Result<(), SetupFailure> {
    let (shell, flags) = shell();
    let mut output = tempfile::tempfile()?;
    let status = Command::new(shell)
        .args(flags)
        .arg(&setup.script)
        .current_dir(checkout)
        .env("CODEX_SOURCE_TREE_PATH", source)
        .env("CODEX_WORKTREE_PATH", checkout)
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(output.try_clone()?)
        .status()?;
    if status.success() {
        return Ok(());
    }
    let mut bytes = Vec::new();
    let from = output.metadata()?.len().saturating_sub(OUTPUT_LIMIT);
    output.seek(SeekFrom::Start(from))?;
    output.take(OUTPUT_LIMIT).read_to_end(&mut bytes)?;
    Err(SetupFailure {
        message: format!("环境初始化失败（{status}）"),
        output: String::from_utf8_lossy(&bytes).into_owned(),
    })
}
