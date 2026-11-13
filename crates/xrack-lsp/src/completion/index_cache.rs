//! Disk snapshots of the workspace index. Open buffers stay in the live index.
use super::workspace_index::CachedWorkspace;
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{self, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::OnceLock,
};

const SCHEMA: u32 = 1;
const MAX_BYTES: u64 = 128 * 1024 * 1024;

pub(super) struct Cache {
    directory: PathBuf,
    build: String,
}

impl Cache {
    pub(super) fn new(directory: PathBuf) -> Option<Self> {
        // Local Bazel builds can change without a package version bump.
        let build = BUILD
            .get_or_init(|| {
                let executable = std::env::current_exe().ok()?;
                let metadata = fs::metadata(&executable).ok()?;
                Some(format!(
                    "{executable:?}:{:?}:{}",
                    metadata.modified().ok()?,
                    metadata.len()
                ))
            })
            .as_ref()?
            .clone();
        Some(Self { directory, build })
    }

    pub(super) fn load(&self, roots: &BTreeSet<PathBuf>) -> Option<CachedWorkspace> {
        let file = File::open(self.directory.join("index-v1.json")).ok()?;
        if file.metadata().ok()?.len() > MAX_BYTES {
            return None;
        }
        let cache: CachedWorkspace =
            serde_json::from_reader(BufReader::new(file).take(MAX_BYTES)).ok()?;
        (cache.schema == SCHEMA && cache.build == self.build && cache.roots == *roots)
            .then_some(cache)
    }

    pub(super) fn save(&self, mut workspace: CachedWorkspace) -> io::Result<()> {
        workspace.schema = SCHEMA;
        workspace.build.clone_from(&self.build);
        fs::create_dir_all(&self.directory)?;
        let mut temporary = tempfile::NamedTempFile::new_in(&self.directory)?;
        {
            let mut writer = BufWriter::new(temporary.as_file_mut());
            serde_json::to_writer(&mut writer, &workspace).map_err(io::Error::other)?;
            writer.flush()?;
        }
        if temporary.as_file().metadata()?.len() <= MAX_BYTES {
            temporary.persist(self.directory.join("index-v1.json"))?;
        }
        Ok(())
    }
}

static BUILD: OnceLock<Option<String>> = OnceLock::new();

pub(super) fn disk_text(path: &Path) -> Option<String> {
    // Reject special files rather than blocking the indexing worker on them.
    if !fs::metadata(path).ok()?.is_file() {
        return None;
    }
    fs::read_to_string(path).ok()
}
