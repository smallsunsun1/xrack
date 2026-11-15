use crate::{
    TextSystem, color,
    theme::{Palette, PaletteRole as Role},
};
use femtovg::{Canvas, Color, Paint, Path, renderer::WGPURenderer};
use std::{borrow::Cow, ops::Range};
use xrack_layout::{
    ComboGeometry, Interaction, LayoutNode, PasswordMask, Point, Rect, SearchGeometry,
    SpinGeometry, Target, TextAlignment, TextAreaLayout, TextAreaMotion, TextEdit, TextMeasurer,
    TextPosition, ToggleGeometry, VisualTextLine,
};
use xrack_runtime::{
    CheckState, Color as UiColor, LayoutDirection, Node, combo_editable, is_multiline_text_input,
    is_spin_box, spin_editable, text_input_value,
};

fn editor_text<'a>(input: &Interaction, node: &'a Node) -> Cow<'a, str> {
    if is_spin_box(node.kind) || matches!(node.kind, "TableCell" | "TreeCell") {
        Cow::Owned(input.editor_value(node).into_owned())
    } else {
        Cow::Borrowed(text_input_value(node))
    }
}

fn editor_part(node: &Node, layout: &LayoutNode) -> Option<Rect> {
    if is_spin_box(node.kind) {
        Some(SpinGeometry::for_layout(layout, node).text)
    } else if combo_editable(node) {
        Some(ComboGeometry::for_layout(layout).entry)
    } else if node.kind == "SearchField" {
        Some(SearchGeometry::for_layout(layout).entry)
    } else {
        None
    }
}

/// A pointer hit bound to the editor that owned the press.
#[doc(hidden)]
pub struct PointerCaret {
    target: Target,
    origin: Point,
    point: Point,
    selecting: bool,
}

impl PointerCaret {
    pub fn new(
        input: &Interaction,
        scene: &Node,
        layout: &LayoutNode,
        target: &Target,
        origin: Point,
        point: Point,
        selecting: bool,
    ) -> Option<Self> {
        let pointer = Self {
            target: target.clone(),
            origin,
            point,
            selecting,
        };
        pointer.hit(input, scene, layout)?;
        Some(pointer)
    }

    pub fn from_pressed(
        input: &Interaction,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
        selecting: bool,
    ) -> Option<Self> {
        let target = input
            .focused
            .as_ref()
            .filter(|target| input.pressed.as_ref() == Some(target))?;
        let origin = input.pointer_press_origin().unwrap_or(point);
        Self::new(input, scene, layout, target, origin, point, selecting)
    }

    fn hit<'a>(
        &self,
        input: &Interaction,
        scene: &'a Node,
        layout: &LayoutNode,
    ) -> Option<(&'a Node, Rect, Point, Option<LayoutDirection>)> {
        let path = &self.target.path;
        let node = scene.interactive_at_path(path)?;
        if node.element_id() != self.target.element_id()
            || scene.row_key_at(path) != self.target.row_key
            || !(input.is_text_editor(node, &self.target) || spin_editable(node))
        {
            return None;
        }
        let item = layout
            .at_path(path)
            .filter(|item| item.visible && item.enabled)?;
        let point = item.map_from_window(self.point)?;
        let part = editor_part(node, item);
        if let Some(bounds) = part {
            let origin = item.map_from_window(self.origin)?;
            if !bounds.contains(origin) {
                return None;
            }
        }
        Some((
            node,
            part.unwrap_or(item.bounds),
            point,
            item.layout_direction_override(),
        ))
    }

    pub fn place(
        self,
        text_system: &TextSystem,
        input: &mut Interaction,
        scene: &Node,
        layout: &LayoutNode,
    ) -> bool {
        if input.focused.as_ref() != Some(&self.target) {
            return false;
        }
        let Some((node, bounds, point, direction)) = self.hit(input, scene, layout) else {
            return false;
        };
        input.sync_editor(scene);
        let value = editor_text(input, node);
        let (position, reveal) = if is_multiline_text_input(node.kind) {
            let geometry =
                text_system.text_area_layout_in_direction(node, bounds, &value, direction);
            let scroll = geometry.clamp_scroll(input.text_scroll(&self.target.path));
            let advance = |text: &str| text_system.advance_node(node, text);
            let position = geometry.hit(&value, point, scroll, advance);
            let caret = geometry.caret_point(&value, position, advance);
            (position, Some(geometry.keep_visible(scroll, caret)))
        } else {
            let hint = input
                .painted_combo_completion_for(node, &self.target.path)
                .and_then(|completion| {
                    let item = layout.at_path(&self.target.path)?;
                    Some((completion.suffix(), item.clip))
                });
            (
                text_system.single_line_hit_in_direction(
                    node,
                    bounds,
                    point,
                    input.editor.as_ref(),
                    &value,
                    (direction, hint),
                ),
                None,
            )
        };
        input.break_text_edit_group();
        let Some(editor) = &mut input.editor else {
            return false;
        };
        editor.place_caret(&value, position, self.selecting);
        if let Some(scroll) = reveal {
            input.set_text_scroll(&self.target, scroll);
        }
        true
    }
}

