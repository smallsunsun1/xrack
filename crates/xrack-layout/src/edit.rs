use crate::{Interaction, Target};
use std::{ops::Range, time::Duration};
use unicode_segmentation::UnicodeSegmentation;
use xrack_runtime::{KeyModifiers, Node, TextCommand, spin_editable};

#[derive(Clone, Debug, Default)]
pub struct TextEdit {
    caret: usize,
    anchor: usize,
    trailing: bool,
    preferred_x: Option<f32>,
    composition: Option<(u64, TextEditContext)>,
    selection_drag: Option<SelectionDrag>,
    horizontal_drag_scroll: Option<f32>,
    pub(crate) password_echo: Option<crate::password::PasswordEcho>,
    pub preedit: String,
    pub preedit_cursor: Option<usize>,
}

/// Clamp a drag hit to the visible edge and advance the viewport toward it.
#[doc(hidden)]
pub fn selection_drag_axis(
    pointer: f32,
    start: f32,
    size: f32,
    offset: f32,
    limit: f32,
    elapsed: Duration,
) -> (f32, f32, bool) {
    if size <= 0.0 {
        return (start, offset, false);
    }
    let end = start + size;
    let outside = pointer - pointer.clamp(start, end);
    let speed = (70.0 + outside.abs() * 9.0).min(1600.0) * outside.signum();
    let offset = if outside == 0.0 {
        offset
    } else {
        (offset + speed * elapsed.as_secs_f32()).clamp(0.0, limit)
    };
    let pending = (outside < 0.0 && offset > 0.0) || (outside > 0.0 && offset < limit);
    (
        pointer.clamp(start, end.next_down().max(start)),
        offset,
        pending,
    )
}

#[derive(Clone, Debug)]
struct SelectionDrag {
    unit: SelectionUnit,
    value: String,
    anchor: Range<usize>,
}

#[derive(Clone, Copy, Debug)]
enum SelectionUnit {
    Word,
    Paragraph,
}

impl SelectionUnit {
    fn range(self, text: &str, position: TextPosition, password: bool) -> Range<usize> {
        if password {
            return 0..text.len();
        }
        match self {
            Self::Word => TextEdit::word_at(text, position, false),
            Self::Paragraph => {
                let offset = boundary(text, position.offset);
                let index = if position.trailing {
                    text[..offset]
                        .grapheme_indices(true)
                        .next_back()
                        .map_or(0, |(index, _)| index)
                } else {
                    offset
                };
                let separator = |c| matches!(c, '\n' | '\r' | '\u{2029}');
                let start = text[..index]
                    .char_indices()
                    .rfind(|(_, character)| separator(*character))
                    .map_or(0, |(index, character)| index + character.len_utf8());
                let end = text[index..]
                    .find(separator)
                    .map_or(text.len(), |end| index + end);
                start..end
            }
        }
    }
}

/// A logical offset with affinity to the preceding grapheme at a bidi or wrap boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextPosition {
    pub offset: usize,
    pub trailing: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum EditAction<'a> {
    Insert(&'a str),
    ReplaceAll(&'a str),
    ReplaceRange { start: i64, end: i64, text: &'a str },
    Backspace,
    Delete,
    BackspaceWord,
    DeleteWord,
    Left(bool),
    Right(bool),
    WordLeft(bool),
    WordRight(bool),
    Home(bool),
    End(bool),
    SelectAll,
}

impl EditAction<'_> {
    #[doc(hidden)]
    pub fn word_shortcut(key: &str, modifiers: KeyModifiers, macos: bool) -> Option<Self> {
        let word = if macos {
            modifiers.alt && !modifiers.control
        } else {
            modifiers.control && !modifiers.alt
        };
        if !word || modifiers.meta {
            return None;
        }
        Some(match key {
            "ArrowLeft" => Self::WordLeft(modifiers.shift),
            "ArrowRight" => Self::WordRight(modifiers.shift),
            "Backspace" => Self::BackspaceWord,
            "Delete" => Self::DeleteWord,
            _ => return None,
        })
    }

    pub(crate) fn changes_text(self) -> bool {
        matches!(
            self,
            Self::Insert(_)
                | Self::ReplaceAll(_)
                | Self::ReplaceRange { .. }
                | Self::Backspace
                | Self::Delete
                | Self::BackspaceWord
                | Self::DeleteWord
        )
    }
}

pub(crate) fn word_ranges(text: &str) -> impl DoubleEndedIterator<Item = Range<usize>> + '_ {
    text.split_word_bound_indices()
        .filter(|(_, word)| {
            word.chars()
                .any(|ch| !ch.is_whitespace() && !ch.is_control())
        })
        .map(|(index, word)| index..index + word.len())
}

