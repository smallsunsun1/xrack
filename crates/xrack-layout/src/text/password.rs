use crate::TextEdit;
use std::{
    fmt,
    ops::Range,
    time::{Duration, Instant},
};
use unicode_segmentation::UnicodeSegmentation;
use xrack_runtime::Node;

const BULLET: &str = "•";

#[derive(Clone)]
pub(crate) struct PasswordEcho {
    pub(crate) deadline: Instant,
    text: Box<str>,
    range: Range<usize>,
}

impl fmt::Debug for PasswordEcho {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PasswordEcho")
            .field("deadline", &self.deadline)
            .finish_non_exhaustive()
    }
}

impl PasswordEcho {
    pub(crate) fn new(
        node: &Node,
        text: &str,
        typed: &str,
        caret: usize,
        now: Instant,
    ) -> Option<Self> {
        if !node.password_masked() || node.text_read_only() || typed.is_empty() {
            return None;
        }
        let milliseconds = u64::try_from(node.int("password-mask-delay")?).ok()?;
        if milliseconds == 0 {
            return None;
        }
        let start = caret.checked_sub(typed.len())?;
        // A combining key can modify the previous grapheme. Only reveal a whole
        // grapheme whose bytes all came from this event.
        if !text
            .grapheme_indices(true)
            .any(|(offset, grapheme)| offset == start && grapheme == typed)
        {
            return None;
        }
        Some(Self {
            deadline: now.checked_add(Duration::from_millis(milliseconds))?,
            text: text.into(),
            range: start..caret,
        })
    }

    pub(crate) fn matches(&self, node: &Node, text: &str) -> bool {
        node.password_masked()
            && !node.text_read_only()
            && node
                .int("password-mask-delay")
                .is_some_and(|delay| delay > 0)
            && self.text.as_ref() == text
    }

    fn visible_range(&self, node: &Node, text: &str) -> Option<Range<usize>> {
        (Instant::now() < self.deadline && self.matches(node, text)).then(|| self.range.clone())
    }
}

/// Display coordinates and committed UTF-8 offsets for one marker per grapheme.
pub struct PasswordMask {
    source_offsets: Vec<usize>,
    display: String,
    marker_len: usize,
    revealed: Option<(usize, usize)>,
}

impl PasswordMask {
    pub fn new(text: &str) -> Self {
        Self::with_marker(text, BULLET)
    }

    pub fn for_node(node: &Node, text: &str) -> Self {
        let Some(marker) = node.text("password-character").and_then(password_marker) else {
            return Self::new(text);
        };
        Self::with_marker(text, marker)
    }

    #[doc(hidden)]
    pub fn for_editor(node: &Node, text: &str, editor: Option<&TextEdit>) -> Self {
        let mut mask = Self::for_node(node, text);
        if let Some(range) = editor
            .and_then(|editor| editor.password_echo.as_ref())
            .and_then(|echo| echo.visible_range(node, text))
        {
            mask.reveal(text, range);
        }
        mask
    }

    fn reveal(&mut self, text: &str, range: Range<usize>) {
        let index = self
            .source_offsets
            .partition_point(|&offset| offset < range.start);
        let display_start = index * self.marker_len;
        let mut display = self.display.clone();
        display.replace_range(
            display_start..display_start + self.marker_len,
            &text[range.clone()],
        );
        self.revealed = Some((index, range.len()));
        // For example, a revealed regional indicator can join a neighboring
        // flag. Keep the redacted display if any shaping boundary would move.
        if !display
            .grapheme_indices(true)
            .map(|(offset, _)| offset)
            .chain(std::iter::once(display.len()))
            .eq((0..self.source_offsets.len()).map(|index| self.display_boundary(index)))
        {
            self.revealed = None;
            return;
        }
        self.display = display;
    }

    fn with_marker(text: &str, marker: &str) -> Self {
        let source_offsets: Vec<_> = text
            .grapheme_indices(true)
            .map(|(offset, _)| offset)
            .chain(std::iter::once(text.len()))
            .collect();
        let display = marker.repeat(source_offsets.len() - 1);
        Self {
            source_offsets,
            display,
            marker_len: marker.len(),
            revealed: None,
        }
    }

