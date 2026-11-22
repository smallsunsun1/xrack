use xrack_runtime::{Color, StyledRun, StyledText, TextStyle};

pub(super) const CURSOR_MARK: char = '▏';

#[derive(Clone, Copy, Default)]
pub(super) struct Keyboard {
    pub(super) application_cursor: bool,
    pub(super) bracketed_paste: bool,
    pub(super) rows: u16,
}

#[derive(Clone)]
pub(super) struct Frame {
    pub(super) screen: StyledText,
    pub(super) keyboard: Keyboard,
    pub(super) status: String,
    pub(super) running: bool,
    pub(super) history: bool,
    pub(super) cursor_byte: Option<usize>,
}

impl Frame {
    pub(super) fn status(status: &str) -> Self {
        Self {
            screen: StyledText::from_plain_text(""),
            keyboard: Keyboard::default(),
            status: status.into(),
            running: false,
            history: false,
            cursor_byte: None,
        }
    }

    pub(super) fn finish(&mut self, status: String) {
        self.running = false;
        self.status = status;
    }

    pub(super) fn read(screen: &vt100::Screen) -> Self {
        let (rows, columns) = screen.size();
        let history = screen.scrollback() > 0;
        let cursor = (!history && !screen.hide_cursor()).then(|| screen.cursor_position());
        let mut text = String::new();
        let mut cursor_byte = None;
        let mut runs: Vec<StyledRun> = Vec::new();
        for row in 0..rows.get() {
            if row > 0 {
                text.push('\n');
            }
            let last = (0..columns.get()).rev().find(|&column| {
                cursor == Some((row, column))
                    || screen
                        .cell(row, column)
                        .is_some_and(|cell| !cell.contents().is_empty())
            });
            let Some(last) = last else { continue };
            for column in 0..=last {
                let Some(cell) = screen.cell(row, column) else {
                    break;
                };
                if cell.is_wide_continuation() {
                    continue;
                }
                let active = cursor == Some((row, column));
                let color = if cell.inverse() {
                    cell.bgcolor()
                } else {
                    cell.fgcolor()
                };
                let style = TextStyle {
                    bold: cell.bold(),
                    italic: cell.italic(),
                    underline: cell.underline() || (active && !cell.contents().is_empty()),
                    color: palette(color),
                    ..TextStyle::default()
                };
                let start = text.len();
                if cell.contents().is_empty() {
                    if active {
                        cursor_byte = Some(start);
                        text.push(CURSOR_MARK);
                    } else {
                        text.push(' ');
                    }
                } else {
                    text.push_str(cell.contents());
                }
                if style != TextStyle::default() {
                    if let Some(run) = runs.last_mut()
                        && run.end == start
                        && run.style == style
                    {
                        run.end = text.len();
                    } else {
                        runs.push(StyledRun {
                            start,
                            end: text.len(),
                            style,
                        });
                    }
                }
            }
        }
        let document = StyledText::from_runs(text.as_str(), runs)
            .unwrap_or_else(|_| StyledText::from_plain_text(text));
        Self {
            screen: document,
            keyboard: Keyboard {
                application_cursor: screen.application_cursor(),
                bracketed_paste: screen.bracketed_paste(),
                rows: rows.get(),
            },
            status: if history {
                "历史输出"
            } else {
                "本机终端"
            }
            .into(),
            running: true,
            history,
            cursor_byte,
        }
    }
}

fn palette(color: vt100::Color) -> Option<Color> {
    const BASIC: [u32; 16] = [
        0x929bad, 0xef7c8b, 0x9bd49c, 0xeacf89, 0x9bb9ff, 0xdca7f4, 0x8bd4db, 0xd3dbed, 0xa9b1c0,
        0xff9caa, 0xb5ebaf, 0xffe7a3, 0xb7ccff, 0xeabbff, 0x9ce8ed, 0xfafcff,
    ];
    let rgb = match color {
        vt100::Color::Default => return None,
        vt100::Color::Rgb(r, g, b) => u32::from_be_bytes([0, r, g, b]),
        vt100::Color::Idx(index @ 0..=15) => BASIC[usize::from(index)],
        vt100::Color::Idx(index @ 16..=231) => {
            let cube = index - 16;
            let channel = |value| if value == 0 { 0 } else { 55 + 40 * value };
            u32::from_be_bytes([
                0,
                channel(cube / 36),
                channel(cube / 6 % 6),
                channel(cube % 6),
            ])
        }
        vt100::Color::Idx(index) => {
            let level = 8 + (index - 232) * 10;
            u32::from_be_bytes([0, level, level, level])
        }
    };
    Some(Color((rgb << 8) | 0xff))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU16;

    #[test]
    fn paints_ansi_text_after_cursor_updates_and_uses_bounded_scrollback() {
        let mut parser = vt100::Parser::new(
            NonZeroU16::new(4).unwrap(),
            NonZeroU16::new(24).unwrap(),
            32,
        );
        parser.process(b"old\r\x1b[31mnew\x1b[m\x1b[?1h\x1b[?2004h");
        let frame = Frame::read(parser.screen());
        assert!(frame.screen.text().starts_with("new"));
        assert!(
            frame
                .screen
                .runs()
                .iter()
                .any(|run| run.style.color == palette(vt100::Color::Idx(1)))
        );
        assert!(frame.keyboard.application_cursor && frame.keyboard.bracketed_paste);
        parser.process(b"\r\nline 2\r\nline 3\r\nline 4\r\nline 5");
        parser.screen_mut().set_scrollback(1);
        let history = Frame::read(parser.screen());
        assert!(history.history);
        assert!(history.screen.text().starts_with("new"));
    }
}
