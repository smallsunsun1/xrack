use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub(crate) struct ThreadIndex(Option<PathBuf>);

pub(crate) struct IndexedThread {
    pub id: String,
    pub workspace: String,
}

pub(crate) struct HandoffLocation {
    pub workspace: String,
    pub worktree: String,
}

impl ThreadIndex {
    pub fn new(directory: Option<PathBuf>) -> Self {
        Self(directory)
    }

    pub fn is_persistent(&self) -> bool {
        self.0.is_some()
    }

    fn record_path(&self, id: &str) -> Option<PathBuf> {
        if id.is_empty()
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return None;
        }
        Some(self.0.as_ref()?.join(format!("{id}.json")))
    }

    pub fn remember(
        &self,
        id: &str,
        workspace: &str,
        archived: bool,
        project: Option<&str>,
    ) -> io::Result<()> {
        let Some(directory) = &self.0 else {
            return Ok(());
        };
        let path = self
            .record_path(id)
            .ok_or_else(|| io::Error::other("Invalid thread ID"))?;
        fs::create_dir_all(directory)?;
        let _lock = lock_index(&path)?;
        let record = read_record(&path);
        write_record(
            &path,
            workspace,
            archived,
            project,
            record
                .as_ref()
                .and_then(|value| value["handoffWorktree"].as_str()),
        )
    }

    pub fn remember_handoff(
        &self,
        id: &str,
        workspace: &str,
        project: &str,
        worktree: &str,
    ) -> io::Result<()> {
        let Some(directory) = &self.0 else {
            return Ok(());
        };
        let path = self
            .record_path(id)
            .ok_or_else(|| io::Error::other("Invalid thread ID"))?;
        fs::create_dir_all(directory)?;
        let _lock = lock_index(&path)?;
        write_record(&path, workspace, false, Some(project), Some(worktree))
    }

    pub fn handoff(&self, id: &str) -> Option<HandoffLocation> {
        let record = read_record(&self.record_path(id)?)?;
        let worktree = record["handoffWorktree"].as_str()?;
        let workspace = record["workspace"].as_str()?;
        if !Path::new(worktree).is_absolute()
            || !Path::new(workspace).is_absolute()
            || !Path::new(workspace).is_dir()
        {
            return None;
        }
        Some(HandoffLocation {
            workspace: workspace.into(),
            worktree: worktree.into(),
        })
    }

    pub fn records(&self, workspace: &str, archived: bool) -> io::Result<Vec<IndexedThread>> {
        let mut records = Vec::new();
        self.visit_records(|id, record| {
            if let Some(thread_workspace) = record["workspace"].as_str()
                && (thread_workspace == workspace || record["project"] == workspace)
                && record["archived"].as_bool() == Some(archived)
            {
                records.push(IndexedThread {
                    id: id.into(),
                    workspace: thread_workspace.into(),
                });
            }
        })?;
        records.sort_unstable_by(|left, right| left.id.cmp(&right.id));
        Ok(records)
    }

    pub(super) fn project_workspaces(&self) -> io::Result<Vec<(String, String)>> {
        let mut workspaces = Vec::new();
        self.visit_records(|_, record| {
            if let (Some(project), Some(workspace)) =
                (record["project"].as_str(), record["workspace"].as_str())
            {
                workspaces.push((project.into(), workspace.into()));
            }
        })?;
        workspaces.sort_unstable();
        workspaces.dedup();
        Ok(workspaces)
    }

    fn visit_records(&self, mut visit: impl FnMut(&str, Value)) -> io::Result<()> {
        let Some(directory) = &self.0 else {
            return Ok(());
        };
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|part| part.to_str()) != Some("json") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|part| part.to_str()) else {
                continue;
            };
            if self.record_path(id).is_none() {
                continue;
            }
            if let Some(record) = read_record(&path) {
                visit(id, record);
            }
        }
        Ok(())
    }

    pub fn archive(&self, id: &str, archived: bool) -> io::Result<()> {
        let Some(path) = self.record_path(id) else {
            return Ok(());
        };
        if !path.try_exists()? {
            return Ok(());
        }
        let _lock = lock_index(&path)?;
        let Some(record) = read_record(&path) else {
            return Ok(());
        };
        if record["archived"].as_bool() != Some(archived)
            && let Some(workspace) = record["workspace"].as_str()
        {
            write_record(
                &path,
                workspace,
                archived,
                record["project"].as_str(),
                record["handoffWorktree"].as_str(),
            )?;
        }
        Ok(())
    }

    pub fn forget(&self, id: &str) {
        if let Some(path) = self.record_path(id)
            && path.exists()
            && let Ok(_lock) = lock_index(&path)
            && read_record(&path).is_none_or(|record| !record["project"].is_string())
        {
            let _ = remove_record(&path);
        }
    }

    pub fn remove(&self, id: &str) -> io::Result<()> {
        let Some(path) = self.record_path(id) else {
            return Ok(());
        };
        if !path.try_exists()? {
            return Ok(());
        }
        let _lock = lock_index(&path)?;
        remove_record(&path)
    }

    pub fn project(&self, id: &str) -> Option<String> {
        read_record(&self.record_path(id)?)?["project"]
            .as_str()
            .map(str::to_owned)
    }
}

fn lock_index(record: &Path) -> io::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(record.with_file_name("index.lock"))?;
    file.lock()?;
    Ok(file)
}

fn write_record(
    path: &Path,
    workspace: &str,
    archived: bool,
    project: Option<&str>,
    handoff_worktree: Option<&str>,
) -> io::Result<()> {
    static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
    let number = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_file_name(format!(".thread.{}.{number}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let written = file.write_all(
        json!({"workspace":workspace,"archived":archived,"project":project,"handoffWorktree":handoff_worktree})
            .to_string()
            .as_bytes(),
    );
    drop(file);
    if let Err(error) = written.and_then(|_| fs::rename(&temporary, path)) {
        let _ = fs::remove_file(temporary);
        return Err(error);
    }
    Ok(())
}

fn remove_record(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

fn read_record(path: &Path) -> Option<Value> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > 16384 {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(16385)
        .read_to_end(&mut bytes)
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    impl ThreadIndex {
        pub(crate) fn ids(&self, workspace: &str, archived: bool) -> io::Result<Vec<String>> {
            Ok(self
                .records(workspace, archived)?
                .into_iter()
                .map(|fork| fork.id)
                .collect())
        }
    }

    #[test]
    fn reopened_index_preserves_workspace_and_archive_scope() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("forks");
        let index = ThreadIndex::new(Some(path.clone()));
        index.remember("pending", "/a", false, None)?;
        index.remember("unrelated", "/b", false, None)?;
        assert!(index.remember("../escape", "/a", false, None).is_err());
        drop(index);

        let index = ThreadIndex::new(Some(path));
        assert_eq!(index.ids("/a", false)?, ["pending"]);
        assert!(index.ids("/a", true)?.is_empty());
        index.archive("pending", true)?;
        assert!(index.ids("/a", false)?.is_empty());
        assert_eq!(index.ids("/a", true)?, ["pending"]);
        index.forget("pending");
        assert!(index.ids("/a", true)?.is_empty());
        assert_eq!(index.ids("/b", false)?, ["unrelated"]);
        Ok(())
    }
}
