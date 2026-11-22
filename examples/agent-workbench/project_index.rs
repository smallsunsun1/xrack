use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const PROJECT_LIMIT: usize = 32;
const INDEX_LIMIT: u64 = 256 * 1024;

#[derive(Default)]
pub(crate) struct ProjectIndex(Option<PathBuf>);

impl ProjectIndex {
    pub fn new(directory: Option<PathBuf>) -> Self {
        Self(directory)
    }

    pub fn load(&self) -> io::Result<Vec<String>> {
        let Some(directory) = &self.0 else {
            return Ok(Vec::new());
        };
        let file = match File::open(directory.join("projects.json")) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut contents = Vec::new();
        file.take(INDEX_LIMIT + 1).read_to_end(&mut contents)?;
        if contents.len() as u64 > INDEX_LIMIT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Project index exceeds size limit",
            ));
        }
        let paths: Vec<String> = serde_json::from_slice(&contents)?;
        let mut projects = Vec::new();
        for path in paths {
            if Path::new(&path).is_absolute() && !projects.contains(&path) {
                projects.push(path);
                if projects.len() == PROJECT_LIMIT {
                    break;
                }
            }
        }
        Ok(projects)
    }

    pub fn remember(&self, path: &str, projects: &mut Vec<String>) -> io::Result<()> {
        if path.is_empty() {
            return Ok(());
        }
        visit(projects, path);
        let Some(directory) = &self.0 else {
            return Ok(());
        };
        fs::create_dir_all(directory)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(directory.join("projects.lock"))?;
        lock.lock()?;
        let mut saved = self.load()?;
        visit(&mut saved, path);
        if saved != *projects {
            *projects = saved;
        }
        let bytes = serde_json::to_vec(projects)?;
        if bytes.len() as u64 > INDEX_LIMIT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Project index exceeds size limit",
            ));
        }
        static NEXT_FILE: AtomicU64 = AtomicU64::new(0);
        let number = NEXT_FILE.fetch_add(1, Ordering::Relaxed);
        let temporary = directory.join(format!(".projects.{}.{number}.tmp", std::process::id()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let written = file.write_all(&bytes);
        drop(file);
        if let Err(error) =
            written.and_then(|()| fs::rename(&temporary, directory.join("projects.json")))
        {
            let _ = fs::remove_file(temporary);
            return Err(error);
        }
        Ok(())
    }
}

fn visit(projects: &mut Vec<String>, path: &str) {
    projects.retain(|project| project != path);
    projects.insert(0, path.to_owned());
    projects.truncate(PROJECT_LIMIT);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visits_merge_on_disk_and_keep_the_latest_project_first() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        let first = directory
            .path()
            .join("proj-a")
            .to_string_lossy()
            .into_owned();
        let second = directory
            .path()
            .join("proj-b")
            .to_string_lossy()
            .into_owned();
        let index = ProjectIndex::new(Some(directory.path().join("state")));
        let mut window_a = index.load()?;
        let mut window_b = index.load()?;
        index.remember(&first, &mut window_a)?;
        index.remember(&second, &mut window_b)?;
        assert_eq!(window_b, [second.clone(), first.clone()]);
        index.remember(&first, &mut window_a)?;
        assert_eq!(index.load()?, [first, second]);
        Ok(())
    }
}
