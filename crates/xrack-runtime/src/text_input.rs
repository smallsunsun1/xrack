use crate::{
    ControlAction, Node, Value, combo_editable, decimal_separator, is_text_input,
    search_suggestion, spin_text_is_valid, text_input_value,
};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputType {
    #[default]
    Text,
    Password,
    Number,
    Decimal,
    Search,
}

#[doc(hidden)]
pub enum SearchInput {
    Edit(String),
    Submit(String),
    Select { text: String, changed: bool },
}

#[doc(hidden)]
pub enum SearchUpdate<'a> {
    SetText(&'a str),
    Edited(&'a str),
    Accepted(&'a str),
    Selected(&'a str),
    Triggered(&'a str),
}

impl SearchInput {
    /// Check the live field and preserve the query or suggestion from the input scene.
    #[doc(hidden)]
    pub fn try_from_control<E>(
        node: &Node,
        action: ControlAction,
        available: impl FnOnce() -> Result<bool, E>,
        editable: impl FnOnce() -> Result<bool, E>,
        current: impl FnOnce() -> Result<String, E>,
        result_query: impl FnOnce() -> Result<Option<String>, E>,
        suggestion: impl FnOnce(usize) -> Result<Option<String>, E>,
    ) -> Result<Option<Self>, E> {
        if !matches!(
            action,
            ControlAction::SubmitSearch | ControlAction::Select(_) | ControlAction::ClearSearch
        ) {
            return Ok(None);
        }
        crate::property::watch_reads(|reads| {
            if !available()? || reads.changed() {
                return Ok(None);
            }
            let shown = text_input_value(node);
            if action == ControlAction::SubmitSearch {
                let query = current()?;
                return Ok((query == shown && !reads.changed()).then_some(Self::Submit(query)));
            }
            if shown.is_empty()
                || node.boolean("read-only", false)
                || !editable()?
                || reads.changed()
                || current()? != shown
                || reads.changed()
            {
                return Ok(None);
            }
            if action == ControlAction::ClearSearch {
                return Ok(Some(Self::Edit(String::new())));
            }
            let ControlAction::Select(index) = action else {
                return Ok(None);
            };
            let Some(candidate) = search_suggestion(node, index) else {
                return Ok(None);
            };
            if result_query()?.is_some_and(|query| query != shown) || reads.changed() {
                return Ok(None);
            }
            let Some(text) = suggestion(index)? else {
                return Ok(None);
            };
            Ok(
                (text == candidate && !reads.changed()).then(|| Self::Select {
                    changed: text != shown,
                    text,
                }),
            )
        })
    }

    #[doc(hidden)]
    pub fn from_control(
        node: &Node,
        action: ControlAction,
        available: impl FnOnce() -> bool,
        editable: impl FnOnce() -> bool,
        current: impl FnOnce() -> String,
        result_query: impl FnOnce() -> Option<String>,
        suggestion: impl FnOnce(usize) -> Option<String>,
    ) -> Option<Self> {
        Self::try_from_control(
            node,
            action,
            || Ok::<_, std::convert::Infallible>(available()),
            || Ok(editable()),
            || Ok(current()),
            || Ok(result_query()),
            |index| Ok(suggestion(index)),
        )
        .unwrap_or_else(|error| match error {})
    }

    #[doc(hidden)]
    pub fn try_apply<E>(
        self,
        live: impl FnOnce() -> Result<bool, E>,
        mut apply: impl FnMut(SearchUpdate<'_>) -> Result<bool, E>,
    ) -> Result<bool, E> {
        match self {
            Self::Edit(text) => {
                if !apply(SearchUpdate::SetText(&text))? {
                    return Ok(false);
                }
                apply(SearchUpdate::Edited(&text))?;
                if live()? {
                    apply(SearchUpdate::Triggered(&text))?;
                }
            }
            Self::Submit(text) => {
                apply(SearchUpdate::Accepted(&text))?;
                apply(SearchUpdate::Triggered(&text))?;
            }
            Self::Select { text, changed } => {
                if changed {
                    if !apply(SearchUpdate::SetText(&text))? {
                        return Ok(false);
                    }
                    apply(SearchUpdate::Edited(&text))?;
                }
                apply(SearchUpdate::Selected(&text))?;
                apply(SearchUpdate::Triggered(&text))?;
            }
        }
        Ok(true)
    }

    #[doc(hidden)]
    pub fn apply(
        self,
        live: impl FnOnce() -> bool,
        mut apply: impl FnMut(SearchUpdate<'_>),
    ) -> bool {
        self.try_apply(
            || Ok::<_, std::convert::Infallible>(live()),
            |update| {
                apply(update);
                Ok(true)
            },
        )
        .unwrap_or_else(|error| match error {})
    }
}

impl Node {
    pub fn input_type(&self) -> InputType {
        match (self.kind, self.properties.get("input-type")) {
            ("TextInput" | "LineEdit", Some(Value::InputType(mode))) => *mode,
            _ => InputType::Text,
        }
    }

    /// Check a proposed user edit, allowing incomplete numeric drafts.
    pub fn allows_text_edit(&self, text: &str) -> bool {
        self.allows_text_edit_with(
            text,
            self.input_type(),
            || decimal_separator(self),
            || self.int("maximum-length"),
        )
    }

    /// Check live text constraints against the draft displayed by the input scene.
    #[doc(hidden)]
    pub fn try_allows_text_edit_with<E>(
        &self,
        text: &str,
        mode: InputType,
        separator: impl FnOnce() -> Result<char, E>,
        maximum_length: impl FnOnce() -> Result<Option<i64>, E>,
    ) -> Result<bool, E> {
        if !text_syntax_allows(text, mode, separator)? {
            return Ok(false);
        }
        let limit = maximum_length()?.and_then(|limit| usize::try_from(limit).ok());
        Ok(limit.is_none_or(|limit| {
            let length = text.graphemes(true).count();
            // An external update can leave the value over its limit. Let edits repair it.
            length <= limit || length < text_input_value(self).graphemes(true).count()
        }))
    }

    #[doc(hidden)]
    pub fn allows_text_edit_with(
        &self,
        text: &str,
        mode: InputType,
        separator: impl FnOnce() -> char,
        maximum_length: impl FnOnce() -> Option<i64>,
    ) -> bool {
        self.try_allows_text_edit_with(
            text,
            mode,
            || Ok::<_, std::convert::Infallible>(separator()),
            || Ok(maximum_length()),
        )
        .unwrap_or_else(|error| match error {})
    }

    #[doc(hidden)]
    pub fn accepts_editor_update(&self, text: &str) -> bool {
        (is_text_input(self.kind) || combo_editable(self))
            && !self.boolean("read-only", false)
            && text_input_value(self) != text
            && self.allows_text_edit(text)
    }

    /// Check whether the current value can be submitted by an editor.
    pub fn accepts_text_submission(&self) -> bool {
        text_submission_is_valid(
            text_input_value(self),
            self.input_type(),
            || decimal_separator(self),
            || self.int("maximum-length"),
        )
    }
}

/// Validate a submitted value against the current editor bindings.
#[doc(hidden)]
pub fn try_text_submission_is_valid<E>(
    text: &str,
    mode: InputType,
    separator: impl FnOnce() -> Result<char, E>,
    maximum_length: impl FnOnce() -> Result<Option<i64>, E>,
) -> Result<bool, E> {
    if !text_syntax_allows(text, mode, separator)? {
        return Ok(false);
    }
    let limit = maximum_length()?.and_then(|limit| usize::try_from(limit).ok());
    Ok(
        limit.is_none_or(|limit| text.graphemes(true).nth(limit).is_none())
            && (!matches!(mode, InputType::Number | InputType::Decimal)
                || text.bytes().any(|byte| byte.is_ascii_digit())),
    )
}

#[doc(hidden)]
pub fn text_submission_is_valid(
    text: &str,
    mode: InputType,
    separator: impl FnOnce() -> char,
    maximum_length: impl FnOnce() -> Option<i64>,
) -> bool {
    try_text_submission_is_valid(
        text,
        mode,
        || Ok::<_, std::convert::Infallible>(separator()),
        || Ok(maximum_length()),
    )
    .unwrap_or_else(|error| match error {})
}

fn text_syntax_allows<E>(
    text: &str,
    mode: InputType,
    separator: impl FnOnce() -> Result<char, E>,
) -> Result<bool, E> {
    Ok(match mode {
        InputType::Number => text.bytes().all(|byte| byte.is_ascii_digit()),
        InputType::Decimal => spin_text_is_valid(text, Some(separator()?)),
        InputType::Text | InputType::Password | InputType::Search => true,
    })
}
