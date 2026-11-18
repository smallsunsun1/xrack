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
}
