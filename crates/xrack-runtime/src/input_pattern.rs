use regex_automata::{
    Anchored, Input, MatchKind,
    dfa::{Automaton, StartKind, dense, sparse},
    nfa::thompson,
    util::primitives::StateID,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque, hash_map::Entry},
};

const CACHED_PATTERNS: usize = 16;

thread_local! {
    static PATTERNS: RefCell<VecDeque<(String, Option<InputPattern>)>> =
        const { RefCell::new(VecDeque::new()) };
}

/// The validity of a draft relative to the editor's current submission rules.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputValidity {
    Acceptable,
    Intermediate,
    #[default]
    Invalid,
}

struct InputPattern {
    dfa: sparse::DFA<Vec<u8>>,
    start: StateID,
    completable: HashSet<StateID>,
}

impl InputPattern {
    fn compile(pattern: &str) -> Option<Self> {
        let dfa = dense::Builder::new()
            .thompson(thompson::Config::new().nfa_size_limit(Some(1024 * 1024)))
            .configure(
                dense::Config::new()
                    .start_kind(StartKind::Anchored)
                    .match_kind(MatchKind::All)
                    .dfa_size_limit(Some(1024 * 1024))
                    .determinize_size_limit(Some(4 * 1024 * 1024)),
            )
            .build(pattern)
            .ok()?
            .to_sparse()
            .ok()?;
        let start = dfa
            .start_state_forward(&Input::new("").anchored(Anchored::Yes))
            .ok()?;
        let completable = completable_states(&dfa, start);
        Some(Self {
            dfa,
            start,
            completable,
        })
    }

    fn classify(&self, text: &str) -> InputValidity {
        let mut state = self.start;
        for byte in text.bytes() {
            state = self.dfa.next_state(state, byte);
            if self.dfa.is_dead_state(state) || self.dfa.is_quit_state(state) {
                return InputValidity::Invalid;
            }
        }
        // The anchored DFA reports matches one byte late. Checking only its
        // end-of-input transition requires a full match of the draft.
        if self.dfa.is_match_state(self.dfa.next_eoi_state(state)) {
            InputValidity::Acceptable
        } else if self.completable.contains(&state) {
            InputValidity::Intermediate
        } else {
            InputValidity::Invalid
        }
    }
}

fn completable_states(dfa: &sparse::DFA<Vec<u8>>, start: StateID) -> HashSet<StateID> {
    let mut seen_classes = [false; 256];
    let alphabet: Vec<u8> = (u8::MIN..=u8::MAX)
        .filter(|byte| {
            let seen = &mut seen_classes[usize::from(dfa.byte_classes().get(*byte))];
            !std::mem::replace(seen, true)
        })
        .collect();
    let mut predecessors = HashMap::from([(start, Vec::new())]);
    let mut reachable = VecDeque::from([start]);
    let mut accepting = VecDeque::new();
    while let Some(state) = reachable.pop_front() {
        if dfa.is_match_state(dfa.next_eoi_state(state)) {
            accepting.push_back(state);
        }
        for &byte in &alphabet {
            let next = dfa.next_state(state, byte);
            if dfa.is_dead_state(next) || dfa.is_quit_state(next) {
                continue;
            }
            match predecessors.entry(next) {
                Entry::Occupied(mut entry) => entry.get_mut().push(state),
                Entry::Vacant(entry) => {
                    entry.insert(vec![state]);
                    reachable.push_back(next);
                }
            }
        }
    }
    let mut completable = HashSet::new();
    while let Some(state) = accepting.pop_front() {
        if completable.insert(state)
            && let Some(previous) = predecessors.remove(&state)
        {
            accepting.extend(previous);
        }
    }
    completable
}

fn with_pattern<T>(pattern: &str, use_pattern: impl FnOnce(&Option<InputPattern>) -> T) -> T {
    PATTERNS.with_borrow_mut(|cache| {
        let cached = cache
            .iter()
            .position(|(source, _)| source == pattern)
            .and_then(|index| cache.remove(index));
        let entry = cached.unwrap_or_else(|| (pattern.to_owned(), InputPattern::compile(pattern)));
        let result = use_pattern(&entry.1);
        cache.push_back(entry);
        if cache.len() > CACHED_PATTERNS {
            cache.pop_front();
        }
        result
    })
}

