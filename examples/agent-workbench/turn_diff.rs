use std::ops::Range;

const DIFF_LIMIT: usize = 1024 * 1024;

#[derive(Clone)]
pub struct DiffFile {
    pub path: String,
    pub status: char,
    range: Range<usize>,
}

#[derive(Clone)]
pub struct TurnDiff {
    pub files: Vec<DiffFile>,
    pub additions: usize,
    pub deletions: usize,
    pub truncated: bool,
    text: String,
}

impl TurnDiff {
    pub fn parse(diff: &str) -> Self {
        let end = diff.floor_char_boundary(DIFF_LIMIT);
        let text = &diff[..end];
        let mut result = Self {
            files: Vec::new(),
            additions: 0,
            deletions: 0,
            truncated: end < diff.len(),
            text: text.to_owned(),
        };
        let mut offset = 0;
        let mut current = None;
        for line in text.split_inclusive('\n') {
            if let Some(header) = line.strip_prefix("diff --git ") {
                if let Some(start) = current.replace(offset) {
                    result.add_file(&text[start..offset], start);
                }
                // A partial header at the preview limit cannot identify a file.
                if result.truncated && !header.ends_with('\n') {
                    current = None;
                }
            }
            offset += line.len();
        }
        if let Some(start) = current {
            result.add_file(&text[start..], start);
        } else if result.files.is_empty() && !text.trim().is_empty() {
            result.add_file(text, 0);
        }
        result
    }

    pub fn patch(&self, file: &DiffFile) -> &str {
        self.text.get(file.range.clone()).unwrap_or_default()
    }

    fn add_file(&mut self, patch: &str, offset: usize) {
        let mut before = None;
        let mut after = None;
        let mut renamed = None;
        let mut status = 'M';
        let mut in_hunk = false;
        for line in patch.lines() {
            if line.starts_with("@@ ") {
                in_hunk = true;
            } else if in_hunk {
                match line.as_bytes().first() {
                    Some(b'+') => self.additions += 1,
                    Some(b'-') => self.deletions += 1,
                    _ => {}
                }
            } else if let Some(path) = line.strip_prefix("--- ") {
                before = Some(path);
            } else if let Some(path) = line.strip_prefix("+++ ") {
                after = Some(path);
            } else if let Some(path) = line.strip_prefix("rename to ") {
                renamed = Some(path);
            } else if line.starts_with("new file mode ") {
                status = 'A';
            } else if line.starts_with("deleted file mode ") {
                status = 'D';
            }
        }
        if renamed.is_some() {
            status = 'R';
        } else if before == Some("/dev/null") {
            status = 'A';
        } else if after == Some("/dev/null") {
            status = 'D';
        }
        // The extended header can be the only path for empty or binary files.
        // Prefer the unified headers so spaces in ordinary Codex paths are kept verbatim.
        let path = renamed
            .or_else(|| after.and_then(|path| marker_path(path, "b/")))
            .or_else(|| before.and_then(|path| marker_path(path, "a/")))
            .or_else(|| patch.lines().next().and_then(header_path));
        self.files.push(DiffFile {
            path: path.map_or_else(|| format!("文件 {}", self.files.len() + 1), str::to_owned),
            status,
            range: offset..offset + patch.len(),
        });
    }
}

fn marker_path<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    (path != "/dev/null").then(|| path.strip_prefix(prefix).unwrap_or(path))
}

fn header_path(line: &str) -> Option<&str> {
    let paths = line.strip_prefix("diff --git a/")?;
    let mut candidates = paths
        .match_indices(" b/")
        .map(|(split, marker)| (&paths[..split], &paths[split + marker.len()..]));
    let (_, fallback) = candidates.clone().next()?;
    Some(
        candidates
            .find(|(old, new)| old == new)
            .map_or(fallback, |(_, new)| new),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separates_files_and_counts_only_hunk_lines() {
        let text = concat!(
            "diff --git a/docs/构建 指南.md b/docs/构建 指南.md\n",
            "--- a/docs/构建 指南.md\n+++ b/docs/构建 指南.md\n",
            "@@ -1,2 +1,2 @@\n heading\n---- old marker\n++++ new marker\n",
            "diff --git a/folder b/empty b/folder b/empty\n",
            "new file mode 100644\nindex 0000000..e69de29\n",
            "diff --git a/obsolete.rs b/obsolete.rs\n",
            "deleted file mode 100644\n--- a/obsolete.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-unused\n",
        );
        let diff = TurnDiff::parse(text);
        assert_eq!((diff.additions, diff.deletions), (1, 2));
        assert_eq!(
            diff.files
                .iter()
                .map(|file| (file.path.as_str(), file.status))
                .collect::<Vec<_>>(),
            [
                ("docs/构建 指南.md", 'M'),
                ("folder b/empty", 'A'),
                ("obsolete.rs", 'D')
            ]
        );
        assert!(diff.patch(&diff.files[0]).ends_with("++++ new marker\n"));
        assert!(diff.patch(&diff.files[1]).contains("new file mode"));
        assert!(!diff.patch(&diff.files[0]).contains("obsolete.rs"));
    }

    #[test]
    fn handles_rename_empty_and_bounded_diff() {
        let rename = "diff --git a/old path b/new path\nsimilarity index 100%\nrename from old path\nrename to new path\n";
        let diff = TurnDiff::parse(rename);
        assert_eq!(
            (diff.files[0].path.as_str(), diff.files[0].status),
            ("new path", 'R')
        );
        assert!(TurnDiff::parse("").files.is_empty());
        let diff = TurnDiff::parse(&format!(
            "diff --git a/large b/large\n--- a/large\n+++ b/large\n@@ -0,0 +1 @@\n+{}\n",
            "中".repeat(DIFF_LIMIT / 2),
        ));
        assert!(diff.truncated);
        assert!(diff.patch(&diff.files[0]).len() <= DIFF_LIMIT);
    }
}
