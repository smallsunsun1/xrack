//! Text directives shared by the Wavefront loader and the editor.

pub fn directives(source: &str) -> impl Iterator<Item = (usize, &str, &str)> {
    source
        .trim_start_matches('\u{feff}')
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let line = line.split_once('#').map_or(line, |(code, _)| code).trim();
            (!line.is_empty()).then(|| {
                let (command, value) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
                (index + 1, command, value.trim())
            })
        })
}