pub(crate) struct AppliedEdit {
    pub text: String,
    pub start: usize,
    pub removed: String,
    pub inserted: String,
}

impl AppliedEdit {
    pub(crate) fn replace_text(self, original: &str, text: String) -> Self {
        if self.text == text {
            return self;
        }
        let start = original
            .graphemes(true)
            .zip(text.graphemes(true))
            .take_while(|(old, new)| old == new)
            .map(|(grapheme, _)| grapheme.len())
            .sum::<usize>();
        let end = original[start..]
            .graphemes(true)
            .rev()
            .zip(text[start..].graphemes(true).rev())
            .take_while(|(old, new)| old == new)
            .map(|(grapheme, _)| grapheme.len())
            .sum::<usize>();
        Self {
            removed: original[start..original.len() - end].to_owned(),
            inserted: text[start..text.len() - end].to_owned(),
            text,
            start,
        }
    }
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
    pub(crate) fn is_composing(&self) -> bool {
        self.composition.is_some() || !self.preedit.is_empty()
    }

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
        self.selection_drag = None;
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

    #[doc(hidden)]
    pub fn has_committed_suggestion(&self, text: &str) -> bool {
        !self.preedit.is_empty() && text[self.selection(text)] == self.preedit
    }

    pub(crate) fn word_at(text: &str, position: TextPosition, password: bool) -> Range<usize> {
        if password {
            return 0..text.len();
        }
        let offset = boundary(text, position.offset);
        let index = if position.trailing || offset == text.len() {
            text[..offset]
                .grapheme_indices(true)
                .next_back()
                .map_or(0, |(index, _)| index)
        } else {
            offset
        };
        text.split_word_bound_indices()
            .find(|(offset, word)| *offset <= index && index < *offset + word.len())
            .map_or(index..index, |(offset, word)| {
                let end = text
                    .grapheme_indices(true)
                    .find(|(index, _)| *index >= offset + word.len())
                    .map_or(text.len(), |(index, _)| index);
                boundary(text, offset)..end
            })
    }

    #[doc(hidden)]
    pub fn start_word_drag(
        &mut self,
        text: &str,
        position: TextPosition,
        password: bool,
        select_word: bool,
    ) {
        self.start_selection_drag(text, position, password, SelectionUnit::Word, select_word);
    }

    #[doc(hidden)]
    pub fn start_paragraph_drag(&mut self, text: &str, position: TextPosition, password: bool) {
        self.start_selection_drag(text, position, password, SelectionUnit::Paragraph, true);
    }

    fn start_selection_drag(
        &mut self,
        text: &str,
        position: TextPosition,
        password: bool,
        unit: SelectionUnit,
        select: bool,
    ) {
        let anchor = unit.range(text, position, password);
        if select {
            self.set_selection(
                text,
                anchor.start,
                TextPosition {
                    offset: anchor.end,
                    trailing: true,
                },
            );
        } else {
            self.place_caret(text, position, false);
        }
        self.selection_drag = Some(SelectionDrag {
            unit,
            value: text.to_owned(),
            anchor,
        });
    }

    #[doc(hidden)]
    pub fn extend_selection_drag(&mut self, text: &str, position: TextPosition) -> bool {
        let Some(drag) = self.selection_drag.take().filter(|drag| drag.value == text) else {
            return false;
        };
        let range = drag.unit.range(text, position, false);
        let (anchor, caret, trailing) = if range.start < drag.anchor.start {
            (drag.anchor.end, range.start, false)
        } else {
            (drag.anchor.start, drag.anchor.end.max(range.end), true)
        };
        self.set_selection(
            text,
            anchor,
            TextPosition {
                offset: caret,
                trailing,
            },
        );
        self.selection_drag = Some(drag);
        true
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
        self.composition = None;
        self.update_preedit(value, cursor);
    }

