use crate::{Node, Value, decimal_separator, spin_text_is_valid, text_input_value};
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

impl Node {
    pub fn input_type(&self) -> InputType {
        match (self.kind, self.properties.get("input-type")) {
            ("TextInput" | "LineEdit", Some(Value::InputType(mode))) => *mode,
            _ => InputType::Text,
        }
    }

    fn text_syntax_allows(&self, text: &str) -> bool {
        match self.input_type() {
            InputType::Number => text.bytes().all(|byte| byte.is_ascii_digit()),
            InputType::Decimal => spin_text_is_valid(text, Some(decimal_separator(self))),
            InputType::Text | InputType::Password | InputType::Search => true,
        }
    }

    fn maximum_text_length(&self) -> Option<usize> {
        usize::try_from(self.int("maximum-length")?).ok()
    }

    /// Check a proposed user edit, allowing incomplete numeric drafts.
    pub fn allows_text_edit(&self, text: &str) -> bool {
        self.text_syntax_allows(text)
            && self.maximum_text_length().is_none_or(|limit| {
                let length = text.graphemes(true).count();
                // An external update can leave the value over its limit. Let edits repair it.
                length <= limit || length < text_input_value(self).graphemes(true).count()
            })
    }

    /// Check whether the current value can be submitted by an editor.
    pub fn accepts_text_submission(&self) -> bool {
        let text = text_input_value(self);
        self.text_syntax_allows(text)
            && self
                .maximum_text_length()
                .is_none_or(|limit| text.graphemes(true).nth(limit).is_none())
            && (!matches!(self.input_type(), InputType::Number | InputType::Decimal)
                || text.bytes().any(|byte| byte.is_ascii_digit()))
    }
}
