use crate::{
    review::{git, git_command, read_limited},
    workspace_files::allowed_relative,
};
use std::{
    collections::BTreeSet,
    fs::{self, File, Metadata},
    io::{self, Seek, Write},
    path::{Path, PathBuf},
    process::Stdio,
};

const MANIFEST: &str = ".worktreeinclude";
const MANIFEST_LIMIT: usize = 256 * 1024;
const LIST_LIMIT: usize = 8 * 1024 * 1024;

pub(super) struct IncludedFiles(Vec<PathBuf>);

impl IncludedFiles {
    pub(super) fn read(repository: &Path) -> Result<Self, String> {
        let manifest = manifest_snapshot(repository)?;
        let mut args = vec!["ls-files", "-z", "--others", "--ignored"];
        let exclude = manifest
            .as_ref()
            .map(|file| {
                file.path()
                    .to_str()
                    .map(|path| format!("--exclude-from={path}"))
                    .ok_or("工作树规则临时路径必须是 UTF-8")
            })
            .transpose()?;
        if let Some(exclude) = exclude.as_deref() {
            args.push(exclude);
        }
        args.extend(["--exclude=AGENTS.override.md", "--"]);
        let (candidates, truncated) = git(repository, &args, LIST_LIMIT)?;
        if truncated {
            return Err("工作树配置匹配的路径列表过长".into());
        }
        let ignored = check_ignored(repository, &candidates)?;
        let mut paths = BTreeSet::new();
        for path in ignored {
            if allowed_relative(&path) && regular_file(repository, &path)?.is_some() {
                paths.insert(path);
            }
        }
        Ok(Self(paths.into_iter().collect()))
    }

    pub(super) fn len(&self) -> usize {
        self.0.len()
    }

    pub(super) fn copy_into(&self, repository: &Path, worktree: &Path) -> Result<(), String> {
        let worktree = worktree.canonicalize().map_err(|error| error.to_string())?;
        // Check the index and ignore rules again before copying, in case the
        // source checkout changed while Git was creating the worktree.
        let candidates = encode_paths(repository, &self.0)?;
        let ignored = check_ignored(repository, &candidates)?;
        for relative in ignored {
            if !allowed_relative(&relative) {
                continue;
            }
            if let Err(error) = copy_file(repository, &worktree, &relative) {
                return Err(format!(
                    "复制工作树配置文件 {} 失败（{error}）",
                    relative.display()
                ));
            }
        }
        Ok(())
    }
}

fn manifest_snapshot(repository: &Path) -> Result<Option<tempfile::NamedTempFile>, String> {
    let path = repository.join(MANIFEST);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => return Err(".worktreeinclude 需要是仓库根目录下的普通文件".into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("无法读取 .worktreeinclude（{error}）")),
    };
    if metadata.len() > MANIFEST_LIMIT as u64 {
        return Err(".worktreeinclude 超出读取上限".into());
    }
    let file = File::open(path).map_err(|error| format!("无法读取 .worktreeinclude（{error}）"))?;
    let (contents, truncated) =
        read_limited(file, MANIFEST_LIMIT).map_err(|error| error.to_string())?;
    if truncated {
        return Err(".worktreeinclude 超出读取上限".into());
    }
    let mut snapshot = tempfile::NamedTempFile::new().map_err(|error| error.to_string())?;
    snapshot
        .write_all(&contents)
        .map_err(|error| error.to_string())?;
    Ok(Some(snapshot))
}

fn check_ignored(repository: &Path, candidates: &[u8]) -> Result<Vec<PathBuf>, String> {
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let mut input = tempfile::tempfile().map_err(|error| error.to_string())?;
    // check-ignore rejects Git's literal-pathspec flag. A leading "./" lets
    // it read names such as ":(glob)file" without treating them as magic.
    for path in candidates
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        input
            .write_all(b"./")
            .and_then(|()| input.write_all(path))
            .and_then(|()| input.write_all(&[0]))
            .map_err(|error| error.to_string())?;
    }
    input.rewind().map_err(|error| error.to_string())?;
    let mut child = git_command(repository)
        .args(["--no-literal-pathspecs", "check-ignore", "-z", "--stdin"])
        .env_remove("GIT_GLOB_PATHSPECS")
        .env_remove("GIT_NOGLOB_PATHSPECS")
        .env_remove("GIT_ICASE_PATHSPECS")
        .stdin(input)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| error.to_string())?;
    let output = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("Git stdout unavailable"))
        .and_then(|stdout| read_limited(stdout, LIST_LIMIT));
    let status = child.wait().map_err(|error| error.to_string())?;
    let (bytes, truncated) = output.map_err(|error| error.to_string())?;
    if truncated {
        return Err("匹配的忽略文件列表过长".into());
    }
    if status.success() || status.code() == Some(1) {
        bytes
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .map(|path| decode_path(path.strip_prefix(b"./").unwrap_or(path)))
            .collect()
    } else {
        Err(format!("无法核对 Git 忽略规则（{status}）"))
    }
}

