use crate::prompt::Attachment;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const LIMIT: u64 = 8 * 1024 * 1024;
const FILE_NAME: &str = "composer-drafts.json";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DraftKey {
    Local(String),
    Thread(String),
}

impl Default for DraftKey {
    fn default() -> Self {
        Self::local()
    }
}

impl DraftKey {
    pub(crate) fn local() -> Self {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        Self::Local(format!("{}-{time}-{serial}", std::process::id()))
    }

    pub(crate) fn valid(&self) -> bool {
        let (Self::Local(id) | Self::Thread(id)) = self;
        !id.is_empty()
            && id.len() <= 256
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct DraftContent {
    pub text: String,
    pub attachments: Vec<Attachment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review_notes: Vec<SavedReviewNote>,
}

impl DraftContent {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.attachments.is_empty() && self.review_notes.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SavedReviewNote {
    pub id: i64,
    pub reference: String,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SavedDraft {
    pub key: DraftKey,
    pub workspace: String,
    pub project: String,
    pub content: DraftContent,
}

impl SavedDraft {
    fn valid(&self) -> bool {
        self.key.valid()
            && Path::new(&self.workspace).is_absolute()
            && Path::new(&self.project).is_absolute()
    }
}

#[derive(Clone, Default)]
pub struct DraftStore(Option<PathBuf>);

impl DraftStore {
    pub fn new(directory: Option<PathBuf>) -> Self {
        Self(directory.map(|directory| directory.join("drafts")))
    }

    pub fn enabled(&self) -> bool {
        self.0.is_some()
    }

    pub fn records(&self) -> io::Result<Vec<SavedDraft>> {
        let Some(directory) = &self.0 else {
            return Ok(Vec::new());
        };
        let file = match File::open(directory.join(FILE_NAME)) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        file.take(LIMIT + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > LIMIT {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "草稿存档过大"));
        }
        let records: Vec<SavedDraft> = serde_json::from_slice(&bytes)?;
        if records.iter().any(|record| !record.valid()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "草稿存档包含无效的会话",
            ));
        }
        Ok(records)
    }

    pub fn save(&self, draft: SavedDraft) -> io::Result<()> {
        if !draft.valid() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "草稿会话无效"));
        }
        self.edit(|records| {
            records.retain(|record| record.key != draft.key);
            if !draft.content.is_empty() {
                records.push(draft);
            }
        })
    }

    pub(crate) fn rekey(&self, old: &DraftKey, new: &DraftKey) -> io::Result<()> {
        let Some(directory) = &self.0 else {
            return Ok(());
        };
        if !directory.join(FILE_NAME).try_exists()? {
            return Ok(());
        }
        if !new.valid() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "草稿会话无效"));
        }
        self.edit(|records| {
            if let Some(index) = records.iter().position(|record| &record.key == old) {
                let mut record = records.remove(index);
                if !records.iter().any(|record| &record.key == new) {
                    record.key = new.clone();
                    records.push(record);
                }
            }
        })
    }

    pub(crate) fn relocate(&self, key: &DraftKey, workspace: &str) -> io::Result<()> {
        let Some(directory) = &self.0 else {
            return Ok(());
        };
        if !directory.join(FILE_NAME).try_exists()? {
            return Ok(());
        }
        self.edit(|records| {
            if let Some(record) = records.iter_mut().find(|record| &record.key == key) {
                record.workspace = workspace.into();
            }
        })
    }

    fn edit(&self, update: impl FnOnce(&mut Vec<SavedDraft>)) -> io::Result<()> {
        let Some(directory) = &self.0 else {
            return Ok(());
        };
        create_private_directory(directory)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(directory.join("drafts.lock"))?;
        lock.lock()?;
        let mut records = self.records()?;
        update(&mut records);
        let contents = serde_json::to_vec(&records)?;
        if contents.len() as u64 > LIMIT {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "草稿存档过大"));
        }
        let mut staged = tempfile::NamedTempFile::new_in(directory)?;
        staged.write_all(&contents)?;
        staged.persist(directory.join(FILE_NAME))?;
        Ok(())
    }
}

pub(crate) fn create_private_directory(directory: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(directory)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_remain_atomic_and_do_not_replace_other_sessions() -> io::Result<()> {
        let directory = tempfile::tempdir()?;
        let profile = DraftStore::new(Some(directory.path().to_owned()));
        let workspace = directory
            .path()
            .canonicalize()?
            .to_string_lossy()
            .into_owned();
        let slot = DraftKey::local();
        let other = DraftKey::local();
        let record = |key, text: &str| SavedDraft {
            key,
            workspace: workspace.clone(),
            project: workspace.clone(),
            content: DraftContent {
                text: text.into(),
                attachments: Vec::new(),
                ..DraftContent::default()
            },
        };
        profile.save(record(slot.clone(), "one"))?;
        profile.save(record(other.clone(), "two"))?;
        let thread = DraftKey::Thread("saved-thread".into());
        profile.rekey(&slot, &thread)?;
        assert_eq!(
            profile.records()?,
            [record(other.clone(), "two"), record(thread.clone(), "one")]
        );
        profile.save(record(thread, ""))?;
        assert_eq!(profile.records()?, [record(other, "two")]);
        Ok(())
    }
}
