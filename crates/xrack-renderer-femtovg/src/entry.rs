use crate::{
    TextSystem, color,
    text::fill_selected_text,
    theme::{Palette, PaletteRole as Role, contrasting_text},
};
use femtovg::{Canvas, Color, Paint, Path, renderer::WGPURenderer};
use std::{
    borrow::Cow,
    ops::Range,
    time::{Duration, Instant},
};
use xrack_layout::{
    ComboGeometry, Interaction, LayoutNode, PasswordMask, Point, Rect, SearchGeometry,
    SpinGeometry, Target, TextAlignment, TextAreaLayout, TextAreaMotion, TextEdit, TextMeasurer,
    TextPosition, ToggleGeometry, VisualTextLine, selection_drag_axis, text_editor_line_bounds,
    text_editor_viewport,
};
use xrack_runtime::{
    CheckState, Color as UiColor, LayoutDirection, MouseSelectionMode, Node, combo_editable,
    is_multiline_text_input, is_spin_box, spin_editable, text_input_value,
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
        Some(ComboGeometry::for_control(node, layout.bounds, layout.layout_direction).entry)
    } else if node.kind == "SearchField" {
        Some(SearchGeometry::for_layout(layout).entry)
    } else {
        None
    }
}

enum EditorPointerMode {
    Caret,
    Press,
    StartWord,
    StartParagraph,
    Drag,
    TouchDrag,
}

/// A pointer hit bound to the editor that owned the press.
#[doc(hidden)]
pub struct PointerCaret {
    target: Target,
    origin: Point,
    point: Point,
    selecting: bool,
    initial_view: bool,
    mode: EditorPointerMode,
}

/// A selection endpoint in window coordinates.
#[doc(hidden)]
pub struct TouchSelectionHandle {
    pub center: Point,
    pub caret: Point,
    pub position: TextPosition,
}

impl TouchSelectionHandle {
    pub fn distance(&self, point: Point) -> f32 {
        (self.center.x - point.x).hypot(self.center.y - point.y)
    }

    pub(crate) fn paint(&self, canvas: &mut Canvas<WGPURenderer>, tint: Color) {
        let mut stem = Path::new();
        stem.move_to(self.caret.x, self.caret.y);
        stem.line_to(self.center.x, self.center.y);
        canvas.stroke_path(&stem, &Paint::color(tint).with_line_width(2.0));
        let mut knob = Path::new();
        knob.circle(self.center.x, self.center.y, 5.5);
        canvas.fill_path(&knob, &Paint::color(tint));
    }
}

impl TextSystem {
    /// Use the same row-constrained font size for the cell value and its editor.
    #[doc(hidden)]
    pub fn entry_font_size(&self, node: &Node, bounds: Rect) -> f32 {
        if matches!(node.kind, "TableCell" | "TreeCell") {
            self.row_label_font_size(node, bounds.height)
        } else {
            self.font_size(node)
        }
    }

    pub(crate) fn row_label_font_size(&self, node: &Node, height: f32) -> f32 {
        let size = self.font_size(node);
        if self.text_scale() > 1.0 {
            size.min(((height - 4.0) / 1.4).max(1.0))
        } else {
            size
        }
    }

