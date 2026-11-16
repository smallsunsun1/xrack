use crate::InputValidity;
use std::{iter::Peekable, ops::Range};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy)]
enum Slot {
    Digit,
    Letter,
    Alphanumeric,
    Hexadecimal,
    Character,
}

impl Slot {
    fn accepts(self, value: &str) -> bool {
        match self {
            Self::Digit => value.len() == 1 && value.as_bytes()[0].is_ascii_digit(),
            Self::Hexadecimal => value.len() == 1 && value.as_bytes()[0].is_ascii_hexdigit(),
            Self::Letter => value.chars().next().is_some_and(char::is_alphabetic),
            Self::Alphanumeric => value.chars().next().is_some_and(char::is_alphanumeric),
            Self::Character => !value.chars().any(char::is_control),
        }
    }
}

enum Token<'a> {
    Slot(Slot),
    Literal(&'a str),
}

#[doc(hidden)]
pub struct InputMask<'a>(Vec<Token<'a>>);

#[doc(hidden)]
#[derive(Clone, Copy)]
pub enum MaskDelete {
    Backward,
    Forward,
}

#[doc(hidden)]
pub struct MaskEdit {
    pub text: String,
    pub caret: usize,
}

struct Formatted {
    edit: MaskEdit,
    slots: Vec<Range<usize>>,
    complete: bool,
}

#[derive(Clone, Copy)]
struct Input<'a> {
    grapheme: &'a str,
    may_be_literal: bool,
    before_caret: bool,
}

impl<'a> InputMask<'a> {
    pub fn parse(pattern: &'a str) -> Option<Self> {
        if pattern.chars().any(char::is_control) {
            return None;
        }
        let mut tokens = Vec::new();
        let mut graphemes = pattern.graphemes(true);
        let mut slots = 0;
        while let Some(grapheme) = graphemes.next() {
            let slot = match grapheme {
                "9" => Slot::Digit,
                "A" => Slot::Letter,
                "N" => Slot::Alphanumeric,
                "H" => Slot::Hexadecimal,
                "X" => Slot::Character,
                "\\" => {
                    tokens.push(Token::Literal(graphemes.next()?));
                    continue;
                }
                literal => {
                    tokens.push(Token::Literal(literal));
                    continue;
                }
            };
            slots += 1;
            tokens.push(Token::Slot(slot));
        }
        (slots > 0).then_some(Self(tokens))
    }

