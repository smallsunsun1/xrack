pub(crate) fn byte_offset(value: &str, units: usize) -> usize {
    let mut position = 0;
    for (index, character) in value.char_indices() {
        position += character.len_utf16();
        if position > units {
            return index;
        }
    }
    value.len()
}

#[cfg(any(target_os = "android", test))]
pub(crate) fn checked_byte_offset(value: &str, units: usize) -> Option<usize> {
    let offset = byte_offset(value, units);
    (value[..offset].encode_utf16().count() == units).then_some(offset)
}

#[cfg(any(target_os = "android", target_os = "ios", test))]
pub(crate) fn cover_graphemes(
    value: &str,
    range: std::ops::Range<usize>,
) -> std::ops::Range<usize> {
    use unicode_segmentation::UnicodeSegmentation;
    let mut start = 0;
    for boundary in value
        .grapheme_indices(true)
        .map(|(boundary, _)| boundary)
        .chain(std::iter::once(value.len()))
    {
        if boundary <= range.start {
            start = boundary;
        }
        if boundary >= range.end {
            return start..boundary;
        }
    }
    start..value.len()
}

#[cfg(test)]
mod tests {
    #[test]
    fn utf16_offsets_preserve_unicode_boundaries() {
        let value = "a🚰你";
        let offsets = (0..=6)
            .map(|units| super::byte_offset(value, units))
            .collect::<Vec<_>>();
        assert_eq!(offsets, [0, 1, 1, 5, 8, 8, 8]);
    }

    #[test]
    fn composing_mark_can_split_an_existing_grapheme() {
        let value = "ae\u{301}👩‍💻";
        assert_eq!(super::cover_graphemes(value, 1..2), 1..4);
        assert_eq!(super::cover_graphemes(value, 2..4), 1..4);
        assert_eq!(super::cover_graphemes(value, 2..2), 1..4);
        assert_eq!(super::cover_graphemes(value, 4..4), 4..4);
        assert_eq!(super::cover_graphemes(value, 4..8), 4..value.len());
    }
}