#[cfg(unix)]
fn decode_path(bytes: &[u8]) -> Result<PathBuf, String> {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    Ok(PathBuf::from(OsStr::from_bytes(bytes)))
}

#[cfg(not(unix))]
fn decode_path(bytes: &[u8]) -> Result<PathBuf, String> {
    std::str::from_utf8(bytes)
        .map(PathBuf::from)
        .map_err(|_| "Git 路径必须是 UTF-8".into())
}

fn encode_paths(repository: &Path, paths: &[PathBuf]) -> Result<Vec<u8>, String> {
    let mut paths_bytes = Vec::new();
    for path in paths {
        if regular_file(repository, path)?.is_none() {
            continue;
        }
        #[cfg(unix)]
        let bytes = path.as_os_str().as_encoded_bytes();
        #[cfg(not(unix))]
        let bytes = path.to_str().ok_or("Git 路径必须是 UTF-8")?.as_bytes();
        paths_bytes.extend_from_slice(bytes);
        paths_bytes.push(0);
    }
    if paths_bytes.len() > LIST_LIMIT {
        return Err("匹配的忽略文件列表过长".into());
    }
    Ok(paths_bytes)
}

fn metadata(path: &Path) -> io::Result<Option<Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn regular_file(root: &Path, relative: &Path) -> Result<Option<Metadata>, String> {
    let mut path = root.to_owned();
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        path.push(component.as_os_str());
        let Some(metadata) = metadata(&path).map_err(|error| error.to_string())? else {
            return Ok(None);
        };
        if components.peek().is_none() {
            return Ok(metadata.is_file().then_some(metadata));
        }
        if !metadata.is_dir() {
            return Ok(None);
        }
    }
    Ok(None)
}