    /// Visible highlight rectangles in the editor's layout coordinates.
    #[doc(hidden)]
    pub fn editor_selection_rects(
        &self,
        node: &Node,
        item: &LayoutNode,
        input: &Interaction,
        target: &Target,
    ) -> Vec<Rect> {
        let Some(editor) = input.editor.as_ref() else {
            return Vec::new();
        };
        let value = input.editor_value(node);
        let display = display_text(node, &value, Some(editor), true);
        let selection = display.selection;
        if selection.is_empty() || !item.visible || !item.enabled {
            return Vec::new();
        }
        if node.selectable_styled_text() {
            return item
                .styled_text
                .iter()
                .flat_map(|plan| plan.fragments.placed())
                .flat_map(|(fragment, offset)| {
                    crate::styled_text::selection_spans(fragment, &selection)
                        .into_iter()
                        .map(move |span| {
                            Rect::new(
                                item.bounds.x + fragment.bounds.x + span.start,
                                item.bounds.y + (fragment.bounds.y + offset),
                                span.end - span.start,
                                fragment.bounds.height,
                            )
                        })
                })
                .collect();
        }
        let bounds = editor_part(node, item).unwrap_or(item.bounds);
        let direction = item.layout_direction_override();
        let area = text_editor_viewport(node, bounds);
        let size = self.entry_font_size(node, bounds);
        let collect = |line: &VisualTextLine, selection: Range<usize>, x, y, height| {
            line.selection_spans(selection)
                .into_iter()
                .filter_map(move |span| {
                    let rect =
                        Rect::new(x + span.start, y, span.end - span.start, height).intersect(area);
                    (rect.width > 0.0 && rect.height > 0.0).then_some(rect)
                })
        };
        if is_multiline_text_input(node.kind) {
            let geometry =
                self.text_area_layout_in_direction(node, bounds, &display.content, direction);
            let scroll = geometry.clamp_scroll(input.text_scroll(&target.path));
            geometry
                .visible_lines(scroll)
                .filter_map(|index| {
                    let line = geometry.visual_line(index)?;
                    let start = geometry.lines[index].start;
                    let x = area.x + geometry.line_offset(index) - scroll.x;
                    let y = area.y + index as f32 * geometry.line_height - scroll.y;
                    let selection =
                        selection.start.saturating_sub(start)..selection.end.saturating_sub(start);
                    Some(collect(line, selection, x, y, geometry.line_height))
                })
                .flatten()
                .collect()
        } else {
            let line = self.single_line_layout_at_size(node, &display.content, size);
            let (x, _) = entry_origin(
                node,
                area,
                &line,
                display.caret,
                false,
                direction,
                editor.horizontal_drag_scroll(),
            );
            let height = size * 1.4;
            collect(
                &line,
                selection,
                x,
                text_editor_line_bounds(area, height).y,
                height,
            )
            .collect()
        }
    }

    /// Anchor a native edit menu to visible editor geometry in window coordinates.
    #[doc(hidden)]
    pub fn editor_menu_anchor(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        input: &mut Interaction,
        floating_keyboard: Option<Rect>,
    ) -> Option<Rect> {
        let target = input.focused.as_ref()?;
        let node = target.node_in(scene)?;
        let item = layout
            .at_path(&target.path)
            .filter(|item| item.enabled && item.visible)?;
        let editor = input
            .editor
            .as_ref()
            .filter(|editor| !editor.is_composing())?;
        let value = input.editor_value(node);
        let selection = editor.selection(&value);
        let rects = if selection.is_empty()
            || value[selection]
                .bytes()
                .all(|byte| matches!(byte, b'\n' | b'\r'))
        {
            if node.selectable_styled_text() {
                vec![item.bounds]
            } else {
                self.editor_caret_rect(input, scene, layout, false)
                    .into_iter()
                    .collect()
            }
        } else {
            self.editor_selection_rects(node, item, input, target)
        };
        rects
            .into_iter()
            .filter_map(|rect| {
                let visible = item.visible_window_bounds(rect).intersect(layout.bounds);
                unobstructed_menu_anchor(visible, floating_keyboard)
            })
            .max_by(|a, b| (a.width * a.height).total_cmp(&(b.width * b.height)))
    }

    #[doc(hidden)]
    pub fn touch_selection_handles(
        &self,
        scene: &Node,
        root: &LayoutNode,
        input: &Interaction,
    ) -> Vec<TouchSelectionHandle> {
        let Some(target) = input.touch_selection_target() else {
            return Vec::new();
        };
        let Some((node, item)) = target.node_in(scene).zip(root.at_path(&target.path)) else {
            return Vec::new();
        };
        let Some(editor) = input
            .editor
            .as_ref()
            .filter(|editor| editor.preedit.is_empty())
        else {
            return Vec::new();
        };
        if !item.visible
            || !item.enabled
            || node.is_password()
            || !xrack_runtime::is_text_input(node.kind)
                && !combo_editable(node)
                && !spin_editable(node)
        {
            return Vec::new();
        }
        let value = input.editor_value(node);
        let selection = editor.selection(&value);
        if selection.is_empty() {
            return Vec::new();
        }
        let bounds = editor_part(node, item).unwrap_or(item.bounds);
        let direction = item.layout_direction_override();
        let visible = item.visible_window_bounds(item.bounds);
        let make_handle = |position: TextPosition, area: Rect, x: f32, y: f32, height: f32| {
            let caret = Point {
                x,
                y: y + height * 0.5,
            };
            if !area.contains(caret) {
                return None;
            }
            let center = Point {
                x,
                y: y + if position.trailing {
                    height + 4.0
                } else {
                    -4.0
                },
            };
            let center = item.map_to_window(center);
            visible.contains(center).then(|| TouchSelectionHandle {
                center,
                caret: item.map_to_window(caret),
                position,
            })
        };
        let positions = [
            TextPosition {
                offset: selection.start,
                trailing: false,
            },
            TextPosition {
                offset: selection.end,
                trailing: true,
            },
        ];
        if is_multiline_text_input(node.kind) {
            let geometry = self.text_area_layout_in_direction(node, bounds, &value, direction);
            let scroll = geometry.clamp_scroll(input.text_scroll(&target.path));
            positions
                .into_iter()
                .filter_map(|position| {
                    let caret = geometry
                        .caret_point(&value, position, |value| self.advance_node(node, value));
                    make_handle(
                        position,
                        geometry.area,
                        geometry.area.x + caret.x - scroll.x,
                        geometry.area.y + caret.y - scroll.y,
                        geometry.line_height,
                    )
                })
                .collect()
        } else {
            let area = text_editor_viewport(node, bounds);
            let size = self.entry_font_size(node, bounds);
            let line = self.single_line_layout_at_size(node, &value, size);
            let line_bounds = text_editor_line_bounds(area, size * 1.4);
            let (x, _) = entry_origin(
                node,
                area,
                &line,
                Some(editor.position(&value)),
                false,
                direction,
                editor.horizontal_drag_scroll(),
            );
            positions
                .into_iter()
                .filter_map(|position| {
                    make_handle(
                        position,
                        area,
                        x + line.caret_x(position),
                        line_bounds.y,
                        line_bounds.height,
                    )
                })
                .collect()
        }
    }
}