pub(crate) fn pattern_validity(pattern: &str, text: &str) -> InputValidity {
    if pattern.is_empty() {
        InputValidity::Acceptable
    } else {
        with_pattern(pattern, |entry| {
            entry
                .as_ref()
                .map_or(InputValidity::Invalid, |pattern| pattern.classify(text))
        })
    }
}

#[doc(hidden)]
pub fn input_pattern_accepts(pattern: &str, text: &str) -> bool {
    pattern_validity(pattern, text) == InputValidity::Acceptable
}

pub(crate) fn input_pattern_allows(pattern: &str, text: &str) -> bool {
    pattern_validity(pattern, text) != InputValidity::Invalid
}

#[cfg(test)]
mod tests {
    use super::{InputPattern, InputValidity::*, pattern_validity};

    #[test]
    fn separates_complete_inputs_prefixes_and_invalid_drafts() {
        for (pattern, text, expected) in [
            ("[A-Z]{2}-[0-9]{3}", "", Intermediate),
            ("[A-Z]{2}-[0-9]{3}", "AB-12", Intermediate),
            ("[A-Z]{2}-[0-9]{3}", "AB-123", Acceptable),
            ("[A-Z]{2}-[0-9]{3}", "AB-1234", Invalid),
            ("[A-Z]{2}-[0-9]{3}", "AB-x", Invalid),
            ("a|abc", "a", Acceptable),
            ("a|abc", "ab", Intermediate),
            ("a|abc", "abc", Acceptable),
            ("a|abc", "ax", Invalid),
            ("(?m)^A$", "A\n", Invalid),
            (r"\p{L}{2}[0-9]", "中文", Intermediate),
            (r"\p{L}{2}[0-9]", "中文1", Acceptable),
            (r"\p{L}{2}[0-9]", "中1", Invalid),
            ("(?x) A #comment to end of pattern", "A", Acceptable),
            (r"a\Ab", "", Invalid),
            ("", "any text", Acceptable),
        ] {
            assert_eq!(
                pattern_validity(pattern, text),
                expected,
                "{pattern:?} with {text:?}"
            );
        }
    }

    #[test]
    fn invalid_dynamic_patterns_reject_both_edits_and_submission() {
        assert!(InputPattern::compile("[").is_none());
        assert_eq!(pattern_validity("[", "a"), Invalid);
        assert!(InputPattern::compile("a{1000000000}").is_none());
    }

    #[test]
    fn validity_combines_draft_checks_and_keeps_submission_rules_lazy() {
        use crate::{InputType, NumericTextLimits, text_input_validity};
        use std::cell::Cell;

        let calls = Cell::new(0);
        let validate = |text: &str, mode, mask: &str, pattern: &str, allowed| {
            text_input_validity(
                text,
                mode,
                || '.',
                || Some(6),
                || 2,
                |_| NumericTextLimits {
                    maximum: Some(10.0),
                    ..NumericTextLimits::default()
                },
                || mask.to_owned(),
                || pattern.to_owned(),
                || {
                    calls.set(calls.get() + 1);
                    allowed
                },
            )
        };
        assert_eq!(validate("xx", InputType::Number, "", "", true), Invalid);
        assert_eq!(validate("hello!!", InputType::Text, "", "", true), Invalid);
        assert_eq!(
            validate("-", InputType::Decimal, "", "", true),
            Intermediate
        );
        assert_eq!(
            validate("12", InputType::Number, "", "", true),
            Intermediate
        );
        assert_eq!(
            validate("12-3", InputType::Text, "99-99", "", true),
            Intermediate
        );
        assert_eq!(
            validate("12-3", InputType::Text, "99-99", "[A-Z]+", true),
            Invalid
        );
        assert_eq!(calls.get(), 0);
        assert_eq!(
            validate("12-34", InputType::Text, "99-99", "", false),
            Intermediate
        );
        assert_eq!(
            validate("12-34", InputType::Text, "99-99", "", true),
            Acceptable
        );
        assert_eq!(calls.get(), 2);
    }
}
