use crate::drafts::{DraftKey, create_private_directory};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const FILE_NAME: &str = "conversation-tabs.json";
const LIMIT: u64 = 512 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct SavedTab {
    pub key: DraftKey,
    pub workspace: String,
    pub project: String,
    pub archived: bool,
}

impl SavedTab {
    fn valid(&self) -> bool {
        self.key.valid()
            && Path::new(&self.workspace).is_absolute()
            && Path::new(&self.project).is_absolute()
    }
}

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct SavedTabs {
    pub open: Vec<SavedTab>,
    pub active: Option<DraftKey>,
}

#[derive(Default)]
pub(crate) struct TabStore(Option<PathBuf>);

impl TabStore {
    pub fn new(directory: Option<PathBuf>) -> Self {
        Self(directory)
    }

    pub fn load(&self) -> io::Result<SavedTabs> {
        let Some(directory) = &self.0 else {
            return Ok(SavedTabs::default());
        };
        let file = match File::open(directory.join(FILE_NAME)) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(SavedTabs::default());
            }
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        file.take(LIMIT + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > LIMIT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "会话标签存档过大",
            ));
        }
        let mut saved: SavedTabs = serde_json::from_slice(&bytes)?;
        let mut keys = Vec::new();
        saved.open.retain(|tab| {
            if !tab.valid() || keys.contains(&tab.key) {
                return false;
            }
            keys.push(tab.key.clone());
            true
        });
        if saved.active.as_ref().is_some_and(|key| !keys.contains(key)) {
            saved.active = None;
        }
        Ok(saved)
    }

    pub fn save(&self, tabs: &SavedTabs) -> io::Result<()> {
        let Some(directory) = &self.0 else {
            return Ok(());
        };
        create_private_directory(directory)?;
        let _lock = lock(directory)?;
        write_tabs(directory, tabs)
    }

    pub fn rekey(&self, old: &DraftKey, new: &DraftKey) -> io::Result<()> {
        let Some(directory) = self.0.as_ref().filter(|directory| directory.is_dir()) else {
            return Ok(());
        };
        let _lock = lock(directory)?;
        let mut saved = self.load()?;
        if let Some(index) = saved.open.iter().position(|tab| &tab.key == old) {
            if saved.open.iter().any(|tab| &tab.key == new) {
                saved.open.remove(index);
            } else {
                saved.open[index].key = new.clone();
            }
            if saved.active.as_ref() == Some(old) {
                saved.active = Some(new.clone());
            }
            write_tabs(directory, &saved)?;
        }
        Ok(())
    }

    pub fn relocate(&self, key: &DraftKey, workspace: &str) -> io::Result<()> {
        let Some(directory) = self.0.as_ref().filter(|directory| directory.is_dir()) else {
            return Ok(());
        };
        let _lock = lock(directory)?;
        let mut saved = self.load()?;
        if let Some(tab) = saved.open.iter_mut().find(|tab| &tab.key == key) {
            tab.workspace = workspace.into();
            write_tabs(directory, &saved)?;
        }
        Ok(())
    }
}

fn lock(directory: &Path) -> io::Result<File> {
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(directory.join("conversation-tabs.lock"))?;
    lock.lock()?;
    Ok(lock)
}

fn write_tabs(directory: &Path, tabs: &SavedTabs) -> io::Result<()> {
    let bytes = serde_json::to_vec(tabs)?;
    if bytes.len() as u64 > LIMIT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "会话标签存档过大",
        ));
    }
    let mut staged = tempfile::NamedTempFile::new_in(directory)?;
    staged.write_all(&bytes)?;
    staged.persist(directory.join(FILE_NAME))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_preserves_other_tabs_and_persists_the_selected_native_identity() -> io::Result<()>
    {
        let temp = tempfile::tempdir()?;
        let path = temp.path().canonicalize()?.to_string_lossy().into_owned();
        let store = TabStore::new(Some(temp.path().to_owned()));
        let local = DraftKey::local();
        let new = DraftKey::Thread("native".into());
        let other = SavedTab {
            key: DraftKey::Thread("other".into()),
            workspace: path.clone(),
            project: path.clone(),
            archived: false,
        };
        let mut selected = SavedTab {
            key: local.clone(),
            ..other.clone()
        };
        store.save(&SavedTabs {
            open: vec![other.clone(), selected.clone()],
            active: Some(local.clone()),
        })?;
        store.rekey(&local, &new)?;
        selected.key = new.clone();
        let restored = store.load()?;
        assert_eq!(restored.open, [other, selected]);
        assert_eq!(restored.active, Some(new));
        Ok(())
    }
}
