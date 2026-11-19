use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

const CHARACTER: i32 = 1;
const WORD: i32 = 2;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Traversal {
    pub selection: [usize; 2],
    pub segment: [i32; 2],
    pub granularity: i32,
    pub forward: bool,
}

pub(crate) fn traverse(
    value: &str,
    previous: Option<[usize; 2]>,
    granularity: i32,
    forward: bool,
    extend: bool,
) -> Option<Traversal> {
    let caret = match previous {
        Some([anchor, focus]) => {
            crate::text_indices::checked_byte_offset(value, anchor)?;
            crate::text_indices::checked_byte_offset(value, focus)?
        }
        None if forward => 0,
        None => value.len(),
    };
    let segment = match granularity {
        CHARACTER => adjacent(
            value
                .grapheme_indices(true)
                .map(|(start, text)| start..start + text.len()),
            caret,
            forward,
            false,
        ),
        WORD => adjacent(
            value
                .unicode_word_indices()
                .map(|(start, text)| start..start + text.len()),
            caret,
            forward,
            true,
        ),
        _ => None,
    }?;
    let start = value[..segment.start].encode_utf16().count();
    let end = start + value[segment].encode_utf16().count();
    let focus = if forward { end } else { start };
    let anchor = if extend {
        previous.map_or(if forward { start } else { end }, |[anchor, _]| anchor)
    } else {
        focus
    };
    Some(Traversal {
        selection: [anchor, focus],
        segment: [i32::try_from(start).ok()?, i32::try_from(end).ok()?],
        granularity,
        forward,
    })
}

fn adjacent(
    mut segments: impl Iterator<Item = Range<usize>>,
    caret: usize,
    forward: bool,
    partial: bool,
) -> Option<Range<usize>> {
    if forward {
        let segment = segments.find(|segment| {
            if partial {
                segment.end > caret
            } else {
                segment.start >= caret
            }
        })?;
        Some(segment.start.max(caret)..segment.end)
    } else {
        let segment = segments
            .take_while(|segment| {
                if partial {
                    segment.start < caret
                } else {
                    segment.end <= caret
                }
            })
            .last()?;
        Some(segment.start..segment.end.min(caret))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_traversal_keeps_combining_text_and_joined_emoji_whole() {
        let text = "e\u{301}👩‍💻\r\n";
        let first = traverse(text, None, CHARACTER, true, false).unwrap();
        assert_eq!(first.segment, [0, 2]);
        assert_eq!(first.selection, [2, 2]);
        let emoji = traverse(text, Some(first.selection), CHARACTER, true, true).unwrap();
        assert_eq!(emoji.segment, [2, 7]);
        assert_eq!(emoji.selection, [2, 7]);
        let newline = traverse(text, Some(emoji.selection), CHARACTER, true, true).unwrap();
        assert_eq!(newline.segment, [7, 9]);
        assert_eq!(newline.selection, [2, 9]);
        assert!(traverse(text, Some(newline.selection), CHARACTER, true, false).is_none());
        assert!(traverse(text, Some([3, 3]), CHARACTER, false, false).is_none());
    }

    #[test]
    fn word_traversal_skips_separators_and_extends_from_the_original_anchor() {
        let text = "one, two";
        let next = traverse(text, Some([1, 3]), WORD, true, true).unwrap();
        assert_eq!(next.segment, [5, 8]);
        assert_eq!(next.selection, [1, 8]);
        let previous = traverse(text, Some(next.selection), WORD, false, false).unwrap();
        assert_eq!(previous.segment, [5, 8]);
        assert_eq!(previous.selection, [5, 5]);
        assert_eq!(
            traverse(text, Some([1, 1]), WORD, true, false)
                .unwrap()
                .segment,
            [1, 3]
        );
        assert!(traverse(text, Some([0, 0]), WORD, false, false).is_none());
    }
}
