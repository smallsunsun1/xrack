use crate::{TextSystem, Theme, color};
use femtovg::{Canvas, Color, Paint, Path, renderer::WGPURenderer};
use std::{borrow::Cow, ops::Range};
use xrack_layout::{PasswordMask, Point, Rect, TextAreaLayout, TextEdit, TextPosition, font_size};
use xrack_runtime::{CheckState, Node, TextWrap};

pub(crate) struct EntryText<'a> {
    pub value: &'a str,
    pub editor: Option<&'a TextEdit>,
    pub scroll: Point,
    pub completion: Option<(&'a str, Rect)>,
}

struct DisplayText<'a> {
    content: Cow<'a, str>,
    selection: Range<usize>,
    composition: Range<usize>,
    caret: Option<TextPosition>,
    placeholder: bool,
}

fn display_text<'a>(node: &'a Node, value: &'a str, editor: Option<&TextEdit>) -> DisplayText<'a> {
    let mut content = Cow::Borrowed(value);
    let mut selection = 0..0;
    let mut composition = 0..0;
    let mut caret = None;
    if let Some(edit) = editor {
        selection = edit.selection(value);
        if edit.preedit.is_empty() {
            caret = Some(edit.position(value));
        } else {
            content
                .to_mut()
                .replace_range(selection.clone(), &edit.preedit);
            composition = selection.start..selection.start + edit.preedit.len();
            caret = edit.preedit_cursor.map(|cursor| TextPosition {
                offset: composition.start + cursor,
                trailing: false,
            });
            selection = 0..0;
        }
    }
    let placeholder = content.is_empty();
    if placeholder {
        content = node.text("placeholder-text").unwrap_or("").into();
    } else if node.password_masked() {
        let mask = PasswordMask::new(&content);
        selection = mask.display_offset(selection.start)..mask.display_offset(selection.end);
        composition = mask.display_offset(composition.start)..mask.display_offset(composition.end);
        if let Some(caret) = &mut caret {
            caret.offset = mask.display_offset(caret.offset);
        }
        content = mask.into_display().into();
    }
    DisplayText {
        content,
        selection,
        composition,
        caret,
        placeholder,
    }
}

impl TextSystem {
    pub fn single_line_hit(
        &self,
        node: &Node,
        bounds: Rect,
        point: Point,
        editor: Option<&TextEdit>,
    ) -> TextPosition {
        self.single_line_hit_with_text(node, bounds, point, editor, node.text("text").unwrap_or(""))
    }

    pub fn single_line_hit_with_text(
        &self,
        node: &Node,
        bounds: Rect,
        point: Point,
        editor: Option<&TextEdit>,
        value: &str,
    ) -> TextPosition {
        let size = font_size(node);
        let area = bounds.inset(node.length("padding").unwrap_or(6.0).max(0.0));
        let advance = |text: &str| self.advance(text, size);
        let mask = node.password_masked().then(|| PasswordMask::new(value));
        let display = mask.as_ref().map_or(value, PasswordMask::display);
        let scroll = editor.map_or(0.0, |edit| {
            let caret = edit.caret(value);
            let offset = mask
                .as_ref()
                .map_or(caret, |mask| mask.display_offset(caret));
            entry_scroll(advance(&display[..offset]), area.width)
        });
        let layout = TextAreaLayout::new(
            display,
            area,
            size * 1.4,
            TextWrap::NoWrap,
            advance,
            str::len,
        );
        let mut position = layout.hit(
            display,
            Point {
                x: point.x,
                y: area.y,
            },
            Point { x: scroll, y: 0.0 },
            advance,
        );
        if let Some(mask) = &mask {
            position.offset = mask.source_offset(position.offset);
        }
        position
    }

    pub fn single_line_caret_x(
        &self,
        node: &Node,
        bounds: Rect,
        value: &str,
        editor: Option<&TextEdit>,
    ) -> f32 {
        let display = display_text(node, value, editor);
        let area = bounds.inset(node.length("padding").unwrap_or(6.0).max(0.0));
        let advance = display
            .caret
            .filter(|_| !display.placeholder)
            .map_or(0.0, |caret| {
                self.advance(&display.content[..caret.offset], font_size(node))
            });
        area.x + (advance - entry_scroll(advance, area.width)).clamp(0.0, area.width)
    }

    pub fn text_area_caret(
        &self,
        node: &Node,
        bounds: Rect,
        editor: Option<&TextEdit>,
    ) -> (TextAreaLayout, Option<Point>) {
        let display = display_text(node, node.text("text").unwrap_or(""), editor);
        let layout = self.text_area_layout(node, bounds, &display.content);
        let caret = display.caret.map(|position| {
            layout.caret_point(&display.content, position, |text| {
                self.advance(text, font_size(node))
            })
        });
        (layout, caret)
    }
}