fn unobstructed_menu_anchor(rect: Rect, keyboard: Option<Rect>) -> Option<Rect> {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    let Some(covered) = keyboard
        .map(|keyboard| rect.intersect(keyboard))
        .filter(|covered| covered.width > 0.0 && covered.height > 0.0)
    else {
        return Some(rect);
    };
    let right = covered.x + covered.width;
    let bottom = covered.y + covered.height;
    [
        Rect::new(rect.x, rect.y, rect.width, covered.y - rect.y),
        Rect::new(rect.x, bottom, rect.width, rect.y + rect.height - bottom),
        Rect::new(rect.x, rect.y, covered.x - rect.x, rect.height),
        Rect::new(right, rect.y, rect.x + rect.width - right, rect.height),
    ]
    .into_iter()
    .filter(|part| part.width > 0.0 && part.height > 0.0)
    .max_by(|a, b| (a.width * a.height).total_cmp(&(b.width * b.height)))
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
        let routed = input.control_pointer(&target.path, point)?;
        let mut pointer = Self {
            target: target.clone(),
            origin: if origin == point { routed } else { origin },
            point: routed,
            selecting,
            initial_view: false,
            mode: EditorPointerMode::Caret,
        };
        let (node, _, _, _) = pointer.hit(input, scene, layout)?;
        pointer.initial_view = input.displayed_editor(node, &target.path).is_none();
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

    #[doc(hidden)]
    pub fn mouse_press(mut self, input: &mut Interaction, now: Instant) -> Self {
        self.mode = match input.editor_mouse_press(&self.target, self.point, now, self.selecting) {
            2 => EditorPointerMode::StartWord,
            3 => EditorPointerMode::StartParagraph,
            _ => EditorPointerMode::Press,
        };
        self
    }

    #[doc(hidden)]
    pub fn during_drag(mut self) -> Self {
        self.mode = EditorPointerMode::Drag;
        self
    }

    #[doc(hidden)]
    pub fn during_touch_drag(mut self) -> Self {
        self.mode = EditorPointerMode::TouchDrag;
        self
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
        self.place_at(text_system, input, scene, layout, Instant::now())
    }

    pub(crate) fn place_at(
        self,
        text_system: &TextSystem,
        input: &mut Interaction,
        scene: &Node,
        layout: &LayoutNode,
        now: Instant,
    ) -> bool {
        self.place_with_scroll(text_system, input, scene, layout, now, Duration::ZERO)
    }

    #[doc(hidden)]
    pub fn advance_drag_scroll(
        text_system: &TextSystem,
        input: &mut Interaction,
        scene: &Node,
        layout: &LayoutNode,
        now: Instant,
    ) -> bool {
        let Some((point, elapsed, touch_origin)) = input.take_text_selection_scroll(now) else {
            return false;
        };
        let pointer = if let Some(origin) = touch_origin {
            input
                .touch_selection_target()
                .and_then(|target| Self::new(input, scene, layout, target, origin, point, true))
                .map(Self::during_touch_drag)
        } else {
            Self::from_pressed(input, scene, layout, point, true).map(Self::during_drag)
        };
        pointer.is_some_and(|pointer| {
            pointer.place_with_scroll(text_system, input, scene, layout, now, elapsed)
        })
    }

    fn place_with_scroll(
        self,
        text_system: &TextSystem,
        input: &mut Interaction,
        scene: &Node,
        layout: &LayoutNode,
        now: Instant,
        scroll_elapsed: Duration,
    ) -> bool {
        if input.focused.as_ref() != Some(&self.target) {
            return false;
        }
        let Some((node, bounds, point, direction)) = self.hit(input, scene, layout) else {
            return false;
        };
        let mode = node.mouse_selection_mode();
        if mode == MouseSelectionMode::Caret {
            match self.mode {
                EditorPointerMode::Drag => {
                    input.schedule_text_selection_scroll(&self.target, None, None, now);
                    return false;
                }
                EditorPointerMode::Press
                | EditorPointerMode::StartWord
                | EditorPointerMode::StartParagraph => input.clear_editor_click(),
                EditorPointerMode::Caret | EditorPointerMode::TouchDrag => {}
            }
        }
        input.sync_editor(scene);
        let value = editor_text(input, node);
        let touch_origin = matches!(self.mode, EditorPointerMode::TouchDrag).then_some(self.origin);
        let dragging = matches!(
            self.mode,
            EditorPointerMode::Drag | EditorPointerMode::TouchDrag
        );
        let mut drag_scroll = None;
        let (position, reveal) = if node.selectable_styled_text() {
            let local = Point {
                x: point.x - bounds.x,
                y: point.y - bounds.y,
            };
            let position = layout
                .at_path(&self.target.path)
                .and_then(|item| item.styled_text.as_ref())
                .and_then(|plan| plan.selection_hit(local))
                .unwrap_or_default();
            (position, None)
        } else if is_multiline_text_input(node.kind) {
            let geometry =
                text_system.text_area_layout_in_direction(node, bounds, &value, direction);
            let scroll = geometry.clamp_scroll(input.text_scroll(&self.target.path));
            let outside = dragging && !geometry.area.contains(point);
            let (point, scroll, pending) = if outside {
                geometry.selection_drag(point, scroll, scroll_elapsed)
            } else {
                (point, scroll, false)
            };
            if dragging {
                input.schedule_text_selection_scroll(
                    &self.target,
                    pending.then_some(self.point),
                    touch_origin,
                    now,
                );
            }
            let advance = |text: &str| text_system.advance_node(node, text);
            let position = geometry.hit(&value, point, scroll, advance);
            (position, Some((geometry, scroll, outside)))
        } else {
            // A newly focused editor defaults to a caret at the end. Resolve the
            // first press against the previous view unless a selection was restored.
            let hit_editor = input.editor.as_ref().filter(|editor| {
                dragging || !self.initial_view || !editor.selection(&value).is_empty()
            });
            let hint = input
                .painted_combo_completion_for(node, &self.target.path)
                .and_then(|completion| {
                    let item = layout.at_path(&self.target.path)?;
                    Some((completion.suffix(), item.clip))
                });
            let hit = text_system.single_line_hit_in_direction(
                node,
                bounds,
                point,
                hit_editor,
                &value,
                LineHitOptions {
                    direction,
                    completion: hint,
                    drag_elapsed: dragging.then_some(scroll_elapsed),
                },
            );
            if dragging {
                input.schedule_text_selection_scroll(
                    &self.target,
                    hit.pending.then_some(self.point),
                    touch_origin,
                    now,
                );
                drag_scroll = hit.drag_scroll;
            }
            (hit.position, None)
        };
        input.break_text_edit_group();
        let Some(editor) = &mut input.editor else {
            return false;
        };
        match (self.mode, mode) {
            (
                EditorPointerMode::StartWord,
                MouseSelectionMode::Characters | MouseSelectionMode::Words,
            ) => {
                editor.start_word_drag(&value, position, node.is_password(), true);
            }
            (
                EditorPointerMode::StartParagraph,
                MouseSelectionMode::Characters | MouseSelectionMode::Words,
            ) => {
                editor.start_paragraph_drag(&value, position, node.is_password());
            }
            (EditorPointerMode::Press, MouseSelectionMode::Words) => {
                let anchor = if self.selecting {
                    let offset = editor.anchor(&value);
                    let caret = editor.position(&value);
                    TextPosition {
                        offset,
                        trailing: offset > caret.offset || offset == caret.offset && caret.trailing,
                    }
                } else {
                    position
                };
                editor.start_word_drag(&value, anchor, node.is_password(), false);
                if self.selecting {
                    editor.extend_selection_drag(&value, position);
                }
            }
            (EditorPointerMode::Drag, mode) => {
                if !editor.extend_selection_drag(&value, position)
                    && mode == MouseSelectionMode::Characters
                {
                    editor.place_caret(&value, position, true);
                }
            }
            (EditorPointerMode::TouchDrag, _) => editor.place_caret(&value, position, true),
            (pointer, mode) => editor.place_caret(
                &value,
                position,
                self.selecting
                    && (matches!(pointer, EditorPointerMode::Caret)
                        || mode != MouseSelectionMode::Caret),
            ),
        }
        if dragging {
            editor.set_horizontal_drag_scroll(drag_scroll);
        }
        if let Some((geometry, scroll, outside)) = reveal {
            let scroll = if outside {
                scroll
            } else {
                let caret = geometry.caret_point(&value, editor.position(&value), |text| {
                    text_system.advance_node(node, text)
                });
                geometry.keep_visible(scroll, caret)
            };
            input.set_text_scroll(&self.target, scroll);
        }
        true
    }
}

