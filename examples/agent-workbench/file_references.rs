use std::{borrow::Cow, path::Path};
use xrack_runtime::{MarkdownStream, StyledText};

const LINK_PREFIX: &str = "workspace-file:";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct LineRange {
    pub first: usize,
    pub last: usize,
}

impl LineRange {
    pub fn parse(text: &str) -> Option<Self> {
        fn line(text: &str) -> Option<usize> {
            let text = text.strip_prefix('L').unwrap_or(text);
            let (line, column) = text.split_once('C').unwrap_or((text, "1"));
            positive(column)?;
            positive(line)
        }
        let (from, to) = text.split_once('-').unwrap_or((text, text));
        let (first, last) = (line(from)?, line(to)?);
        (first <= last).then_some(Self { first, last })
    }

    pub fn label(self) -> String {
        if self.first == self.last {
            format!("第 {} 行", self.first)
        } else {
            format!("第 {}–{} 行", self.first, self.last)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileReference {
    pub path: String,
    pub lines: Option<LineRange>,
    pub column: Option<usize>,
}

impl FileReference {
    pub fn from_link(href: &str) -> Option<Self> {
        Self::parse(href.strip_prefix(LINK_PREFIX)?, true)
    }

    fn parse(input: &str, decode_destination: bool) -> Option<Self> {
        let uri = input.starts_with("file://");
        let input = if uri {
            // Only file URIs without a remote authority are local file references.
            let local = input.strip_prefix("file://")?;
            let local = local.strip_prefix("localhost").unwrap_or(local);
            if !local.starts_with('/') || local.starts_with("//") {
                return None;
            }
            #[cfg(windows)]
            let local = match local.as_bytes() {
                [b'/', drive, b':', b'/', ..] if drive.is_ascii_alphabetic() => &local[1..],
                _ => local,
            };
            local
        } else {
            input
        };
        if input.is_empty()
            || input.starts_with('#')
            || input.starts_with("//")
            || input.starts_with("\\\\")
        {
            return None;
        }
        let (raw_path, lines, column) = if let Some((path, fragment)) = input.split_once('#') {
            if !fragment.starts_with('L') {
                return None;
            }
            let first = fragment.split('-').next()?;
            let column = first
                .split_once('C')
                .and_then(|(_, column)| positive(column));
            (path, Some(LineRange::parse(fragment)?), column)
        } else {
            split_line_suffix(input)?
        };
        let path = if uri || decode_destination {
            decode_path(raw_path)?
        } else {
            Cow::Borrowed(raw_path)
        };
        let path = path.strip_prefix("./").unwrap_or(&path);
        if path.is_empty() || path.trim() != path || path.chars().any(char::is_control) {
            return None;
        }
        if let Some((drive, rest)) = path.split_once(':')
            && !(drive.len() == 1
                && drive.as_bytes()[0].is_ascii_alphabetic()
                && rest.starts_with(['/', '\\']))
        {
            return None;
        }
        if path.starts_with("//") || path.starts_with("\\\\") {
            return None;
        }
        Some(Self {
            path: path.into(),
            lines,
            column,
        })
    }
}

fn positive(text: &str) -> Option<usize> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok().filter(|value| *value > 0)
}

fn split_line_suffix(input: &str) -> Option<(&str, Option<LineRange>, Option<usize>)> {
    let Some((prefix, suffix)) = input.rsplit_once(':') else {
        return Some((input, None, None));
    };
    if !suffix.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        return Some((input, None, None));
    }
    if let Some((path, line)) = prefix.rsplit_once(':')
        && line.as_bytes().first().is_some_and(u8::is_ascii_digit)
    {
        return Some((path, Some(LineRange::parse(line)?), Some(positive(suffix)?)));
    }
    Some((prefix, Some(LineRange::parse(suffix)?), None))
}

fn decode_path(path: &str) -> Option<Cow<'_, str>> {
    if !path.contains('%') {
        return Some(Cow::Borrowed(path));
    }
    let mut bytes = Vec::with_capacity(path.len());
    let mut input = path.bytes();
    while let Some(byte) = input.next() {
        if byte == b'%' {
            let high = char::from(input.next()?).to_digit(16)?;
            let low = char::from(input.next()?).to_digit(16)?;
            bytes.push((high * 16 + low) as u8);
        } else {
            bytes.push(byte);
        }
    }
    String::from_utf8(bytes).ok().map(Cow::Owned)
}

fn looks_like_path(reference: &FileReference) -> bool {
    let path = reference.path.as_str();
    if path.contains(['<', '>', '=', ';', '"', '`', '{', '}']) {
        return false;
    }
    let extension = Path::new(path).extension().and_then(|ext| ext.to_str());
    reference.lines.is_some() && path.chars().any(char::is_alphabetic)
        || path.contains(['/', '\\'])
        || extension.is_some_and(|ext| ext.bytes().any(|byte| byte.is_ascii_alphabetic()))
        || matches!(
            path,
            "README" | "LICENSE" | "Makefile" | "Dockerfile" | ".bazelrc" | ".gitignore"
        )
}