fn entry_scroll(advance: f32, width: f32) -> f32 {
    (advance + 1.0 - width).max(0.0)
}

pub(crate) fn draw(
    canvas: &mut Canvas<WGPURenderer>,
    text_system: &TextSystem,
    theme: Theme,
    node: &Node,
    bounds: Rect,
    text: EntryText<'_>,
    enabled: bool,
) -> Result<bool, String> {
    let DisplayText {
        content,
        selection,
        composition,
        caret,
        placeholder,
    } = display_text(node, text.value, text.editor);
    let area = bounds.inset(node.length("padding").unwrap_or(6.0).max(0.0));
    if area.width <= 0.0 || area.height <= 0.0 {
        return Ok(false);
    }
    let size = font_size(node);
    let height = size * 1.4;
    let y = bounds.y + ((bounds.height - height) * 0.5).max(0.0);
    let width_at = |offset: usize| {
        if offset == 0 || placeholder {
            0.0
        } else {
            text_system.advance(&content[..offset], size)
        }
    };
    let advance = caret.map(|caret| width_at(caret.offset));
    let x = area.x - advance.map_or(0.0, |advance| entry_scroll(advance, area.width));
    canvas.intersect_scissor(area.x, bounds.y, area.width, bounds.height);
    let mut rect = |x: f32, y: f32, width: f32, height: f32, tint: Color| {
        let mut path = Path::new();
        path.rect(x, y, width, height);
        canvas.fill_path(&path, &Paint::color(tint));
    };
    if !selection.is_empty() {
        let start = width_at(selection.start);
        rect(
            x + start,
            y,
            (width_at(selection.end) - start).max(0.0),
            height,
            Color::rgba(78, 113, 192, 155),
        );
    }
    if !composition.is_empty() {
        let start = width_at(composition.start);
        rect(
            x + start,
            y + height - 2.0,
            (width_at(composition.end) - start).max(0.0),
            1.0,
            color(theme.focus),
        );
    }
    if let Some(advance) = advance {
        rect(
            x + advance,
            y + 1.0,
            1.0,
            (height - 2.0).max(0.0),
            color(theme.focus),
        );
    }
    let foreground = if placeholder || !enabled {
        theme.disabled_text
    } else {
        node.color("color").unwrap_or(theme.text)
    };
    let paint = text_system.paint(size, color(foreground), false);
    canvas
        .fill_text(x, y, &content, &paint)
        .map_err(|error| format!("text input drawing failed: {error}"))?;
    if let Some((suffix, clip)) = text.completion {
        let start = x + width_at(content.len()) + 2.0;
        let clip = clip.intersect(Rect::new(area.x, bounds.y, area.width, bounds.height));
        let width = text_system.advance(suffix, size);
        if width > 0.0
            && clip.contains(Point {
                x: start,
                y: y + height * 0.5,
            })
            && clip.x + clip.width - start >= width.min(8.0)
        {
            canvas
                .fill_text(
                    start,
                    y,
                    suffix,
                    &text_system.paint(size, color(theme.disabled_text), false),
                )
                .map_err(|error| format!("completion drawing failed: {error}"))?;
            return Ok(true);
        }
    }
    Ok(false)
}

fn fill_rect(canvas: &mut Canvas<WGPURenderer>, rect: Rect, tint: Color) {
    let mut path = Path::new();
    path.rect(rect.x, rect.y, rect.width, rect.height);
    canvas.fill_path(&path, &Paint::color(tint));
}