    fn raw(text: &str, caret: usize) -> impl Iterator<Item = Input<'_>> {
        text.grapheme_indices(true)
            .map(move |(offset, grapheme)| Input {
                grapheme,
                may_be_literal: true,
                before_caret: offset < caret,
            })
    }

    fn format<'b>(&self, input: impl Iterator<Item = Input<'b>>) -> Option<Formatted> {
        let mut input: Peekable<_> = input.peekable();
        let mut formatted = Formatted {
            edit: MaskEdit {
                text: String::new(),
                caret: 0,
            },
            slots: Vec::new(),
            complete: true,
        };
        let mut last_slot = 0;
        for token in &self.0 {
            match token {
                Token::Literal(literal) => {
                    if input
                        .peek()
                        .is_some_and(|part| part.may_be_literal && part.grapheme == *literal)
                    {
                        input.next();
                    }
                    formatted.edit.text.push_str(literal);
                }
                Token::Slot(slot) => {
                    let Some(part) = input.next() else {
                        // Only display leading/intermediate separators before a filled slot.
                        formatted.edit.text.truncate(last_slot);
                        formatted.complete = false;
                        return Some(formatted);
                    };
                    if !slot.accepts(part.grapheme) {
                        return None;
                    }
                    let start = formatted.edit.text.len();
                    formatted.edit.text.push_str(part.grapheme);
                    last_slot = formatted.edit.text.len();
                    formatted.slots.push(start..last_slot);
                    if part.before_caret {
                        formatted.edit.caret = last_slot;
                    }
                }
            }
        }
        if formatted.edit.caret == last_slot {
            formatted.edit.caret = formatted.edit.text.len();
        }
        input.next().is_none().then_some(formatted)
    }

    pub fn allows(&self, text: &str) -> bool {
        self.validity(text) != InputValidity::Invalid
    }

    pub fn accepts(&self, text: &str) -> bool {
        self.validity(text) == InputValidity::Acceptable
    }

    pub(crate) fn validity(&self, text: &str) -> InputValidity {
        match self.format(Self::raw(text, 0)) {
            Some(candidate) if candidate.edit.text == text => {
                if candidate.complete {
                    InputValidity::Acceptable
                } else {
                    InputValidity::Intermediate
                }
            }
            _ => InputValidity::Invalid,
        }
    }

    pub fn overwrite(&self, old: &str, caret: usize, inserted: &str) -> Option<MaskEdit> {
        let Some(original) = self
            .format(Self::raw(old, 0))
            .filter(|value| value.edit.text == old)
        else {
            return self.edit(old, caret..caret, inserted, None);
        };
        let mut leading = old.get(..caret)?.to_owned();
        leading.push_str(inserted);
        let leading = self.format(Self::raw(&leading, leading.len()))?;
        // Consumed mask slots determine the replaced range. Fixed separators consume no slot.
        let remaining = &original.slots[leading.slots.len().min(original.slots.len())..];
        let input =
            Self::raw(&leading.edit.text, leading.edit.caret).chain(remaining.iter().map(|slot| {
                Input {
                    grapheme: &old[slot.clone()],
                    may_be_literal: false,
                    before_caret: false,
                }
            }));
        self.format(input).map(|value| value.edit)
    }

    pub fn edit(
        &self,
        old: &str,
        range: Range<usize>,
        inserted: &str,
        delete: Option<MaskDelete>,
    ) -> Option<MaskEdit> {
        let mut candidate = old.get(..range.start)?.to_owned();
        candidate.push_str(inserted);
        let caret = candidate.len();
        candidate.push_str(old.get(range.end..)?);
        let formatted_candidate = self.format(Self::raw(&candidate, caret));
        if formatted_candidate
            .as_ref()
            .is_some_and(|value| value.edit.text == candidate)
        {
            // An insertion can join the preceding slot's grapheme cluster.
            return formatted_candidate.map(|value| value.edit);
        }
        let canonical = self
            .format(Self::raw(old, 0))
            .filter(|value| value.edit.text == old);
        let Some(canonical) = canonical else {
            return formatted_candidate.map(|value| value.edit);
        };
        let slots = canonical.slots;
        let mut first = slots.partition_point(|slot| slot.end <= range.start);
        let mut last = slots.partition_point(|slot| slot.start < range.end);
        if first == last && inserted.is_empty() {
            match delete {
                Some(MaskDelete::Backward) => first = first.saturating_sub(1),
                Some(MaskDelete::Forward) => last = (last + 1).min(slots.len()),
                None => {}
            }
        }
        let retained = |range: &Range<usize>, before_caret| Input {
            grapheme: &old[range.clone()],
            may_be_literal: false,
            before_caret,
        };
        let input = slots[..first]
            .iter()
            .map(|slot| retained(slot, true))
            .chain(Self::raw(inserted, inserted.len()))
            .chain(slots[last..].iter().map(|slot| retained(slot, false)));
        self.format(input).map(|result| result.edit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_digits_replaces_a_range_and_backspaces_across_separators() {
        let mask = InputMask::parse("9999-99-99").unwrap();
        let draft = mask.edit("", 0..0, "202611", None).unwrap();
        assert_eq!((draft.text.as_str(), draft.caret), ("2026-11", 7));
        assert!(mask.allows(&draft.text));
        assert!(!mask.accepts(&draft.text));

        let finished = mask.edit(&draft.text, 7..7, "17", None).unwrap();
        assert_eq!((finished.text.as_str(), finished.caret), ("2026-11-17", 10));
        assert!(mask.accepts(&finished.text));
        assert!(!mask.accepts("20261117"));
        assert!(mask.edit(&finished.text, 5..7, "a2", None).is_none());
        let replaced = mask.edit(&finished.text, 5..7, "12", None).unwrap();
        assert_eq!((replaced.text.as_str(), replaced.caret), ("2026-12-17", 7));
        let deleted = mask
            .edit(&replaced.text, 4..5, "", Some(MaskDelete::Backward))
            .unwrap();
        assert_eq!((deleted.text.as_str(), deleted.caret), ("2021-21-7", 3));
    }

    #[test]
    fn overwrite_keeps_literals_and_replaces_only_consumed_slots() {
        let mask = InputMask::parse("9999-99-99").unwrap();
        let old = "2026-11-17";
        let edit = mask.overwrite(old, 4, "9").unwrap();
        assert_eq!((edit.text.as_str(), edit.caret), ("2026-91-17", 6));
        let edit = mask.overwrite(old, 4, "12-2").unwrap();
        assert_eq!((edit.text.as_str(), edit.caret), ("2026-12-27", 9));
        assert!(mask.overwrite(old, 4, "word").is_none());

        let mask = InputMask::parse("A-A").unwrap();
        let edit = mask.overwrite("e-猫", 1, "\u{301}").unwrap();
        assert_eq!((edit.text.as_str(), edit.caret), ("e\u{301}-猫", 3));
    }

    #[test]
    fn accepts_combining_input_and_literal_slot_characters() {
        let mask = InputMask::parse("A").unwrap();
        let accented = mask.edit("e", 1..1, "\u{301}", None).unwrap();
        assert_eq!((accented.text.as_str(), accented.caret), ("e\u{301}", 3));
        assert!(mask.accepts(&accented.text));

        let escaped = InputMask::parse(r"\99H-N").unwrap();
        assert!(escaped.accepts("95F-中"));
        assert_eq!(escaped.edit("", 0..0, "5F中", None).unwrap().text, "95F-中");
        assert!(InputMask::parse("----").is_none());
        assert!(InputMask::parse("9\\").is_none());
    }
}
