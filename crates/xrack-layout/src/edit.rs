use crate::{Interaction, Target};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;
use xrack_runtime::{Node, TextCommand, spin_editable};

#[derive(Clone, Debug, Default)]
pub struct TextEdit {
    caret: usize,
    anchor: usize,
    trailing: bool,
    preferred_x: Option<f32>,
    composition: Option<(u64, TextEditContext)>,
    pub preedit: String,
    pub preedit_cursor: Option<usize>,
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
        self.composition = None;
        self.update_preedit(value, cursor);
    }

    fn update_preedit(&mut self, value: String, cursor: Option<(usize, usize)>) {
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
        self.set_preedit(String::new(), None);
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
                let replace_all = matches!(action, EditAction::ReplaceAll(_));
                if filtered.is_empty() && !(replace_all && value.is_empty()) {
                    return None;
                }
                if replace_all {
                    (0..text.len(), filtered)
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
    pub fn for_clipboard(input: &Interaction, scene: &Node, command: TextCommand) -> Option<Self> {
        let node = scene.interactive_at_path(&input.focused.as_ref()?.path)?;
        if node.is_password() && command != TextCommand::Paste
            || node.boolean("read-only", false) && command != TextCommand::Copy
        {
            return None;
        }
        let context = Self::capture(input, scene)?;
        match command {
            TextCommand::Paste => Some(context),
            TextCommand::Cut | TextCommand::Copy if !context.selection().is_empty() => {
                Some(context)
            }
            _ => None,
        }
    }

    pub fn capture(input: &Interaction, scene: &Node) -> Option<Self> {
        let editor = input.editor.as_ref()?;
        if !editor.preedit.is_empty() || editor.composition.is_some() {
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
            .is_some_and(|editor| editor.preedit.is_empty() && editor.composition.is_none())
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
        let Some(context) = TextEditContext::capture(self, scene)
            .filter(|context| !self.menu_open() && context.can_compose(self, scene))
        else {
            return cancelled;
        };
        self.cancel_shortcut_sequence();
        self.dismiss_choice();
        self.break_text_edit_group();
        if let Some(editor) = &mut self.editor {
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
        let (id, context) = self.editor.as_ref()?.composition.as_ref()?;
        Some((*id, !self.menu_open() && context.can_compose(self, scene)))
    }
}

impl TextEditContext {
    fn can_compose(&self, input: &Interaction, scene: &Node) -> bool {
        self.matches(input, scene)
            && scene
                .interactive_at_path(&self.target.path)
                .is_some_and(|node| !node.boolean("read-only", false))
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
