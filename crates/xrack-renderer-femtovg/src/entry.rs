use crate::{
    TextSystem, color,
    theme::{Palette, PaletteRole as Role},
};
use femtovg::{Canvas, Color, Paint, Path, renderer::WGPURenderer};
use std::{borrow::Cow, ops::Range};
use xrack_layout::{
    PasswordMask, Point, Rect, TextAreaLayout, TextEdit, TextMeasurer, TextPosition,
};
use xrack_runtime::{CheckState, Color as UiColor, Node};

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
                trailing: cursor > 0,
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
        let area = bounds.inset(node.length("padding").unwrap_or(6.0).max(0.0));
        let mask = node.password_masked().then(|| PasswordMask::new(value));
        let display = mask.as_ref().map_or(value, PasswordMask::display);
        let layout = self.single_line_layout(node, display);
        let scroll = editor.map_or(0.0, |edit| {
            let mut caret = edit.position(value);
            if let Some(mask) = &mask {
                caret.offset = mask.display_offset(caret.offset);
            }
            entry_scroll(layout.caret_x(caret), area.width)
        });
        let mut position = layout.hit(point.x - area.x + scroll);
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
                self.single_line_layout(node, &display.content)
                    .caret_x(caret)
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
                self.advance_node(node, text)
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
    theme: Palette,
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
    let size = text_system.font_size(node);
    let height = size * 1.4;
    let y = bounds.y + ((bounds.height - height) * 0.5).max(0.0);
    let layout = text_system.single_line_layout(node, &content);
    let advance = caret.map(|caret| {
        if placeholder {
            0.0
        } else {
            layout.caret_x(caret)
        }
    });
    let x = area.x - advance.map_or(0.0, |advance| entry_scroll(advance, area.width));
    canvas.intersect_scissor(area.x, bounds.y, area.width, bounds.height);
    let mut rect = |x: f32, y: f32, width: f32, height: f32, tint: Color| {
        let mut path = Path::new();
        path.rect(x, y, width, height);
        canvas.fill_path(&path, &Paint::color(tint));
    };
    for span in layout.selection_spans(selection) {
        rect(
            x + span.start,
            y,
            span.end - span.start,
            height,
            color(theme.role(Role::TextSelection)),
        );
    }
    for span in layout.selection_spans(composition) {
        rect(
            x + span.start,
            y + height - 2.0,
            span.end - span.start,
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
    let paint = text_system.paint_node(node, color(foreground), false);
    canvas
        .fill_text(x, y, &content, &paint)
        .map_err(|error| format!("text input drawing failed: {error}"))?;
    if let Some((suffix, clip)) = text.completion {
        let start = x + layout.width() + 2.0;
        let clip = clip.intersect(Rect::new(area.x, bounds.y, area.width, bounds.height));
        let width = text_system.advance_node(node, suffix);
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
                    &text_system.paint_node(node, color(theme.disabled_text), false),
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
    theme: Palette,
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
    let size = text_system.font_size(node);
    let height = layout.line_height;
    let x = area.x - scroll.x;
    let advance = |text: &str| text_system.advance_node(node, text);
    let foreground = if display.placeholder || !enabled {
        theme.disabled_text
    } else {
        node.color("color").unwrap_or(theme.text)
    };
    let paint = text_system.paint_node(node, color(foreground), false);
    canvas.intersect_scissor(area.x, area.y, area.width, area.height);
    for index in layout.visible_lines(scroll) {
        let line = &layout.lines[index];
        let visual = layout
            .visual_line(index)
            .expect("text system shapes each line");
        let y = area.y + index as f32 * height - scroll.y;
        let local = |span: &Range<usize>| {
            span.start.saturating_sub(line.start)..span.end.saturating_sub(line.start)
        };
        if !display.selection.is_empty() {
            for span in visual.selection_spans(local(&display.selection)) {
                fill_rect(
                    canvas,
                    Rect::new(x + span.start, y, span.end - span.start, height),
                    color(theme.role(Role::TextSelection)),
                );
            }
            let newline = display.selection.start <= line.end
                && display.selection.end > line.end
                && layout
                    .lines
                    .get(index + 1)
                    .is_some_and(|next| next.start > line.end);
            if newline {
                let edge = visual.caret_x(TextPosition {
                    offset: line.len(),
                    trailing: true,
                });
                let width = size * 0.45;
                let left = if visual.is_rtl() { edge - width } else { edge };
                fill_rect(
                    canvas,
                    Rect::new(x + left, y, width, height),
                    color(theme.role(Role::TextSelection)),
                );
            }
        }
        if !line.is_empty() {
            canvas
                .fill_text(x, y, &display.content[line.clone()], &paint)
                .map_err(|error| format!("multiline text drawing failed: {error}"))?;
        }
        for span in visual.selection_spans(local(&display.composition)) {
            fill_rect(
                canvas,
                Rect::new(x + span.start, y + height - 2.0, span.end - span.start, 1.0),
                color(theme.focus),
            );
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
            color(theme.role(Role::Grip)),
        );
    }
    Ok(())
}

pub(crate) fn toggle(
    canvas: &mut Canvas<WGPURenderer>,
    text_system: &TextSystem,
    theme: Palette,
    node: &Node,
    bounds: Rect,
    enabled: bool,
) -> Result<(), String> {
    let box_size = 18.0_f32.min(bounds.height);
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
        theme.role(Role::Control)
    };
    let width = if node.kind == "Switch" {
        box_size * 2.0
    } else {
        box_size
    };
    let item_delegate = node.is_item_delegate();
    let x = if item_delegate {
        bounds.x + (bounds.width - width - 12.0).max(0.0)
    } else {
        bounds.x + 6.0
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
                if checked {
                    theme.on_accent(theme.text)
                } else {
                    theme.text
                }
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
        let foreground = if !enabled && theme.is_light() {
            theme.disabled_text
        } else {
            theme.on_accent(UiColor(0xffffffff))
        };
        let mut paint = Paint::color(color(foreground));
        paint.set_line_width(2.0);
        canvas.stroke_path(&mark, &paint);
    }
    let size = text_system.font_size(node);
    let tint = if enabled {
        node.color("color").unwrap_or(theme.text)
    } else {
        theme.disabled_text
    };
    if item_delegate {
        return crate::controls::label(
            canvas,
            text_system,
            node,
            Rect::new(
                bounds.x + 12.0,
                bounds.y,
                (x - bounds.x - 22.0).max(0.0),
                bounds.height,
            ),
            node.text("text").unwrap_or(""),
            size,
            tint,
        );
    }
    let paint = text_system.paint_node(node, color(tint), false);
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
