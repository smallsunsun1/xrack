use unicode_segmentation::UnicodeSegmentation;

const BULLET: &str = "•";

/// Display coordinates and committed UTF-8 offsets for one bullet per grapheme.
pub struct PasswordMask {
    source_offsets: Vec<usize>,
    display: String,
}

impl PasswordMask {
    pub fn new(text: &str) -> Self {
        let source_offsets: Vec<_> = text
            .grapheme_indices(true)
            .map(|(offset, _)| offset)
            .chain(std::iter::once(text.len()))
            .collect();
        let display = BULLET.repeat(source_offsets.len() - 1);
        Self {
            source_offsets,
            display,
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
        index.saturating_sub(1) * BULLET.len()
    }

    pub fn source_offset(&self, display: usize) -> usize {
        let index = (display / BULLET.len()).min(self.source_offsets.len() - 1);
        self.source_offsets[index]
    }
}