pub(crate) struct EntryText<'a> {
    pub value: &'a str,
    pub editor: Option<&'a TextEdit>,
    pub scroll: Point,
    pub direction: Option<LayoutDirection>,
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
    /// Move the focused editor through its shaped visual caret positions.
    #[doc(hidden)]
    pub fn move_editor_caret(
        &self,
        input: &mut Interaction,
        scene: &Node,
        layout: &LayoutNode,
        motion: TextAreaMotion,
        selecting: bool,
    ) -> bool {
        let Some(target) = input.focused.clone() else {
            return false;
        };
        let Some(node) = scene.interactive_at_path(&target.path) else {
            return false;
        };
        if node.password_masked()
            || !input.is_text_editor(node, &target)
            || node.element_id() != target.element_id()
            || scene.row_key_at(&target.path) != target.row_key
        {
            return false;
        }
        let Some(item) = layout
            .at_path(&target.path)
            .filter(|item| item.visible && item.enabled)
        else {
            return false;
        };
        let value = editor_text(input, node);
        let scroll = input.text_scroll(&target.path);
        let Some(editor) = &mut input.editor else {
            return false;
        };
        if is_multiline_text_input(node.kind) {
            let geometry = self.text_area_layout_in_direction(
                node,
                item.bounds,
                &value,
                item.layout_direction_override(),
            );
            let advance = |text: &str| self.advance_node(node, text);
            geometry.move_caret(&value, editor, motion, selecting, advance);
            let caret = geometry.caret_point(&value, editor.position(&value), advance);
            input.set_text_scroll(&target, geometry.keep_visible(scroll, caret));
        } else {
            let visual = self.single_line_layout(node, &value);
            let position = match motion {
                TextAreaMotion::Left | TextAreaMotion::Right => {
                    let right = matches!(motion, TextAreaMotion::Right);
                    let edge = (!selecting)
                        .then(|| visual.selection_edge(editor.selection(&value), right))
                        .flatten();
                    edge.or_else(|| visual.step(editor.position(&value), right))
                }
                TextAreaMotion::Home | TextAreaMotion::End => {
                    Some(visual.edge(matches!(motion, TextAreaMotion::End) != visual.is_rtl()))
                }
                _ => return false,
            };
            if let Some(position) = position {
                editor.place_caret(&value, position, selecting);
            }
        }
        input.break_text_edit_group();
        true
    }

    /// Scroll the multiline editor under the pointer and return unconsumed motion.
    #[doc(hidden)]
    pub fn scroll_editor_area(
        &self,
        input: &mut Interaction,
        scene: &Node,
        layout: &LayoutNode,
        delta: Point,
    ) -> Point {
        if !delta.x.is_finite() || !delta.y.is_finite() {
            return delta;
        }
        let Some(target) = input.pointer_hover_target(scene, layout) else {
            return delta;
        };
        let Some(node) = scene
            .at_path(&target.path)
            .filter(|node| is_multiline_text_input(node.kind))
        else {
            return delta;
        };
        let Some(item) = layout
            .at_path(&target.path)
            .filter(|item| item.visible && item.enabled)
        else {
            return delta;
        };
        let editor = input
            .editor
            .as_ref()
            .filter(|_| input.focused.as_ref() == Some(&target));
        let (geometry, _) = self.text_area_caret_in_direction(
            node,
            item.bounds,
            editor,
            item.layout_direction_override(),
        );
        let old = geometry.clamp_scroll(input.text_scroll(&target.path));
        let scroll = geometry.clamp_scroll(Point {
            x: old.x + delta.x,
            y: old.y + delta.y,
        });
        if scroll == old {
            return delta;
        }
        input.set_text_scroll(&target, scroll);
        let remaining =
            |delta: f32, used: f32| (delta - used).clamp(delta.min(0.0), delta.max(0.0));
        Point {
            x: remaining(delta.x, scroll.x - old.x),
            y: remaining(delta.y, scroll.y - old.y),
        }
    }

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
        self.single_line_hit_in_direction(
            node,
            bounds,
            point,
            editor,
            value,
            (node.layout_direction(), None),
        )
    }

    fn single_line_hit_in_direction(
        &self,
        node: &Node,
        bounds: Rect,
        point: Point,
        editor: Option<&TextEdit>,
        value: &str,
        (direction, completion): (Option<LayoutDirection>, Option<(&str, Rect)>),
    ) -> TextPosition {
        let area = bounds.inset(node.length("padding").unwrap_or(6.0).max(0.0));
        let mask = node.password_masked().then(|| PasswordMask::new(value));
        let display = mask.as_ref().map_or(value, PasswordMask::display);
        let layout = self.single_line_layout(node, display);
        let caret = editor.map(|edit| {
            let mut caret = edit.position(value);
            if let Some(mask) = &mask {
                caret.offset = mask.display_offset(caret.offset);
            }
            caret
        });
        let (mut x, _) = entry_origin(node, area, &layout, caret, false, direction);
        if let Some((draft, _)) = completion
            .and_then(|hint| self.inline_completion_positions(node, area, &layout, x, hint))
        {
            x = draft;
        }
        let mut position = layout.hit(point.x - x);
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
        self.single_line_caret_in_direction(
            node,
            bounds,
            value,
            editor,
            (node.layout_direction(), None),
        )
    }

    fn single_line_caret_in_direction(
        &self,
        node: &Node,
        bounds: Rect,
        value: &str,
        editor: Option<&TextEdit>,
        (direction, completion): (Option<LayoutDirection>, Option<(&str, Rect)>),
    ) -> f32 {
        let display = display_text(node, value, editor);
        let area = bounds.inset(node.length("padding").unwrap_or(6.0).max(0.0));
        let layout = self.single_line_layout(node, &display.content);
        let (x, caret) = entry_origin(
            node,
            area,
            &layout,
            display.caret,
            display.placeholder,
            direction,
        );
        let shift = completion
            .and_then(|hint| self.inline_completion_positions(node, area, &layout, x, hint))
            .map_or(0.0, |(draft, _)| draft - x);
        (caret.unwrap_or(x) + shift).clamp(area.x, area.x + area.width)
    }

    /// Keep the suffix on the draft's visual continuation side.
    #[doc(hidden)]
    pub fn inline_completion_positions(
        &self,
        node: &Node,
        area: Rect,
        draft: &VisualTextLine,
        aligned_x: f32,
        (suffix, clip): (&str, Rect),
    ) -> Option<(f32, f32)> {
        let visible = area.intersect(clip);
        let gap = 2.0;
        let suffix_width = self.advance_node(node, suffix);
        let capacity = visible.width - draft.width() - gap - 1.0;
        if visible.height <= 0.0 || suffix_width <= 0.0 || capacity < suffix_width.min(8.0) {
            return None;
        }
        let reserve = suffix_width.min(capacity) + gap;
        let end = visible.x + visible.width - draft.width() - 1.0;
        Some(if draft.is_rtl() {
            let draft_x = aligned_x.clamp(visible.x + reserve, end.max(visible.x + reserve));
            (draft_x, draft_x - suffix_width - gap)
        } else {
            let draft_x = aligned_x.clamp(visible.x, (end - reserve).max(visible.x));
            (draft_x, draft_x + draft.width() + gap)
        })
    }

    pub fn text_area_caret(
        &self,
        node: &Node,
        bounds: Rect,
        editor: Option<&TextEdit>,
    ) -> (TextAreaLayout, Option<Point>) {
        self.text_area_caret_in_direction(node, bounds, editor, node.layout_direction())
    }

    fn text_area_caret_in_direction(
        &self,
        node: &Node,
        bounds: Rect,
        editor: Option<&TextEdit>,
        direction: Option<LayoutDirection>,
    ) -> (TextAreaLayout, Option<Point>) {
        let display = display_text(node, node.text("text").unwrap_or(""), editor);
        let layout = self.text_area_layout_in_direction(node, bounds, &display.content, direction);
        let caret = display.caret.map(|position| {
            layout.caret_point(&display.content, position, |text| {
                self.advance_node(node, text)
            })
        });
        (layout, caret)
    }

    /// Return the editor caret in window coordinates for the platform input method.
    pub fn editor_ime_area(
        &self,
        input: &mut Interaction,
        scene: &Node,
        layout: &LayoutNode,
        follow_caret: bool,
    ) -> Option<Rect> {
        let target = input.focused.as_ref()?;
        let node = scene.interactive_at_path(&target.path)?;
        if !input.is_text_editor(node, target) && !spin_editable(node) {
            return None;
        }
        let layout = layout.at_path(&target.path)?;
        let bounds = editor_part(node, layout).unwrap_or(layout.bounds);
        if is_multiline_text_input(node.kind) {
            let (geometry, caret) = self.text_area_caret_in_direction(
                node,
                bounds,
                input.editor.as_ref(),
                layout.layout_direction_override(),
            );
            let mut scroll = geometry.clamp_scroll(input.text_scroll(&target.path));
            let rect = caret.map(|caret| {
                if follow_caret {
                    scroll = geometry.keep_visible(scroll, caret);
                }
                let area = geometry.area;
                layout.window_bounds(Rect::new(
                    area.x + (caret.x - scroll.x).clamp(0.0, area.width),
                    area.y
                        + (caret.y - scroll.y)
                            .clamp(0.0, (area.height - geometry.line_height).max(0.0)),
                    1.0,
                    geometry.line_height,
                ))
            });
            input.set_text_scroll(&target.clone(), scroll);
            rect
        } else {
            let text = input.editor_value(node);
            let completion = input.combo_completion(node, &target.path);
            let hint = completion
                .as_ref()
                .map(|completion| (completion.suffix(), layout.clip));
            let x = self.single_line_caret_in_direction(
                node,
                bounds,
                &text,
                input.editor.as_ref(),
                (layout.layout_direction_override(), hint),
            );
            Some(layout.window_bounds(Rect::new(x, bounds.y, 1.0, bounds.height.max(1.0))))
        }
    }
}

