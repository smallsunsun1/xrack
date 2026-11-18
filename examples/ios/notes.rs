use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

pub fn read_notes(paths: &[String]) -> Option<String> {
    let mut notes = String::new();
    for (index, path) in paths.iter().enumerate() {
        append_note(Path::new(path), index, &mut notes).ok()?;
    }
    Some(notes)
}

pub fn append_note(path: &Path, index: usize, notes: &mut String) -> io::Result<()> {
    const MAX_NOTE_BYTES: usize = 64 * 1024;
    let separator = if index == 0 { "" } else { "\n\n" };
    let too_large = || io::Error::new(io::ErrorKind::InvalidData, "Notes exceed 64 KiB");
    let remaining = MAX_NOTE_BYTES
        .checked_sub(notes.len() + separator.len())
        .ok_or_else(too_large)?;
    notes.push_str(separator);
    File::open(path)?
        .take(remaining as u64 + 1)
        .read_to_string(notes)?;
    if notes.len() > MAX_NOTE_BYTES {
        return Err(too_large());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::read_notes;

    #[test]
    fn combines_utf8_notes_in_selection_order() -> std::io::Result<()> {
        let directory = tempfile::tempdir()?;
        let first = directory.path().join("round.txt");
        let second = directory.path().join("pressure.md");
        std::fs::write(&first, "巡检通过 🚰")?;
        std::fs::write(&second, "Second note")?;
        let paths = [first, second].map(|path| path.to_string_lossy().into_owned());
        assert_eq!(
            read_notes(&paths).as_deref(),
            Some("巡检通过 🚰\n\nSecond note")
        );
        Ok(())
    }

    #[test]
    fn rejects_invalid_missing_or_over_budget_imports() -> std::io::Result<()> {
        let directory = tempfile::tempdir()?;
        let note = directory.path().join("round.txt");
        let path = note.to_string_lossy().into_owned();
        assert_eq!(read_notes(std::slice::from_ref(&path)), None);
        std::fs::write(&note, [0xff])?;
        assert_eq!(read_notes(std::slice::from_ref(&path)), None);
        std::fs::write(&note, "x".repeat(64 * 1024))?;
        assert_eq!(
            read_notes(std::slice::from_ref(&path)).map(|notes| notes.len()),
            Some(64 * 1024)
        );
        assert_eq!(read_notes(&[path.clone(), path.clone()]), None);
        std::fs::write(&note, "x".repeat(64 * 1024 + 1))?;
        assert_eq!(read_notes(&[path]), None);
        Ok(())
    }
}