    fn update_preedit(&mut self, value: String, cursor: Option<(usize, usize)>) {
        self.password_echo = None;
        self.horizontal_drag_scroll = None;
        self.preedit_cursor = cursor.map(|(start, _)| boundary(&value, start));
        self.preedit = value;
    }

    #[doc(hidden)]
    pub fn horizontal_drag_scroll(&self) -> Option<f32> {
        self.horizontal_drag_scroll
    }

    #[doc(hidden)]
    pub fn set_horizontal_drag_scroll(&mut self, offset: Option<f32>) {
        self.horizontal_drag_scroll = offset;
    }

    /// Return a replacement value for actions that change the committed text.
    pub fn apply(&mut self, text: &str, action: EditAction<'_>) -> Option<String> {
        self.apply_mode(text, action, false, false)
            .map(|edit| edit.text)
    }

    pub fn apply_multiline(&mut self, text: &str, action: EditAction<'_>) -> Option<String> {
        self.apply_mode(text, action, true, false)
            .map(|edit| edit.text)
    }

    pub(crate) fn apply_mode(
        &mut self,
        text: &str,
        action: EditAction<'_>,
        multiline: bool,
        overwrite: bool,
    ) -> Option<AppliedEdit> {
        self.set_preedit(String::new(), None);
        self.selection_drag = None;
        self.preferred_x = None;
        self.trailing = matches!(
            action,
            EditAction::Right(_) | EditAction::WordRight(_) | EditAction::End(_)
        );
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
        let previous_word = || {
            word_ranges(text)
                .rev()
                .find(|word| word.start < caret)
                .map_or(0, |word| boundary(text, word.start))
        };
        let next_word = || {
            word_ranges(text)
                .find(|word| word.end > caret)
                .map_or(text.len(), |word| {
                    text.grapheme_indices(true)
                        .find(|(index, _)| *index >= word.end)
                        .map_or(text.len(), |(index, _)| index)
                })
        };
        let mut move_caret = |next: usize, selecting: bool| {
            self.caret = next;
            if !selecting {
                self.anchor = next;
            }
        };
        let (range, replacement) = match action {
            EditAction::Left(selecting) | EditAction::WordLeft(selecting) => {
                move_caret(
                    if !selecting && !selection.is_empty() {
                        selection.start
                    } else if matches!(action, EditAction::WordLeft(_)) {
                        previous_word()
                    } else {
                        left()
                    },
                    selecting,
                );
                return None;
            }
            EditAction::Right(selecting) | EditAction::WordRight(selecting) => {
                move_caret(
                    if !selecting && !selection.is_empty() {
                        selection.end
                    } else if matches!(action, EditAction::WordRight(_)) {
                        next_word()
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
            EditAction::BackspaceWord if selection.is_empty() => {
                (previous_word()..caret, String::new())
            }
            EditAction::DeleteWord if selection.is_empty() => (caret..next_word(), String::new()),
            EditAction::Backspace
            | EditAction::Delete
            | EditAction::BackspaceWord
            | EditAction::DeleteWord => (selection, String::new()),
            EditAction::Insert(value)
            | EditAction::ReplaceAll(value)
            | EditAction::ReplaceRange { text: value, .. } => {
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
                let explicit = match action {
                    EditAction::ReplaceAll(_) => Some(0..text.len()),
                    EditAction::ReplaceRange { start, end, .. } => {
                        let offset = |byte: i64| {
                            boundary(text, usize::try_from(byte.max(0)).unwrap_or(text.len()))
                        };
                        let (start, end) = (offset(start), offset(end));
                        Some(start.min(end)..start.max(end))
                    }
                    _ => None,
                };
                if filtered.is_empty() && !(explicit.is_some() && value.is_empty()) {
                    return None;
                }
                if let Some(range) = explicit {
                    (range, filtered)
                } else if overwrite && selection.is_empty() {
                    let end = overwrite_end(text, caret, &filtered);
                    (caret..end, filtered)
                } else {
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
        self.trailing = !replacement.is_empty();
        Some(AppliedEdit {
            text: result,
            start: range.start,
            removed: text[range].to_owned(),
            inserted: replacement,
        })
    }
}

fn overwrite_end(text: &str, caret: usize, inserted: &str) -> usize {
    // Segment with the preceding grapheme so a combining mark does not consume the next one.
    let previous = text[..caret]
        .graphemes(true)
        .next_back()
        .unwrap_or_default();
    let combined = [previous, inserted].concat();
    let line_break = |cluster: &str| cluster.contains(['\r', '\n']);
    let count = combined
        .grapheme_indices(true)
        .filter(|(offset, _)| *offset >= previous.len())
        .take_while(|(_, cluster)| !line_break(cluster))
        .count();
    caret
        + text[caret..]
            .graphemes(true)
            .take(count)
            .take_while(|cluster| !line_break(cluster))
            .map(str::len)
            .sum::<usize>()
}

/// The focused editor and selection at the start of an asynchronous edit.
#[derive(Clone, Debug)]
pub struct TextEditContext {
    target: Target,
    text: String,
    selection: Range<usize>,
    caret: TextPosition,
    password: bool,
}

impl TextEditContext {
    #[doc(hidden)]
    pub fn clipboard_allowed(node: &Node, command: TextCommand, selected: bool) -> bool {
        match command {
            TextCommand::Copy => selected && !node.is_password(),
            TextCommand::Cut => selected && !node.is_password() && !node.text_read_only(),
            TextCommand::Paste => !node.text_read_only(),
            _ => false,
        }
    }

    pub fn for_clipboard(input: &Interaction, scene: &Node, command: TextCommand) -> Option<Self> {
        let node = scene.interactive_at_path(&input.focused.as_ref()?.path)?;
        if !Self::clipboard_allowed(node, command, true) {
            return None;
        }
        let context = Self::capture(input, scene)?;
        Self::clipboard_allowed(node, command, !context.selection().is_empty()).then_some(context)
    }

    pub fn capture(input: &Interaction, scene: &Node) -> Option<Self> {
        let editor = input.editor.as_ref()?;
        if editor.is_composing() {
            return None;
        }
        let (target, text, selection, caret) = current(input, scene)?;
        Some(Self {
            target: target.clone(),
            text: text.into_owned(),
            selection,
            caret,
            password: scene.at_path(&target.path)?.is_password(),
        })
    }

    pub fn selection(&self) -> &str {
        &self.text[self.selection.clone()]
    }

    pub fn is_current(&self, input: &Interaction, scene: &Node) -> bool {
        input
            .editor
            .as_ref()
            .is_some_and(|editor| !editor.is_composing())
            && self.matches(input, scene)
    }

    fn matches(&self, input: &Interaction, scene: &Node) -> bool {
        current(input, scene).is_some_and(|(target, text, selection, caret)| {
            self.target == *target
                && scene
                    .at_path(&target.path)
                    .is_some_and(|node| node.is_password() == self.password)
                && self.text == text
                && self.selection == selection
                && self.caret == caret
        })
    }
}

impl Interaction {
    pub fn begin_text_composition(&mut self, scene: &Node, id: u64) -> bool {
        let cancelled = self.cancel_text_composition(None);
        if let Some(focus) = self.list_typeahead_focus(scene).cloned() {
            self.cancel_shortcut_sequence();
            self.list_composition = Some((id, focus));
            return true;
        }
        let Some(context) = TextEditContext::capture(self, scene)
            .filter(|context| !self.menu_open() && context.can_compose(self, scene))
        else {
            return cancelled;
        };
        self.cancel_shortcut_sequence();
        self.dismiss_choice();
        self.break_text_edit_group();
        if let Some(editor) = &mut self.editor {
            editor.password_echo = None;
            editor.composition = Some((id, context));
        }
        true
    }

    pub fn update_text_composition(&mut self, scene: &Node, id: u64, text: String) -> bool {
        let (current_id, valid) = match self.text_composition(scene) {
            Some(composition) => composition,
            None => return false,
        };
        if id != current_id {
            return false;
        }
        if !valid {
            return self.cancel_text_composition(Some(id));
        }
        if let Some(editor) = &mut self.editor {
            let end = text.len();
            editor.update_preedit(text, Some((end, end)));
        }
        true
    }

    pub fn finish_text_composition(&mut self, scene: &Node, id: u64) -> (bool, bool) {
        let commit = self.text_composition(scene) == Some((id, true));
        let ended = self.cancel_text_composition(Some(id));
        (ended, commit)
    }

    pub fn cancel_text_composition(&mut self, id: Option<u64>) -> bool {
        if self
            .list_composition
            .as_ref()
            .is_some_and(|(current_id, _)| id.is_none_or(|id| id == *current_id))
        {
            self.list_composition = None;
            return true;
        }
        if let Some(editor) = &mut self.editor
            && let Some((current_id, _)) = &editor.composition
            && id.is_none_or(|id| id == *current_id)
        {
            editor.set_preedit(String::new(), None);
            return true;
        }
        false
    }

    pub(crate) fn reconcile_text_composition(&mut self, scene: &Node) {
        if let Some((id, false)) = self.text_composition(scene) {
            self.cancel_text_composition(Some(id));
        }
    }

    fn text_composition(&self, scene: &Node) -> Option<(u64, bool)> {
        if let Some((id, focus)) = &self.list_composition {
            return Some((*id, self.list_typeahead_focus(scene) == Some(focus)));
        }
        let (id, context) = self.editor.as_ref()?.composition.as_ref()?;
        Some((*id, !self.menu_open() && context.can_compose(self, scene)))
    }
}

impl TextEditContext {
    fn can_compose(&self, input: &Interaction, scene: &Node) -> bool {
        self.matches(input, scene)
            && scene
                .interactive_at_path(&self.target.path)
                .is_some_and(|node| !node.text_read_only())
    }
}

fn current<'a>(
    input: &'a Interaction,
    scene: &'a Node,
) -> Option<(
    &'a Target,
    std::borrow::Cow<'a, str>,
    Range<usize>,
    TextPosition,
)> {
    let target = input.focused.as_ref()?;
    let node = scene
        .interactive_at_path(&target.path)
        .filter(|node| input.is_text_editor(node, target) || spin_editable(node))?;
    if !target.matches_node(node) || scene.row_key_at(&target.path) != target.row_key {
        return None;
    }
    let editor = input.editor.as_ref()?;
    let text = input.editor_value(node);
    let selection = editor.selection(&text);
    let caret = editor.position(&text);
    Some((target, text, selection, caret))
}

#[cfg(test)]
mod word_tests {
    use super::*;

    #[test]
    fn word_boundaries_keep_combining_sequences_and_joined_emoji_intact() {
        let text = "e\u{301}clair  👩‍💻 猫";
        let emoji = text.find('👩').unwrap();
        let mut editor = TextEdit::at_end(text);
        editor.apply(text, EditAction::WordLeft(false));
        assert_eq!(&text[editor.caret(text)..], "猫");
        editor.apply(text, EditAction::WordLeft(false));
        assert_eq!(editor.caret(text), emoji);
        editor.apply(text, EditAction::WordRight(true));
        assert_eq!(&text[editor.selection(text)], "👩‍💻");
        assert_eq!(
            editor.apply(text, EditAction::DeleteWord).as_deref(),
            Some("e\u{301}clair   猫")
        );

        let word = "first, second";
        let mut editor = TextEdit::at_end(word);
        assert_eq!(
            editor.apply(word, EditAction::BackspaceWord).as_deref(),
            Some("first, ")
        );
        assert_eq!(
            editor
                .apply("first, ", EditAction::BackspaceWord)
                .as_deref(),
            Some("first")
        );
    }

    #[test]
    fn overwrite_counts_new_graphemes_and_respects_line_boundaries() {
        let mut editor = TextEdit::default();
        let text = "e\u{301}👩‍💻\r\n猫";
        let result = editor
            .apply_mode(text, EditAction::Insert("ABC"), true, true)
            .unwrap();
        assert_eq!(result.text, "ABC\r\n猫");
        assert_eq!(result.removed, "e\u{301}👩‍💻");

        editor.place_caret(
            "e猫",
            TextPosition {
                offset: 1,
                trailing: true,
            },
            false,
        );
        let edit = editor
            .apply_mode("e猫", EditAction::Insert("\u{301}"), false, true)
            .unwrap();
        assert_eq!(edit.text, "e\u{301}猫");
        assert!(edit.removed.is_empty());

        editor.place_caret(text, TextPosition::default(), false);
        let edit = editor
            .apply_mode(text, EditAction::Insert("\n"), true, true)
            .unwrap();
        assert_eq!(edit.text, format!("\n{text}"));

        editor.set_selection(
            text,
            0,
            TextPosition {
                offset: "e\u{301}".len(),
                trailing: true,
            },
        );
        let edit = editor
            .apply_mode(text, EditAction::Insert("AB"), true, true)
            .unwrap();
        assert_eq!(edit.text, "AB👩‍💻\r\n猫");
    }

    #[test]
    fn pointer_paragraph_drag_keeps_paragraph_boundaries_in_both_directions() {
        let text = "左端\r\nsecond e\u{301} 👩‍💻\u{2029}last line";
        let second = text.find("second").unwrap();
        let third = text.find("last").unwrap();
        let position = |offset| TextPosition {
            offset,
            trailing: false,
        };
        let mut edit = TextEdit::default();
        edit.start_paragraph_drag(text, position(second + 2), false);
        assert_eq!(&text[edit.selection(text)], "second e\u{301} 👩‍💻");

        assert!(edit.extend_selection_drag(text, position(third + 1)));
        assert_eq!(
            &text[edit.selection(text)],
            "second e\u{301} 👩‍💻\u{2029}last line"
        );
        assert!(edit.extend_selection_drag(text, position(0)));
        assert_eq!(&text[edit.selection(text)], "左端\r\nsecond e\u{301} 👩‍💻");
        assert!(edit.extend_selection_drag(text, position(second)));
        assert_eq!(&text[edit.selection(text)], "second e\u{301} 👩‍💻");

        edit.start_paragraph_drag(text, position(0), true);
        assert_eq!(edit.selection(text), 0..text.len());
    }

    #[test]
    fn pointer_word_drag_keeps_its_anchor_when_extending_and_reversing() {
        let text = "alpha e\u{301}clair 👩‍💻 猫";
        let selected = text.find('e').unwrap();
        let after_word = text.find('👩').unwrap() - 1;
        let mut editor = TextEdit::at_end(text);
        editor.start_word_drag(
            text,
            TextPosition {
                offset: after_word,
                trailing: true,
            },
            false,
            true,
        );
        assert_eq!(&text[editor.selection(text)], "e\u{301}clair");
        assert!(editor.extend_selection_drag(
            text,
            TextPosition {
                offset: text.len(),
                trailing: true
            }
        ));
        assert_eq!(&text[editor.selection(text)], "e\u{301}clair 👩‍💻 猫");
        assert!(editor.extend_selection_drag(text, TextPosition::default()));
        assert_eq!(&text[editor.selection(text)], "alpha e\u{301}clair");
        assert!(editor.extend_selection_drag(
            text,
            TextPosition {
                offset: selected,
                trailing: false
            }
        ));
        assert_eq!(&text[editor.selection(text)], "e\u{301}clair");
        assert!(!editor.extend_selection_drag("replacement", TextPosition::default()));

        editor.start_word_drag(text, TextPosition::default(), true, true);
        assert_eq!(editor.selection(text), 0..text.len());

        let position = TextPosition {
            offset: 2,
            trailing: true,
        };
        editor.start_word_drag(text, position, false, false);
        assert_eq!(editor.selection(text), 2..2);
        assert!(editor.extend_selection_drag(
            text,
            TextPosition {
                offset: 3,
                trailing: true
            }
        ));
        assert_eq!(&text[editor.selection(text)], "alpha");
    }

    #[test]
    fn word_shortcuts_respect_the_keyboard_platform_and_alt_graph() {
        let option = KeyModifiers {
            alt: true,
            shift: true,
            ..Default::default()
        };
        let control = KeyModifiers {
            control: true,
            ..Default::default()
        };
        assert!(matches!(
            EditAction::word_shortcut("ArrowLeft", option, true),
            Some(EditAction::WordLeft(true))
        ));
        assert!(EditAction::word_shortcut("ArrowLeft", option, false).is_none());
        assert!(matches!(
            EditAction::word_shortcut("Backspace", control, false),
            Some(EditAction::BackspaceWord)
        ));
        assert!(EditAction::word_shortcut("Backspace", control, true).is_none());
        let both = KeyModifiers {
            alt: true,
            ..control
        };
        assert!(EditAction::word_shortcut("Delete", both, true).is_none());
        assert!(EditAction::word_shortcut("Delete", both, false).is_none());
    }
}
