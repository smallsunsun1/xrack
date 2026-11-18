use crate::{
    ControlAction, InputMask, InputRoute, InputValidity, Node, SpinDecimal, Value, combo_editable,
    decimal_separator, is_text_input, search_suggestion, spin_text_is_valid, text_input_value,
};
use std::cmp::Ordering;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputType {
    #[default]
    Text,
    Password,
    Number,
    Decimal,
    Search,
    Email,
    Url,
    Tel,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReturnKey {
    #[default]
    Auto,
    Next,
    Done,
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
            ("ComboBox", Some(Value::InputType(mode))) if combo_editable(self) => *mode,
            _ => InputType::Text,
        }
    }

    pub fn return_key(&self) -> ReturnKey {
        match (self.kind, self.properties.get("return-key")) {
            ("TextInput" | "LineEdit", Some(Value::ReturnKey(key))) => *key,
            ("ComboBox", Some(Value::ReturnKey(key))) if combo_editable(self) => *key,
            _ => ReturnKey::Auto,
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
        if !self
            .text("input-mask")
            .filter(|mask| !mask.is_empty())
            .is_none_or(|mask| InputMask::parse(mask).is_some_and(|mask| mask.allows(text)))
            || !text_syntax_allows(text, mode, separator)?
        {
            return Ok(false);
        }
        let limit = maximum_length()?.and_then(|limit| usize::try_from(limit).ok());
        let length_allowed = limit.is_none_or(|limit| {
            let length = text.graphemes(true).count();
            // An external update can leave the value over its limit. Let edits repair it.
            length <= limit || length < text_input_value(self).graphemes(true).count()
        });
        Ok(length_allowed
            && self
                .text("input-pattern")
                .is_none_or(|pattern| crate::input_pattern::input_pattern_allows(pattern, text)))
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

    /// Reject an edit based on text replaced since the scene was prepared.
    #[doc(hidden)]
    pub fn try_prepare_text_edit<E>(
        &self,
        input: InputRoute<'_>,
        editable: impl FnOnce() -> Result<bool, E>,
        current: impl FnOnce() -> Result<String, E>,
        validate: impl FnOnce() -> Result<bool, E>,
    ) -> Result<bool, E> {
        if !input.try_available(editable)? {
            return Ok(false);
        }
        // Keep the draft read in the route's watch, outside its availability
        // watch. A linked table write checks its own field after reading history.
        if current()? != text_input_value(self) {
            return Ok(false);
        }
        input.try_available(validate)
    }

    #[doc(hidden)]
    pub fn prepare_text_edit(
        &self,
        input: InputRoute<'_>,
        editable: impl FnOnce() -> bool,
        current: impl FnOnce() -> String,
        validate: impl FnOnce() -> bool,
    ) -> bool {
        self.try_prepare_text_edit(
            input,
            || Ok::<_, std::convert::Infallible>(editable()),
            || Ok(current()),
            || Ok(validate()),
        )
        .unwrap_or_else(|error| match error {})
    }

    /// Check whether the current value can be submitted by an editor.
    pub fn accepts_text_submission(&self) -> bool {
        let text = text_input_value(self);
        self.text("input-mask")
            .filter(|mask| !mask.is_empty())
            .is_none_or(|mask| InputMask::parse(mask).is_some_and(|mask| mask.accepts(text)))
            && text_submission_is_valid(
                text,
                self.input_type(),
                || decimal_separator(self),
                || self.int("maximum-length"),
            )
            && self
                .text("input-pattern")
                .is_none_or(|pattern| crate::input_pattern_accepts(pattern, text))
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
            && submission_has_value(text, mode),
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

/// Optional numeric limits for a single-line editor's submitted value.
#[doc(hidden)]
#[derive(Default)]
pub struct NumericTextLimits {
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub decimals: Option<i64>,
}

impl NumericTextLimits {
    fn allows(self, text: &str, mode: InputType, separator: char) -> bool {
        if let Some(places) = self
            .decimals
            .and_then(|places| usize::try_from(places).ok())
            && text
                .split_once(separator)
                .is_some_and(|(_, fraction)| fraction.len() > places)
        {
            return false;
        }
        if self.minimum.is_none() && self.maximum.is_none() {
            return true;
        }
        if mode == InputType::Number {
            // Compare the digits to the integral bounds before any float
            // conversion, so adjacent large integers remain distinct.
            let digits = text.trim_start_matches('0');
            let digits = if digits.is_empty() { "0" } else { digits };
            let compare = |bound: f64| {
                let bound = format!("{:.0}", bound.abs());
                digits
                    .len()
                    .cmp(&bound.len())
                    .then_with(|| digits.cmp(&bound))
            };
            return self.minimum.is_none_or(|minimum| {
                minimum.is_finite() && (minimum <= 0.0 || compare(minimum.ceil()) != Ordering::Less)
            }) && self.maximum.is_none_or(|maximum| {
                maximum.is_finite()
                    && maximum >= 0.0
                    && compare(maximum.floor()) != Ordering::Greater
            });
        }
        let Some(number) = SpinDecimal::parse(text, separator).map(SpinDecimal::get) else {
            return false;
        };
        self.minimum
            .is_none_or(|minimum| minimum.is_finite() && number >= minimum)
            && self
                .maximum
                .is_none_or(|maximum| maximum.is_finite() && number <= maximum)
    }
}

/// Evaluate final submission conditions in order, keeping later bindings lazy.
#[doc(hidden)]
pub fn try_text_submission_with<E>(
    text: &str,
    mode: InputType,
    separator: impl FnOnce() -> Result<char, E>,
    maximum_length: impl FnOnce() -> Result<Option<i64>, E>,
    minimum_length: impl FnOnce() -> Result<i64, E>,
    numeric_limits: impl FnOnce(InputType) -> Result<NumericTextLimits, E>,
    submit_when: impl FnOnce() -> Result<bool, E>,
) -> Result<bool, E> {
    try_text_submission_with_mask(
        text,
        mode,
        separator,
        maximum_length,
        minimum_length,
        numeric_limits,
        || Ok(String::new()),
        submit_when,
    )
}

#[doc(hidden)]
pub fn text_submission_with(
    text: &str,
    mode: InputType,
    separator: impl FnOnce() -> char,
    maximum_length: impl FnOnce() -> Option<i64>,
    minimum_length: impl FnOnce() -> i64,
    numeric_limits: impl FnOnce(InputType) -> NumericTextLimits,
    submit_when: impl FnOnce() -> bool,
) -> bool {
    try_text_submission_with(
        text,
        mode,
        || Ok::<_, std::convert::Infallible>(separator()),
        || Ok(maximum_length()),
        || Ok(minimum_length()),
        |mode| Ok(numeric_limits(mode)),
        || Ok(submit_when()),
    )
    .unwrap_or_else(|error| match error {})
}

pub fn try_text_submission_with_mask<E>(
    text: &str,
    mode: InputType,
    separator: impl FnOnce() -> Result<char, E>,
    maximum_length: impl FnOnce() -> Result<Option<i64>, E>,
    minimum_length: impl FnOnce() -> Result<i64, E>,
    numeric_limits: impl FnOnce(InputType) -> Result<NumericTextLimits, E>,
    mask: impl FnOnce() -> Result<String, E>,
    submit_when: impl FnOnce() -> Result<bool, E>,
) -> Result<bool, E> {
    let separator = if mode == InputType::Decimal {
        separator()?
    } else {
        '.'
    };
    if !try_text_submission_is_valid(text, mode, || Ok(separator), maximum_length)? {
        return Ok(false);
    }
    if !try_text_submission_limits(text, mode, separator, minimum_length, numeric_limits)? {
        return Ok(false);
    }
    let mask = mask()?;
    if !mask.is_empty() && !InputMask::parse(&mask).is_some_and(|pattern| pattern.accepts(text)) {
        return Ok(false);
    }
    submit_when()
}

fn submission_has_value(text: &str, mode: InputType) -> bool {
    !matches!(mode, InputType::Number | InputType::Decimal)
        || text.bytes().any(|byte| byte.is_ascii_digit())
}

fn try_text_submission_limits<E>(
    text: &str,
    mode: InputType,
    separator: char,
    minimum_length: impl FnOnce() -> Result<i64, E>,
    numeric_limits: impl FnOnce(InputType) -> Result<NumericTextLimits, E>,
) -> Result<bool, E> {
    let Ok(minimum) = usize::try_from(minimum_length()?.max(0)) else {
        return Ok(false);
    };
    if minimum > text.len() || text.graphemes(true).take(minimum).count() != minimum {
        return Ok(false);
    }
    if matches!(mode, InputType::Number | InputType::Decimal)
        && !numeric_limits(mode)?.allows(text, mode, separator)
    {
        return Ok(false);
    }
    Ok(true)
}

#[doc(hidden)]
pub fn text_submission_with_mask(
    text: &str,
    mode: InputType,
    separator: impl FnOnce() -> char,
    maximum_length: impl FnOnce() -> Option<i64>,
    minimum_length: impl FnOnce() -> i64,
    numeric_limits: impl FnOnce(InputType) -> NumericTextLimits,
    mask: impl FnOnce() -> String,
    submit_when: impl FnOnce() -> bool,
) -> bool {
    try_text_submission_with_mask(
        text,
        mode,
        || Ok::<_, std::convert::Infallible>(separator()),
        || Ok(maximum_length()),
        || Ok(minimum_length()),
        |mode| Ok(numeric_limits(mode)),
        || Ok(mask()),
        || Ok(submit_when()),
    )
    .unwrap_or_else(|error| match error {})
}

#[doc(hidden)]
pub fn try_text_input_validity<E>(
    text: &str,
    mode: InputType,
    separator: impl FnOnce() -> Result<char, E>,
    maximum_length: impl FnOnce() -> Result<Option<i64>, E>,
    minimum_length: impl FnOnce() -> Result<i64, E>,
    numeric_limits: impl FnOnce(InputType) -> Result<NumericTextLimits, E>,
    mask: impl FnOnce() -> Result<String, E>,
    pattern: impl FnOnce() -> Result<String, E>,
    submit_when: impl FnOnce() -> Result<bool, E>,
) -> Result<InputValidity, E> {
    let separator = if mode == InputType::Decimal {
        separator()?
    } else {
        '.'
    };
    if !text_syntax_allows(text, mode, || Ok(separator))?
        || maximum_length()?
            .and_then(|limit| usize::try_from(limit).ok())
            .is_some_and(|limit| text.graphemes(true).nth(limit).is_some())
    {
        return Ok(InputValidity::Invalid);
    }
    let mask = mask()?;
    let masked = if mask.is_empty() {
        InputValidity::Acceptable
    } else {
        InputMask::parse(&mask).map_or(InputValidity::Invalid, |mask| mask.validity(text))
    };
    if masked == InputValidity::Invalid {
        return Ok(InputValidity::Invalid);
    }
    let matched = crate::input_pattern::pattern_validity(&pattern()?, text);
    if matched == InputValidity::Invalid {
        return Ok(InputValidity::Invalid);
    }
    if masked == InputValidity::Acceptable
        && matched == InputValidity::Acceptable
        && submission_has_value(text, mode)
        && try_text_submission_limits(text, mode, separator, minimum_length, numeric_limits)?
        && submit_when()?
    {
        Ok(InputValidity::Acceptable)
    } else {
        Ok(InputValidity::Intermediate)
    }
}

#[doc(hidden)]
pub fn text_input_validity(
    text: &str,
    mode: InputType,
    separator: impl FnOnce() -> char,
    maximum_length: impl FnOnce() -> Option<i64>,
    minimum_length: impl FnOnce() -> i64,
    numeric_limits: impl FnOnce(InputType) -> NumericTextLimits,
    mask: impl FnOnce() -> String,
    pattern: impl FnOnce() -> String,
    submit_when: impl FnOnce() -> bool,
) -> InputValidity {
    try_text_input_validity(
        text,
        mode,
        || Ok::<_, std::convert::Infallible>(separator()),
        || Ok(maximum_length()),
        || Ok(minimum_length()),
        |mode| Ok(numeric_limits(mode)),
        || Ok(mask()),
        || Ok(pattern()),
        || Ok(submit_when()),
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
        InputType::Text
        | InputType::Password
        | InputType::Search
        | InputType::Email
        | InputType::Url
        | InputType::Tel => true,
    })
}