    pub fn display(&self) -> &str {
        &self.display
    }

    pub fn into_display(self) -> String {
        self.display
    }

    pub fn display_offset(&self, source: usize) -> usize {
        let index = self
            .source_offsets
            .partition_point(|&offset| offset <= source);
        self.display_boundary(index.saturating_sub(1))
    }

    fn display_boundary(&self, index: usize) -> usize {
        if let Some((slot, len)) = self.revealed
            && index > slot
        {
            (index - 1) * self.marker_len + len
        } else {
            index * self.marker_len
        }
    }

    pub fn source_offset(&self, display: usize) -> usize {
        let index = if let Some((slot, len)) = self.revealed {
            let start = slot * self.marker_len;
            let end = start + len;
            if display >= end {
                slot + 1 + (display - end) / self.marker_len
            } else {
                (display / self.marker_len).min(slot)
            }
        } else {
            display / self.marker_len
        };
        self.source_offsets[index.min(self.source_offsets.len() - 1)]
    }
}

fn password_marker(value: &str) -> Option<&str> {
    let marker = value.graphemes(true).next()?;
    // Bound the size of a marker repeated for every grapheme of the password.
    if marker.len() > 64 || marker.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    // Two adjacent markers must keep a caret stop between them, even for emoji.
    let pair = marker.repeat(2);
    (pair.graphemes(true).next() == Some(marker)).then_some(marker)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echo_uses_only_new_graphemes_and_remaps_all_offsets() {
        let mut node = Node::new("LineEdit", None);
        node.properties
            .insert("input-type", xrack_runtime::InputType::Password.into());
        node.properties
            .insert("password-character", "*".to_owned().into());
        node.properties
            .insert("password-mask-delay", 1000_i64.into());
        let text = "a👩‍💻b";
        let end = text.len() - 1;
        let now = Instant::now();
        let mut editor = TextEdit::at_end(text);
        editor.password_echo = PasswordEcho::new(&node, text, "👩‍💻", end, now);
        assert_eq!(
            editor.password_echo.as_ref().unwrap().deadline,
            now + Duration::from_secs(1)
        );
        let mask = PasswordMask::for_editor(&node, text, Some(&editor));
        assert_eq!(mask.display(), "*👩‍💻*");
        assert_eq!(mask.source_offset(2), 1);
        assert_eq!(mask.source_offset(end), end);
        assert_eq!(mask.display_offset(end), end);
        assert_eq!(mask.source_offset(mask.display().len()), text.len());
        assert_eq!(PasswordMask::for_node(&node, text).display(), "***");
        assert_eq!(
            PasswordMask::for_editor(&node, "x👩‍💻b", Some(&editor)).display(),
            "***"
        );
        assert!(PasswordEcho::new(&node, "e\u{301}", "\u{301}", 3, now).is_none());
        assert!(PasswordEcho::new(&node, "ab", "ab", 2, now).is_none());
        node.properties.insert("password-mask-delay", 0_i64.into());
        assert_eq!(
            PasswordMask::for_editor(&node, text, Some(&editor)).display(),
            "***"
        );
    }

    #[test]
    fn marker_preserves_utf8_grapheme_boundaries_and_has_a_safe_fallback() {
        let mut node = Node::new("LineEdit", None);
        let source = "e\u{301}👩‍💻";
        for (configured, marker) in [
            ("*unused", "*"),
            ("🔒", "🔒"),
            ("🇨🇳", "🇨🇳"),
            ("", "•"),
            ("\n", "•"),
            ("🇨", "•"),
            ("\u{301}", "•"),
        ] {
            node.properties
                .insert("password-character", configured.to_owned().into());
            let mask = PasswordMask::for_node(&node, source);
            assert_eq!(mask.display(), marker.repeat(2));
            assert_eq!(mask.display_offset(1), 0);
            assert_eq!(mask.display_offset(3), marker.len());
            assert_eq!(mask.source_offset(marker.len()), 3);
            assert_eq!(mask.source_offset(mask.display().len()), source.len());
        }
    }
}
