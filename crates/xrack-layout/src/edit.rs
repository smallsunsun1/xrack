use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug, Default)]
pub struct TextEdit {
    caret: usize,
    anchor: usize,
    trailing: bool,
    preferred_x: Option<f32>,
    pub preedit: String,
    pub preedit_cursor: Option<usize>,
}

/// A logical text offset plus its visual side at a soft wrap boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextPosition {
    pub offset: usize,
    pub trailing: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum EditAction<'a> {
    Insert(&'a str),
    ReplaceAll(&'a str),
    Backspace,
    Delete,
    Left(bool),
    Right(bool),
    Home(bool),
    End(bool),
    SelectAll,
}

pub(crate) struct AppliedEdit {
    pub text: String,
    pub start: usize,
    pub removed: String,
    pub inserted: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SelectionState {
    caret: TextPosition,
    anchor: usize,
}

impl SelectionState {
    pub(crate) fn collapsed(self) -> bool {
        self.caret.offset == self.anchor
    }
}

fn boundary(text: &str, position: usize) -> usize {
    if position >= text.len() {
        return text.len();
    }
    text.grapheme_indices(true)
        .take_while(|(index, _)| *index <= position)
        .last()
        .map_or(0, |(index, _)| index)
}

impl TextEdit {
    pub fn at_end(text: &str) -> Self {
        Self {
            caret: text.len(),
            anchor: text.len(),
            ..Self::default()
        }
    }

    pub fn caret(&self, text: &str) -> usize {
        boundary(text, self.caret)
    }

    pub fn anchor(&self, text: &str) -> usize {
        boundary(text, self.anchor)
    }

    pub fn position(&self, text: &str) -> TextPosition {
        TextPosition {
            offset: self.caret(text),
            trailing: self.trailing,
        }
    }

    pub fn place_caret(&mut self, text: &str, position: TextPosition, selecting: bool) {
        self.set_preedit(String::new(), None);
        self.caret = boundary(text, position.offset);
        self.trailing = position.trailing;
        self.preferred_x = None;
        if !selecting {
            self.anchor = self.caret;
        }
    }

    pub fn set_selection(&mut self, text: &str, anchor: usize, focus: TextPosition) {
        self.place_caret(text, focus, false);
        self.anchor = boundary(text, anchor);
    }

    pub(crate) fn preferred_x(&self) -> Option<f32> {
        self.preferred_x
    }

    pub(crate) fn set_preferred_x(&mut self, x: f32) {
        self.preferred_x = Some(x);
    }

    pub fn selection(&self, text: &str) -> Range<usize> {
        let caret = self.caret(text);
        let anchor = self.anchor(text);
        caret.min(anchor)..caret.max(anchor)
    }

    pub(crate) fn selection_state(&self, text: &str) -> SelectionState {
        SelectionState {
            caret: self.position(text),
            anchor: self.anchor(text),
        }
    }

    pub(crate) fn restore_selection(&mut self, text: &str, state: SelectionState) {
        self.set_selection(text, state.anchor, state.caret);
    }

    pub fn set_preedit(&mut self, value: String, cursor: Option<(usize, usize)>) {
        self.preedit_cursor = cursor.map(|(start, _)| boundary(&value, start));
        self.preedit = value;
    }

    /// Return a replacement value for actions that change the committed text.
    pub fn apply(&mut self, text: &str, action: EditAction<'_>) -> Option<String> {
        self.apply_mode(text, action, false).map(|edit| edit.text)
    }

    pub fn apply_multiline(&mut self, text: &str, action: EditAction<'_>) -> Option<String> {
        self.apply_mode(text, action, true).map(|edit| edit.text)
    }

    pub(crate) fn apply_mode(
        &mut self,
        text: &str,
        action: EditAction<'_>,
        multiline: bool,
    ) -> Option<AppliedEdit> {
        self.preedit.clear();
        self.preedit_cursor = None;
        self.preferred_x = None;
        self.trailing = matches!(action, EditAction::Right(_) | EditAction::End(_));
        let caret = self.caret(text);
        let selection = self.selection(text);
        let left = || {
            text[..caret]
                .grapheme_indices(true)
                .next_back()
                .map_or(0, |(index, _)| index)
        };
        let right = || {
            text[caret..]
                .graphemes(true)
                .next()
                .map_or(caret, |g| caret + g.len())
        };
        let mut move_caret = |next: usize, selecting: bool| {
            self.caret = next;
            if !selecting {
                self.anchor = next;
            }
        };
        let (range, replacement) = match action {
            EditAction::Left(selecting) => {
                move_caret(
                    if !selecting && !selection.is_empty() {
                        selection.start
                    } else {
                        left()
                    },
                    selecting,
                );
                return None;
            }
            EditAction::Right(selecting) => {
                move_caret(
                    if !selecting && !selection.is_empty() {
                        selection.end
                    } else {
                        right()
                    },
                    selecting,
                );
                return None;
            }
            EditAction::Home(selecting) => {
                move_caret(0, selecting);
                return None;
            }
            EditAction::End(selecting) => {
                move_caret(text.len(), selecting);
                return None;
            }
            EditAction::SelectAll => {
                self.anchor = 0;
                self.caret = text.len();
                return None;
            }
            EditAction::Backspace if selection.is_empty() => (left()..caret, String::new()),
            EditAction::Delete if selection.is_empty() => (caret..right(), String::new()),
            EditAction::Backspace | EditAction::Delete => (selection, String::new()),
            EditAction::Insert(value) | EditAction::ReplaceAll(value) => {
                let normalized;
                let value = if multiline {
                    normalized = value.replace("\r\n", "\n").replace('\r', "\n");
                    normalized.as_str()
                } else {
                    value
                };
                let filtered = value
                    .chars()
                    .filter(|ch| !ch.is_control() || (multiline && *ch == '\n'))
                    .collect::<String>();
                if matches!(action, EditAction::ReplaceAll(_)) {
                    (0..text.len(), filtered)
                } else {
                    if filtered.is_empty() {
                        return None;
                    }
                    (selection, filtered)
                }
            }
        };
        if range.is_empty() && replacement.is_empty() {
            return None;
        }
        let next = range.start + replacement.len();
        let mut result = text.to_owned();
        result.replace_range(range.clone(), &replacement);
        self.caret = if next == result.len() {
            next
        } else {
            result
                .grapheme_indices(true)
                .map(|(index, _)| index)
                .find(|&index| index >= next)
                .unwrap_or(result.len())
        };
        self.anchor = self.caret;
        Some(AppliedEdit {
            text: result,
            start: range.start,
            removed: text[range].to_owned(),
            inserted: replacement,
        })
    }
}