pub(crate) struct EntryText<'a> {
    pub value: &'a str,
    pub editor: Option<&'a TextEdit>,
    pub active: bool,
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

struct LineHitOptions<'a> {
    direction: Option<LayoutDirection>,
    completion: Option<(&'a str, Rect)>,
    drag_elapsed: Option<Duration>,
}

struct LineHit {
    position: TextPosition,
    drag_scroll: Option<f32>,
    pending: bool,
}

fn display_text<'a>(
    node: &'a Node,
    value: &'a str,
    editor: Option<&TextEdit>,
    active: bool,
) -> DisplayText<'a> {
    let mut content = Cow::Borrowed(value);
    let mut selection = 0..0;
    let mut composition = 0..0;
    let mut caret = None;
    if let Some(edit) = editor {
        selection = edit.selection(value);
        if !active || edit.preedit.is_empty() {
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
        let mask = PasswordMask::for_editor(
            node,
            &content,
            editor.filter(|_| active && composition.is_empty()),
        );
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
        if !input.is_text_editor(node, &target)
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
        if node.selectable_styled_text() {
            let Some(plan) = &item.styled_text else {
                return false;
            };
            return plan.move_selection(&value, editor, motion, selecting);
        }
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
            let mask = node
                .password_masked()
                .then(|| PasswordMask::for_editor(node, &value, Some(editor)));
            let visual = self.single_line_layout_at_size(
                node,
                mask.as_ref().map_or(&value, PasswordMask::display),
                self.entry_font_size(node, item.bounds),
            );
            let display_offset = |offset| {
                mask.as_ref()
                    .map_or(offset, |mask| mask.display_offset(offset))
            };
            let position = match motion {
                TextAreaMotion::Left
                | TextAreaMotion::Right
                | TextAreaMotion::WordLeft
                | TextAreaMotion::WordRight => {
                    let right = matches!(motion, TextAreaMotion::Right | TextAreaMotion::WordRight);
                    let edge = (!selecting)
                        .then(|| {
                            let selection = editor.selection(&value);
                            visual.selection_edge(
                                display_offset(selection.start)..display_offset(selection.end),
                                right,
                            )
                        })
                        .flatten();
                    edge.or_else(|| {
                        let mut position = editor.position(&value);
                        position.offset = display_offset(position.offset);
                        if matches!(motion, TextAreaMotion::WordLeft | TextAreaMotion::WordRight) {
                            if mask.is_some() {
                                Some(visual.edge(right))
                            } else {
                                visual.word_step(&value, position, right)
                            }
                        } else {
                            visual.step(position, right)
                        }
                    })
                }
                TextAreaMotion::Home | TextAreaMotion::End => {
                    Some(visual.edge(matches!(motion, TextAreaMotion::End) != visual.is_rtl()))
                }
                _ => return false,
            };
            if let Some(mut position) = position {
                if let Some(mask) = &mask {
                    position.offset = mask.source_offset(position.offset);
                }
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
            LineHitOptions {
                direction: node.layout_direction(),
                completion: None,
                drag_elapsed: None,
            },
        )
        .position
    }

    fn single_line_hit_in_direction(
        &self,
        node: &Node,
        bounds: Rect,
        point: Point,
        editor: Option<&TextEdit>,
        value: &str,
        options: LineHitOptions<'_>,
    ) -> LineHit {
        let area = text_editor_viewport(node, bounds);
        let mask = node
            .password_masked()
            .then(|| PasswordMask::for_editor(node, value, editor));
        let display = mask.as_ref().map_or(value, PasswordMask::display);
        let layout =
            self.single_line_layout_at_size(node, display, self.entry_font_size(node, bounds));
        let caret = editor.map(|edit| {
            let mut caret = edit.position(value);
            if let Some(mask) = &mask {
                caret.offset = mask.display_offset(caret.offset);
            }
            caret
        });
        let mut drag_scroll = editor.and_then(TextEdit::horizontal_drag_scroll);
        let (mut x, _) = entry_origin(
            node,
            area,
            &layout,
            caret,
            false,
            options.direction,
            drag_scroll,
        );
        let mut hit_x = point.x;
        let mut pending = false;
        let overflow = layout.width() + 1.0 - area.width;
        if let Some(elapsed) = options.drag_elapsed
            && area.width > 0.0
            && overflow > 0.0
            && (hit_x < area.x || hit_x > area.x + area.width)
        {
            let (edge, offset, more) = selection_drag_axis(
                hit_x,
                area.x,
                area.width,
                (area.x - x).clamp(0.0, overflow),
                overflow,
                elapsed,
            );
            x = area.x - offset;
            hit_x = edge;
            drag_scroll = Some(offset);
            pending = more;
        } else if let Some((draft, _)) = options
            .completion
            .and_then(|hint| self.inline_completion_positions(node, area, &layout, x, hint))
        {
            x = draft;
        }
        let mut position = layout.hit(hit_x - x);
        if let Some(mask) = &mask {
            position.offset = mask.source_offset(position.offset);
        }
        LineHit {
            position,
            drag_scroll,
            pending,
        }
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
        let display = display_text(node, value, editor, true);
        let area = text_editor_viewport(node, bounds);
        let layout = self.single_line_layout_at_size(
            node,
            &display.content,
            self.entry_font_size(node, bounds),
        );
        let (x, caret) = entry_origin(
            node,
            area,
            &layout,
            display.caret,
            display.placeholder,
            direction,
            editor.and_then(TextEdit::horizontal_drag_scroll),
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
        let display = display_text(node, node.text("text").unwrap_or(""), editor, true);
        let layout = self.text_area_layout_in_direction(node, bounds, &display.content, direction);
        let caret = display.caret.map(|position| {
            layout.caret_point(&display.content, position, |text| {
                self.advance_node(node, text)
            })
        });
        (layout, caret)
    }

    /// Return the visible part of the active multiline caret in layout coordinates.
    #[doc(hidden)]
    pub fn visible_multiline_caret_rect(
        &self,
        input: &Interaction,
        scene: &Node,
        layout: &LayoutNode,
    ) -> Option<Rect> {
        let target = input.focused.as_ref()?;
        let node = scene.interactive_at_path(&target.path)?;
        if !is_multiline_text_input(node.kind) || node.text_read_only() {
            return None;
        }
        let editor = input.displayed_editor(node, &target.path)?;
        let item = layout.at_path(&target.path)?;
        let (geometry, caret) = self.text_area_caret_in_direction(
            node,
            item.bounds,
            Some(editor),
            item.layout_direction_override(),
        );
        let caret = caret?;
        let scroll = geometry.clamp_scroll(input.text_scroll(&target.path));
        let area = geometry.area;
        let rect = area.intersect(Rect::new(
            area.x + caret.x - scroll.x,
            area.y + caret.y - scroll.y,
            1.0,
            geometry.line_height,
        ));
        (rect.width > 0.0 && rect.height > 0.0).then_some(rect)
    }

    /// Return the editor caret in window coordinates for the platform input method.
    pub fn editor_ime_area(
        &self,
        input: &mut Interaction,
        scene: &Node,
        layout: &LayoutNode,
        follow_caret: bool,
    ) -> Option<Rect> {
        let caret = self.editor_caret_rect(input, scene, layout, follow_caret)?;
        let item = layout.at_path(&input.focused.as_ref()?.path)?;
        Some(item.window_bounds(caret))
    }

    /// Compute the active caret in the editor's layout coordinates.
    #[doc(hidden)]
    pub fn editor_caret_rect(
        &self,
        input: &mut Interaction,
        scene: &Node,
        layout: &LayoutNode,
        follow_caret: bool,
    ) -> Option<Rect> {
        let target = input.focused.as_ref()?;
        let node = scene.interactive_at_path(&target.path)?;
        if node.selectable_styled_text() {
            return None;
        }
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
                Rect::new(
                    area.x + (caret.x - scroll.x).clamp(0.0, area.width),
                    area.y
                        + (caret.y - scroll.y)
                            .clamp(0.0, (area.height - geometry.line_height).max(0.0)),
                    1.0,
                    geometry.line_height,
                )
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
            let line = text_editor_line_bounds(
                text_editor_viewport(node, bounds),
                self.entry_font_size(node, bounds) * 1.4,
            );
            Some(Rect::new(x, line.y, 1.0, line.height.max(1.0)))
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
    drag_scroll: Option<f32>,
) -> (f32, Option<f32>) {
    let alignment = TextAlignment::editor(node, direction);
    let advance = if placeholder {
        None
    } else {
        caret.map(|position| line.caret_x(position))
    };
    let x = area.x
        + alignment.single_line_view_offset(
            area.width,
            line.width(),
            line.is_rtl(),
            advance,
            if placeholder { None } else { drag_scroll },
        );
    let caret_x = if placeholder {
        caret.map(|_| {
            area.x + alignment.single_line_offset(area.width, 0.0, line.is_rtl(), Some(0.0))
        })
    } else {
        advance.map(|advance| x + advance)
    };
    (x, caret_x)
}

pub(super) fn passive_origin(
    text_system: &TextSystem,
    node: &Node,
    area: Rect,
    content: &str,
    paint: &Paint,
    direction: Option<LayoutDirection>,
    measurement_scale: f32,
) -> Point {
    let x = area.x
        + TextAlignment::editor(node, direction).passive_line_offset(area.width, content, || {
            let scaled = (measurement_scale != 1.0).then(|| {
                paint
                    .clone()
                    .with_font_size(paint.font_size() * measurement_scale)
            });
            let paint = scaled.as_ref().unwrap_or(paint);
            #[cfg(xrack_femtovg_glyph_samples)]
            let advance = text_system.context().unwrapped_advance(content, paint);
            #[cfg(not(xrack_femtovg_glyph_samples))]
            let advance = text_system
                .context()
                .measure_text(0.0, 0.0, content, paint)
                .map_or(0.0, |metrics| {
                    // Match VisualTextLine's advance without building editing slots.
                    metrics
                        .glyphs
                        .iter()
                        .filter(|glyph| {
                            glyph.byte_index < content.len() && glyph.advance_x.is_finite()
                        })
                        .map(|glyph| glyph.advance_x)
                        .sum::<f32>()
                        .max(0.0)
                });
            advance / measurement_scale
        });
    Point {
        x,
        y: text_editor_line_bounds(area, paint.font_size() * 1.4).y,
    }
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
        mut content,
        selection,
        composition,
        caret,
        placeholder,
    } = display_text(node, text.value, text.editor, text.active);
    let area = text_editor_viewport(node, bounds);
    if area.width <= 0.0 || area.height <= 0.0 {
        return Ok(false);
    }
    let size = text_system.entry_font_size(node, bounds);
    let height = size * 1.4;
    let y = text_editor_line_bounds(area, height).y;
    let foreground = theme.editor_foreground(node, placeholder, enabled);
    let paint = text_system.paint_node_at_size(node, size, color(foreground), false);
    if matches!(node.kind, "TableCell" | "TreeCell")
        && text_system.text_scale() > 1.0
        && text.editor.is_none()
        && !text.active
        && let Cow::Owned(shortened) = xrack_layout::elide_to_width(&content, area.width, |value| {
            text_system.advance_painted(value, &paint)
        })
    {
        content = Cow::Owned(shortened);
    }
    if text.editor.is_none() && !text.active && text.completion.is_none() {
        let origin = passive_origin(
            text_system,
            node,
            area,
            &content,
            &paint,
            text.direction,
            1.0,
        );
        canvas.intersect_scissor(area.x, area.y, area.width, area.height);
        text_system
            .fill_text(canvas, origin.x, origin.y, &content, &paint)
            .map_err(|error| format!("text input drawing failed: {error}"))?;
        return Ok(false);
    }
    let layout = text_system.single_line_layout_at_size(node, &content, size);
    let (aligned_x, caret_x) = entry_origin(
        node,
        area,
        &layout,
        caret,
        placeholder,
        text.direction,
        text.editor.and_then(TextEdit::horizontal_drag_scroll),
    );
    let hint = text.completion.and_then(|(suffix, clip)| {
        text_system
            .inline_completion_positions(node, area, &layout, aligned_x, (suffix, clip))
            .map(|(draft, ghost)| (draft, ghost, suffix))
    });
    let x = hint.map_or(aligned_x, |(draft, _, _)| draft);
    let caret_x = caret_x.map(|caret| caret + x - aligned_x);
    canvas.intersect_scissor(area.x, area.y, area.width, area.height);
    let mut rect = |x: f32, y: f32, width: f32, height: f32, tint: Color| {
        let mut path = Path::new();
        path.rect(x, y, width, height);
        canvas.fill_path(&path, &Paint::color(tint));
    };
    let selected = layout.selection_spans(selection);
    let highlight = color(theme.text_selection(node));
    for span in &selected {
        rect(x + span.start, y, span.end - span.start, height, highlight);
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
    if text.active
        && let Some(caret_x) = caret_x
    {
        rect(
            caret_x,
            y + 1.0,
            1.0,
            (height - 2.0).max(0.0),
            color(theme.focus),
        );
    }
    fill_selected_text(
        canvas,
        text_system,
        area,
        Point { x, y },
        &content,
        &paint,
        &selected,
        node.color("selected-text-color").map(color),
    )
    .map_err(|error| format!("text input drawing failed: {error}"))?;
    if let Some((_, ghost, suffix)) = hint {
        text_system
            .fill_text(
                canvas,
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
    let display = display_text(node, text.value, text.editor, text.active);
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
    let foreground = theme.editor_foreground(node, display.placeholder, enabled);
    let paint = text_system.paint_node(node, color(foreground), false);
    let selected_color = node.color("selected-text-color").map(color);
    let highlight = color(theme.text_selection(node));
    canvas.intersect_scissor(area.x, area.y, area.width, area.height);
    for index in layout.visible_lines(scroll) {
        let x = origin + layout.line_offset(index);
        let line = &layout.lines[index];
        let Some(visual) = layout.visual_line(index) else {
            continue;
        };
        let y = area.y + index as f32 * height - scroll.y;
        let local = |span: &Range<usize>| {
            span.start.saturating_sub(line.start)..span.end.saturating_sub(line.start)
        };
        let selected = visual.selection_spans(local(&display.selection));
        if !display.selection.is_empty() {
            for span in &selected {
                fill_rect(
                    canvas,
                    Rect::new(x + span.start, y, span.end - span.start, height),
                    highlight,
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
                fill_rect(canvas, Rect::new(x + left, y, width, height), highlight);
            }
        }
        if !line.is_empty() {
            fill_selected_text(
                canvas,
                text_system,
                area,
                Point { x, y },
                &display.content[line.clone()],
                &paint,
                &selected,
                selected_color,
            )
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
    if text.active
        && let Some(position) = display.caret
    {
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
            toggle_indicator(canvas, theme, node, &geometry, layout);
        }
    }
    let tint = if enabled {
        node.color("color").unwrap_or(theme.text)
    } else {
        theme.disabled_text
    };
    if let Some((size, lines)) = layout.group_title() {
        return crate::controls::draw_title(canvas, text_system, node, bounds, tint, size, lines);
    }
    crate::controls::row_label(
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
        tint,
        direction,
    )
}

fn toggle_indicator(
    canvas: &mut Canvas<WGPURenderer>,
    theme: Palette,
    node: &Node,
    geometry: &ToggleGeometry,
    layout: &LayoutNode,
) {
    let bounds = geometry.indicator;
    let enabled = layout.enabled;
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
    if node.kind == "Switch" {
        if theme.switch_labels {
            switch_label(canvas, bounds, layout.layout_direction, checked, background);
        }
        if node.control_part("SwitchThumb").is_some() {
            return;
        }
        let thumb = geometry.switch_thumb(checked, layout.switch_position);
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
    } else if state != CheckState::Unchecked {
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

fn switch_label(
    canvas: &mut Canvas<WGPURenderer>,
    track: Rect,
    direction: LayoutDirection,
    checked: bool,
    background: UiColor,
) {
    let end_radius = track.height * 0.5;
    let radius = (track.height * 0.19).min((track.width - track.height) * 0.25);
    if radius < 1.0 {
        return;
    }
    // Place the mark at the end opposite the committed position of the thumb.
    let center_x = if checked == (direction == LayoutDirection::LeftToRight) {
        track.x + end_radius
    } else {
        track.x + track.width - end_radius
    };
    let center_y = track.y + end_radius;
    let mut mark = Path::new();
    if checked {
        mark.move_to(center_x, center_y - radius);
        mark.line_to(center_x, center_y + radius);
    } else {
        mark.circle(center_x, center_y, radius);
    }
    let mut paint = Paint::color(color(contrasting_text(background)));
    paint.set_line_width(radius * 0.55);
    canvas.stroke_path(&mark, &paint);
}