fn entry_origin(
    node: &Node,
    area: Rect,
    line: &VisualTextLine,
    caret: Option<TextPosition>,
    placeholder: bool,
    direction: Option<LayoutDirection>,
) -> (f32, Option<f32>) {
    let alignment = TextAlignment::editor(node, direction);
    let advance = if placeholder {
        None
    } else {
        caret.map(|position| line.caret_x(position))
    };
    let x = area.x + alignment.single_line_offset(area.width, line.width(), line.is_rtl(), advance);
    let caret_x = if placeholder {
        caret.map(|_| {
            area.x + alignment.single_line_offset(area.width, 0.0, line.is_rtl(), Some(0.0))
        })
    } else {
        advance.map(|advance| x + advance)
    };
    (x, caret_x)
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
    let (aligned_x, caret_x) =
        entry_origin(node, area, &layout, caret, placeholder, text.direction);
    let hint = text.completion.and_then(|(suffix, clip)| {
        text_system
            .inline_completion_positions(node, area, &layout, aligned_x, (suffix, clip))
            .map(|(draft, ghost)| (draft, ghost, suffix))
    });
    let x = hint.map_or(aligned_x, |(draft, _, _)| draft);
    let caret_x = caret_x.map(|caret| caret + x - aligned_x);
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
    if let Some(caret_x) = caret_x {
        rect(
            caret_x,
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
    if let Some((_, ghost, suffix)) = hint {
        canvas
            .fill_text(
                ghost,
                y,
                suffix,
                &text_system.paint_node(node, color(theme.disabled_text), false),
            )
            .map_err(|error| format!("completion drawing failed: {error}"))?;
        return Ok(true);
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
    let layout =
        text_system.text_area_layout_in_direction(node, bounds, &display.content, text.direction);
    let area = layout.area;
    if area.width <= 0.0 || area.height <= 0.0 {
        return Ok(());
    }
    let scroll = layout.clamp_scroll(text.scroll);
    let size = text_system.font_size(node);
    let height = layout.line_height;
    let origin = area.x - scroll.x;
    let advance = |text: &str| text_system.advance_node(node, text);
    let foreground = if display.placeholder || !enabled {
        theme.disabled_text
    } else {
        node.color("color").unwrap_or(theme.text)
    };
    let paint = text_system.paint_node(node, color(foreground), false);
    canvas.intersect_scissor(area.x, area.y, area.width, area.height);
    for index in layout.visible_lines(scroll) {
        let x = origin + layout.line_offset(index);
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
                origin + caret.x,
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
    layout: &LayoutNode,
) -> Result<(), String> {
    let direction = layout.layout_direction;
    let enabled = layout.enabled;
    let geometry = ToggleGeometry::new(node, bounds, direction);
    if node.control_part("ToggleIndicator").is_none() {
        if node.kind == "RadioButton" {
            crate::controls::radio_indicator(canvas, theme, node, geometry.indicator, enabled);
        } else {
            let thumb =
                (node.kind == "Switch" && node.control_part("SwitchThumb").is_none()).then(|| {
                    geometry.switch_thumb(node.boolean("checked", false), layout.switch_position)
                });
            toggle_indicator(canvas, theme, node, geometry.indicator, thumb, enabled);
        }
    }
    crate::controls::leading_label(
        canvas,
        text_system,
        node,
        geometry.label,
        node.text(if node.kind == "GroupBox" {
            "title"
        } else {
            "text"
        })
        .unwrap_or(""),
        if enabled {
            node.color("color").unwrap_or(theme.text)
        } else {
            theme.disabled_text
        },
        direction,
    )
}

fn toggle_indicator(
    canvas: &mut Canvas<WGPURenderer>,
    theme: Palette,
    node: &Node,
    bounds: Rect,
    thumb: Option<Rect>,
    enabled: bool,
) {
    let Rect {
        x,
        y,
        width,
        height,
    } = bounds;
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let box_size = height.min(width);
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
    let mut frame = Path::new();
    frame.rounded_rect(
        x,
        y,
        width,
        height,
        if node.kind == "Switch" {
            box_size * 0.5
        } else {
            4.0
        },
    );
    canvas.fill_path(&frame, &Paint::color(color(background)));
    if let Some(thumb) = thumb {
        let mut knob = Path::new();
        knob.circle(
            thumb.x + thumb.width * 0.5,
            thumb.y + thumb.height * 0.5,
            thumb.width * 0.5,
        );
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
    } else if node.kind != "Switch" && state != CheckState::Unchecked {
        let y = y + (height - box_size) * 0.5;
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
}