pub(super) fn transcript_document(text: &str, stream: Option<&mut MarkdownStream>) -> StyledText {
    let parsed = match stream {
        Some(stream) => stream.update(text),
        None => StyledText::from_markdown(text),
    };
    let Ok(document) = parsed else {
        return StyledText::from_plain_text(text);
    };
    document.rewrite_links(
        |link| {
            FileReference::parse(&link.href, true)?;
            Some(format!("{LINK_PREFIX}{}", link.href).into())
        },
        |span| {
            if !span.style.monospace {
                return None;
            }
            let raw = &document.text()[span.start..span.end];
            if !FileReference::parse(raw, false)
                .as_ref()
                .is_some_and(looks_like_path)
            {
                return None;
            }
            // Code spans contain literal % characters. Escape them before using the
            // same link destination decoder as Markdown links.
            let destination = if raw.starts_with("file://") {
                raw.to_owned()
            } else {
                raw.replace('%', "%25")
            };
            Some(format!("{LINK_PREFIX}{destination}").into())
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_file_references_support_line_column_and_range_forms() {
        for (input, path, first, last, column) in [
            ("src/app.ts:42", "src/app.ts", 42, 42, None),
            (
                "b/server/index.js#L10C6",
                "b/server/index.js",
                10,
                10,
                Some(6),
            ),
            ("./src/a.rs#L12-L14", "src/a.rs", 12, 14, None),
            (
                "C:\\repo\\project\\main.rs:12:5",
                "C:\\repo\\project\\main.rs",
                12,
                12,
                Some(5),
            ),
            (
                "file:///work/source%20file.rs#L8",
                "/work/source file.rs",
                8,
                8,
                None,
            ),
        ] {
            let reference = FileReference::from_link(&format!("{LINK_PREFIX}{input}")).unwrap();
            assert_eq!(
                reference,
                FileReference {
                    path: path.into(),
                    lines: Some(LineRange { first, last }),
                    column,
                }
            );
        }
        for invalid in [
            "https://server/src.rs#L1",
            "file://server/src.rs",
            "src.rs:0",
            "src.rs#L6-L2",
            "src.rs%00",
            "//server/file.rs",
        ] {
            assert!(
                FileReference::from_link(&format!("{LINK_PREFIX}{invalid}")).is_none(),
                "{invalid}"
            );
        }
    }

    #[test]
    fn transcript_keeps_explicit_paths_and_promotes_inline_references() {
        let markdown = concat!(
            "看 [配置](src/%E7%95%8C%20%E9%9D%A2.xr#L10-L11)，再改 `src/counter.rs:42`。\n\n",
            "函数 `foo::bar` 和 [网页](https://example.invalid) 保留文本。\n\n",
            "```sh\nsrc/counter.rs:42\n```",
        );
        let document = transcript_document(markdown, None);
        let links = document.links();
        assert_eq!(links.len(), 2);
        assert_eq!(&document.text()[links[0].start..links[0].end], "配置");
        assert_eq!(
            FileReference::from_link(&links[0].href).unwrap().path,
            "src/界 面.xr"
        );
        assert_eq!(
            &document.text()[links[1].start..links[1].end],
            "src/counter.rs:42"
        );
        let bare = transcript_document("[Entrypoint](startup) and `12:34`", None);
        assert_eq!(bare.links().len(), 1);
        assert_eq!(
            FileReference::from_link(&bare.links()[0].href)
                .unwrap()
                .path,
            "startup"
        );
    }

    #[test]
    fn inline_reference_promotion_keeps_explicit_link_boundaries() {
        let document = transcript_document(
            concat!(
                "[`hidden.rs:7`](https://example.invalid) `first.rs:8` ",
                "[配置](src/config.rs:4) `second.rs:11`",
            ),
            None,
        );
        let labels = document
            .links()
            .iter()
            .map(|link| &document.text()[link.start..link.end])
            .collect::<Vec<_>>();
        assert_eq!(labels, ["first.rs:8", "配置", "second.rs:11"]);
    }

    #[test]
    fn streamed_file_references_match_a_complete_parse() {
        let mut stream = MarkdownStream::default();
        let prefix = "**准备** [配置](src/config.rs:4) 和 `main.rs:11`\n\n";
        let document = transcript_document(prefix, Some(&mut stream));
        assert_eq!(document, transcript_document(prefix, None));
        let latest =
            format!("{prefix}[linked `embedded.rs:5`](https://example.invalid) `new.rs:8`");
        assert_eq!(
            transcript_document(&latest, Some(&mut stream)),
            transcript_document(&latest, None)
        );
    }
}