pub(crate) fn draw_area(
    canvas: &mut Canvas<WGPURenderer>,
    text_system: &TextSystem,
    theme: Theme,
    node: &Node,
    bounds: Rect,
    text: EntryText<'_>,
    enabled: bool,
) -> Result<(), String> {
    let display = display_text(node, text.value, text.editor);
    let layout = text_system.text_area_layout(node, bounds, &display.content);
    let area = layout.area;
    if area.width <= 0.0 || area.height <= 0.0 {
        return Ok(());
    }
    let scroll = layout.clamp_scroll(text.scroll);
    let size = font_size(node);
    let height = layout.line_height;
    let x = area.x - scroll.x;
    let advance = |text: &str| text_system.advance(text, size);
    let foreground = if display.placeholder || !enabled {
        theme.disabled_text
    } else {
        node.color("color").unwrap_or(theme.text)
    };
    let paint = text_system.paint(size, color(foreground), false);
    canvas.intersect_scissor(area.x, area.y, area.width, area.height);
    for index in layout.visible_lines(scroll) {
        let line = &layout.lines[index];
        let y = area.y + index as f32 * height - scroll.y;
        let decoration = |span: &Range<usize>| {
            let start = span.start.max(line.start).min(line.end);
            let end = span.end.max(line.start).min(line.end);
            let left = advance(&display.content[line.start..start]);
            let right = advance(&display.content[line.start..end]);
            (x + left, (right - left).max(0.0))
        };
        if !display.selection.is_empty() {
            let (left, width) = decoration(&display.selection);
            let newline = display.selection.start <= line.end
                && display.selection.end > line.end
                && layout
                    .lines
                    .get(index + 1)
                    .is_some_and(|next| next.start > line.end);
            if width > 0.0 || newline {
                fill_rect(
                    canvas,
                    Rect::new(
                        left,
                        y,
                        width + if newline { size * 0.45 } else { 0.0 },
                        height,
                    ),
                    Color::rgba(78, 113, 192, 155),
                );
            }
        }
        if !line.is_empty() {
            canvas
                .fill_text(x, y, &display.content[line.clone()], &paint)
                .map_err(|error| format!("multiline text drawing failed: {error}"))?;
        }
        if !display.composition.is_empty() {
            let (left, width) = decoration(&display.composition);
            if width > 0.0 {
                fill_rect(
                    canvas,
                    Rect::new(left, y + height - 2.0, width, 1.0),
                    color(theme.focus),
                );
            }
        }
    }
    if let Some(position) = display.caret {
        let caret = layout.caret_point(&display.content, position, advance);
        fill_rect(
            canvas,
            Rect::new(
                x + caret.x,
                area.y + caret.y - scroll.y + 1.0,
                1.0,
                height - 2.0,
            ),
            color(theme.focus),
        );
    }
    if layout.max_scroll.y > 0.0 && area.width > 6.0 {
        let thumb = (area.height * area.height / (area.height + layout.max_scroll.y))
            .max(12.0)
            .min(area.height);
        let y = area.y + (area.height - thumb) * scroll.y / layout.max_scroll.y;
        fill_rect(
            canvas,
            Rect::new(area.x + area.width - 2.0, y, 2.0, thumb),
            Color::rgba(155, 180, 227, 160),
        );
    }
    Ok(())
}

pub(crate) fn toggle(
    canvas: &mut Canvas<WGPURenderer>,
    text_system: &TextSystem,
    theme: Theme,
    node: &Node,
    bounds: Rect,
    enabled: bool,
) -> Result<(), String> {
    let box_size = 18.0_f32.min(bounds.height);
    let x = bounds.x + 6.0;
    let y = bounds.y + (bounds.height - box_size) * 0.5;
    let state = if node.kind == "GroupBox" {
        node.boolean("checked", true).into()
    } else {
        node.check_state()
    };
    let checked = state == CheckState::Checked;
    let background = if !enabled {
        theme.disabled
    } else if state != CheckState::Unchecked {
        theme.button
    } else {
        xrack_runtime::Color(0x384761ff)
    };
    let width = if node.kind == "Switch" {
        box_size * 2.0
    } else {
        box_size
    };
    let mut frame = Path::new();
    frame.rounded_rect(
        x,
        y,
        width,
        box_size,
        if node.kind == "Switch" {
            box_size * 0.5
        } else {
            4.0
        },
    );
    canvas.fill_path(&frame, &Paint::color(color(background)));
    if node.kind == "Switch" {
        let radius = (box_size * 0.5 - 2.0).max(0.0);
        let offset = if checked {
            width - box_size * 0.5
        } else {
            box_size * 0.5
        };
        let mut knob = Path::new();
        knob.circle(x + offset, y + box_size * 0.5, radius);
        canvas.fill_path(
            &knob,
            &Paint::color(color(if enabled {
                theme.text
            } else {
                theme.disabled_text
            })),
        );
    } else if state != CheckState::Unchecked {
        let mut mark = Path::new();
        mark.move_to(x + box_size * 0.20, y + box_size * 0.50);
        if checked {
            mark.line_to(x + box_size * 0.43, y + box_size * 0.72);
            mark.line_to(x + box_size * 0.80, y + box_size * 0.28);
        } else {
            mark.line_to(x + box_size * 0.80, y + box_size * 0.50);
        }
        let mut paint = Paint::color(Color::white());
        paint.set_line_width(2.0);
        canvas.stroke_path(&mark, &paint);
    }
    let size = font_size(node);
    let color = color(if enabled {
        node.color("color").unwrap_or(theme.text)
    } else {
        theme.disabled_text
    });
    let paint = text_system.paint(size, color, false);
    canvas
        .fill_text(
            x + width + 7.0,
            bounds.y + ((bounds.height - size * 1.4) * 0.5).max(0.0),
            node.text(if node.kind == "GroupBox" {
                "title"
            } else {
                "text"
            })
            .unwrap_or(""),
            &paint,
        )
        .map_err(|error| format!("checkbox label drawing failed: {error}"))?;
    Ok(())
}