fn destination_parent(worktree: &Path, relative: &Path) -> io::Result<Option<PathBuf>> {
    let mut parent = worktree.to_owned();
    let Some(parts) = relative.parent() else {
        return Ok(None);
    };
    for part in parts.components() {
        parent.push(part.as_os_str());
        if metadata(&parent)?.is_none() {
            match fs::create_dir(&parent) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        if !metadata(&parent)?.is_some_and(|attributes| attributes.is_dir()) {
            return Ok(None);
        }
    }
    let canonical = parent.canonicalize()?;
    Ok(canonical.starts_with(worktree).then_some(canonical))
}

fn copy_file(repository: &Path, worktree: &Path, relative: &Path) -> Result<(), String> {
    let Some(source_metadata) = regular_file(repository, relative)? else {
        return Ok(());
    };
    let source = repository
        .join(relative)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !source.starts_with(repository) {
        return Ok(());
    }
    let Some(parent) = destination_parent(worktree, relative).map_err(|error| error.to_string())?
    else {
        return Ok(());
    };
    let destination = parent.join(relative.file_name().ok_or("无效的配置文件名")?);
    if metadata(&destination)
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Ok(());
    }
    let mut source = File::open(source).map_err(|error| error.to_string())?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    io::copy(&mut source, &mut temporary).map_err(|error| error.to_string())?;
    let permissions = source_metadata.permissions();
    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        fs::Permissions::from_mode(permissions.mode() & 0o777)
    };
    temporary
        .as_file()
        .set_permissions(permissions)
        .map_err(|error| error.to_string())?;
    match temporary.persist_noclobber(destination) {
        Ok(_) => Ok(()),
        Err(error) if error.error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init(root: &Path, source: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let exclusions = root.join("empty-global-exclusions");
        fs::write(&exclusions, "")?;
        git(source, &["init", "-q"], 4096)?;
        git(
            source,
            &[
                "config",
                "--local",
                "core.excludesFile",
                exclusions.to_str().unwrap(),
            ],
            4096,
        )?;
        Ok(())
    }

    #[test]
    fn copies_only_ignored_matches_and_ignored_overrides_without_overwriting()
    -> Result<(), Box<dyn std::error::Error>> {
        let scratch = tempfile::tempdir()?;
        let root = scratch.path().canonicalize()?;
        let source = root.join("source");
        let target = root.join("target");
        fs::create_dir_all(source.join("config"))?;
        fs::create_dir(&target)?;
        init(&root, &source)?;
        fs::write(
            source.join(".gitignore"),
            ".env.local\nconfig/\nAGENTS.override.md\ntracked.txt\n",
        )?;
        fs::write(
            source.join(MANIFEST),
            "# Local setup\n/.env.local\n/config/*.json\n!/config/private.json\nordinary.txt\ntracked.txt\n",
        )?;
        for (path, contents) in [
            (".env.local", "local fixture"),
            ("ordinary.txt", "ordinary file"),
            ("tracked.txt", "tracked even though it matches"),
            ("config/setup.json", "fixture setup"),
            ("config/private.json", "excluded by negation"),
            ("config/AGENTS.override.md", "local instructions"),
        ] {
            fs::write(source.join(path), contents)?;
        }
        git(&source, &["add", "-f", "--", "tracked.txt"], 4096)?;
        fs::write(target.join(".env.local"), "existing checkout content")?;

        let files = IncludedFiles::read(&source)?;
        assert_eq!(
            files.0,
            [
                PathBuf::from(".env.local"),
                PathBuf::from("config/AGENTS.override.md"),
                PathBuf::from("config/setup.json")
            ]
        );
        files.copy_into(&source, &target)?;
        assert_eq!(
            fs::read_to_string(target.join(".env.local"))?,
            "existing checkout content"
        );
        assert_eq!(
            fs::read_to_string(target.join("config/setup.json"))?,
            "fixture setup"
        );
        assert_eq!(
            fs::read_to_string(target.join("config/AGENTS.override.md"))?,
            "local instructions"
        );
        assert!(!target.join("config/private.json").exists());
        assert!(!target.join("ordinary.txt").exists());
        assert!(!target.join("tracked.txt").exists());
        Ok(())
    }

    #[test]
    fn rechecks_the_source_index_and_ignore_state_before_copying()
    -> Result<(), Box<dyn std::error::Error>> {
        let scratch = tempfile::tempdir()?;
        let root = scratch.path().canonicalize()?;
        let source = root.join("source");
        let target = root.join("target");
        fs::create_dir(&source)?;
        fs::create_dir(&target)?;
        init(&root, &source)?;
        fs::write(
            source.join(".gitignore"),
            "tracked.local\nnot-ignored.local\n",
        )?;
        fs::write(source.join(MANIFEST), "*.local\n")?;
        fs::write(source.join("tracked.local"), "fixture")?;
        fs::write(source.join("not-ignored.local"), "fixture")?;
        let files = IncludedFiles::read(&source)?;
        assert_eq!(files.len(), 2);
        git(&source, &["add", "-f", "--", "tracked.local"], 4096)?;
        fs::write(source.join(".gitignore"), "tracked.local\n")?;
        files.copy_into(&source, &target)?;
        assert_eq!(fs::read_dir(target)?.count(), 0);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn skips_source_symlinks_and_target_symlink_directories_and_keeps_nul_paths()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let scratch = tempfile::tempdir()?;
        let root = scratch.path().canonicalize()?;
        let source = root.join("source");
        let target = root.join("target");
        let outside = root.join("outside");
        fs::create_dir_all(source.join("config"))?;
        fs::create_dir(&target)?;
        fs::create_dir(&outside)?;
        init(&root, &source)?;
        fs::write(source.join(".gitignore"), "*.setup\nconfig/\n")?;
        fs::write(source.join(MANIFEST), "*.setup\n/config/**\n")?;
        let unusual = "界 面\nfixture.setup";
        fs::write(source.join(unusual), [0, 3, 255])?;
        fs::write(source.join(":(glob)fixture.setup"), "literal filename")?;
        fs::set_permissions(source.join(unusual), fs::Permissions::from_mode(0o700))?;
        fs::write(outside.join("private.setup"), "outside")?;
        symlink(outside.join("private.setup"), source.join("link.setup"))?;
        fs::write(source.join("config/inside.setup"), "inside")?;
        symlink(&outside, target.join("config"))?;
        let files = IncludedFiles::read(&source)?;
        assert_eq!(files.len(), 3);
        files.copy_into(&source, &target)?;
        assert_eq!(fs::read(target.join(unusual))?, [0, 3, 255]);
        assert_eq!(
            fs::read_to_string(target.join(":(glob)fixture.setup"))?,
            "literal filename"
        );
        assert_eq!(
            fs::metadata(target.join(unusual))?.permissions().mode() & 0o777,
            0o700
        );
        assert!(fs::symlink_metadata(target.join("link.setup")).is_err());
        assert!(!outside.join("inside.setup").exists());
        Ok(())
    }
}
