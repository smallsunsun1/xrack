//! Map AccessKit text positions to the editor's current UTF-8 text.
use accesskit::{Node as AccessibleNode, NodeId, Role, TextDirection, TextSelection};
use std::{borrow::Cow, ops::Range};
use unicode_segmentation::UnicodeSegmentation;
use xrack_layout::Affine;
use xrack_layout::{
    ComboGeometry, Interaction, PasswordMask, Point, Rect, TextAlignment, TextAreaLayout, TextEdit,
    TextPosition, VisualTextLine, font_size, text_editor_line_bounds, text_editor_viewport,
};
use xrack_renderer_femtovg::TextSystem;
use xrack_runtime::{
    LayoutDirection, Node, TextCommand, TextCommandAvailability, TextWrap, combo_editable,
    is_multiline_text_input, is_text_input, spin_editable, text_input_value,
};

// AccessKit represents word start indices with u8. Runs on the same visual
// line are linked so splitting a long line doesn't change line navigation.
const RUN_CHARACTERS: usize = 256;

pub(crate) fn supports_ranges(node: &Node) -> bool {
    is_text_input(node.kind) || combo_editable(node) || node.selectable_styled_text()
}

pub(crate) fn accessible_text_value<'a>(
    node: &'a Node,
    input: &'a Interaction,
    path: &[usize],
) -> Cow<'a, str> {
    if input.table_cell_editing(path) {
        input.editor_value(node)
    } else {
        Cow::Borrowed(text_input_value(node))
    }
}

pub(crate) fn selection_positions(editor: &TextEdit, value: &str) -> (usize, TextPosition) {
    if cfg!(target_os = "android")
        && editor.has_committed_suggestion(value)
        && let Some(cursor) = editor.preedit_cursor
    {
        // The replacement range is internal to the input method. Report the
        // caret drawn within the unchanged composing text to Android instead.
        let offset = editor.selection(value).start + cursor;
        return (
            offset,
            TextPosition {
                offset,
                trailing: false,
            },
        );
    }
    (editor.anchor(value), editor.position(value))
}

#[derive(PartialEq)]
pub(crate) struct EditorState {
    anchor: usize,
    focus: TextPosition,
    composing: bool,
    #[cfg(target_os = "android")]
    committed_suggestion: bool,
    horizontal_drag_scroll: Option<f32>,
    draft: Option<String>,
    pub(super) ios_actions: Option<IosTextActions>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct IosTextActions {
    commands: TextCommandAvailability,
    select_all: bool,
    paste: bool,
}

impl IosTextActions {
    pub(super) fn focused(scene: &Node, input: &Interaction) -> Option<Self> {
        let target = input.focused.as_ref()?;
        let node = target.node_in(scene)?;
        if input.menu_open() || !(input.is_text_editor(node, target) || spin_editable(node)) {
            return None;
        }
        let editor = input
            .editor
            .as_ref()
            .filter(|editor| !editor.is_composing())?;
        let value = input.editor_value(node);
        let selection = editor.selection(&value);
        Some(Self {
            commands: input.text_command_availability(
                scene,
                target,
                node,
                &value,
                !selection.is_empty(),
            ),
            select_all: selection.len() < value.len(),
            paste: !node.text_read_only(),
        })
    }

    pub(super) fn allows(self, command: TextCommand) -> bool {
        match command {
            TextCommand::SelectAll => self.select_all,
            TextCommand::Copy => self.commands.copy,
            TextCommand::Cut => self.commands.cut,
            TextCommand::Paste => self.paste,
            TextCommand::Undo => self.commands.undo,
            TextCommand::Redo => self.commands.redo,
            _ => false,
        }
    }
}

impl EditorState {
    pub fn focused(scene: &Node, input: &Interaction, ios: bool) -> Option<Self> {
        let target = input.focused.as_ref()?;
        let cell_editing = input.table_cell_editing(&target.path);
        let node = scene.at_path(&target.path)?;
        let draft = cell_editing
            || (ios || cfg!(target_os = "android")) && xrack_runtime::spin_editable(node);
        if !draft && !supports_ranges(node) {
            return None;
        }
        let editor = input.editor.as_ref()?;
        let text = if draft {
            input.editor_value(node)
        } else {
            accessible_text_value(node, input, &target.path)
        };
        let (anchor, focus) = selection_positions(editor, &text);
        Some(Self {
            anchor,
            focus,
            composing: !editor.preedit.is_empty(),
            #[cfg(target_os = "android")]
            committed_suggestion: editor.has_committed_suggestion(&text),
            horizontal_drag_scroll: editor.horizontal_drag_scroll(),
            draft: draft.then(|| text.into_owned()),
            ios_actions: if ios {
                IosTextActions::focused(scene, input)
            } else {
                None
            },
        })
    }
}

struct Run {
    id: NodeId,
    offsets: Vec<usize>,
    trailing_boundary: bool,
}

pub(crate) struct TextDocument {
    runs: Vec<Run>,
    path: Vec<usize>,
    scroll: Point,
    cell_draft: Option<String>,
}

impl TextDocument {
    pub fn scroll_matches(&self, input: &Interaction) -> bool {
        self.scroll == input.text_scroll(&self.path)
    }

    pub fn matches_value(&self, scene: &Node, value: &str) -> bool {
        self.cell_draft.as_deref().map_or_else(
            || {
                scene
                    .at_path(&self.path)
                    .is_some_and(|node| text_input_value(node) == value)
            },
            |draft| draft == value,
        )
    }

    fn position(&self, position: TextPosition) -> accesskit::TextPosition {
        let mut index = self
            .runs
            .partition_point(|run| run.offsets[0] <= position.offset)
            - 1;
        if position.trailing
            && index > 0
            && self.runs[index].offsets[0] == position.offset
            && self.runs[index - 1].trailing_boundary
        {
            index -= 1;
        }
        let run = &self.runs[index];
        accesskit::TextPosition {
            node: run.id,
            character_index: run
                .offsets
                .binary_search(&position.offset)
                .unwrap_or_else(|next| next.saturating_sub(1)),
        }
    }

    pub fn selection(&self, editor: &TextEdit, value: &str) -> TextSelection {
        let (anchor, focus) = selection_positions(editor, value);
        self.selection_at(anchor, focus)
    }

    pub fn selection_at(&self, anchor: usize, position: TextPosition) -> TextSelection {
        let focus = self.position(position);
        TextSelection {
            anchor: if anchor == position.offset {
                focus
            } else {
                self.position(TextPosition {
                    offset: anchor,
                    trailing: false,
                })
            },
            focus,
        }
    }

    fn decode_position(&self, position: accesskit::TextPosition) -> Option<TextPosition> {
        let run = self.runs.iter().find(|run| run.id == position.node)?;
        let offset = *run.offsets.get(position.character_index)?;
        Some(TextPosition {
            offset,
            trailing: run.trailing_boundary && Some(&offset) == run.offsets.last(),
        })
    }

    pub fn decode(&self, selection: TextSelection) -> Option<(usize, TextPosition)> {
        Some((
            self.decode_position(selection.anchor)?.offset,
            self.decode_position(selection.focus)?,
        ))
    }
}

pub(crate) fn project_text(
    node: &Node,
    rect: Rect,
    path: &[usize],
    input: &Interaction,
    (scale, transform, direction, clip): (f64, Affine, Option<LayoutDirection>, Rect),
    text_system: Option<&TextSystem>,
    mut allocate_id: impl FnMut(usize, &str) -> NodeId,
) -> (TextDocument, Vec<(NodeId, AccessibleNode)>) {
    let text = accessible_text_value(node, input, path);
    let source = text.as_ref();
    let mask = node
        .is_password()
        .then(|| PasswordMask::for_node(node, source));
    let value = mask.as_ref().map_or(source, PasswordMask::display);
    let editor = input.displayed_editor(node, path);
    let displayed_mask = node
        .password_masked()
        .then(|| PasswordMask::for_editor(node, source, editor));
    // Shape the same glyphs the user sees, while publishing only the redacted value.
    let measured = displayed_mask
        .as_ref()
        .map_or(source, PasswordMask::display);
    let scroll = input.text_scroll(path);
    let rect = if combo_editable(node) {
        ComboGeometry::for_control(node, rect, direction.unwrap_or_default()).entry
    } else if node.kind == "SearchField" {
        xrack_layout::SearchGeometry::with_layout_direction(rect, direction.unwrap_or_default())
            .entry
    } else {
        rect
    };
    let size = text_system.map_or_else(
        || font_size(node),
        |system| system.entry_font_size(node, rect),
    );
    let viewport = text_editor_viewport(node, rect);
    let multiline = is_multiline_text_input(node.kind);
    let layout = multiline.then(|| {
        text_system.map_or_else(
            || {
                TextAreaLayout::new(
                    value,
                    viewport,
                    size * 1.4,
                    TextWrap::NoWrap,
                    |_| 0.0,
                    str::len,
                )
            },
            |text_system| text_system.text_area_layout_in_direction(node, rect, value, direction),
        )
    });
    let single = 0..value.len();
    let lines = layout
        .as_ref()
        .map_or(std::slice::from_ref(&single), |layout| &layout.lines);
    let single_geometry = if multiline {
        None
    } else {
        text_system.map(|system| system.single_line_layout_at_size(node, measured, size))
    };
    let (origin, height) = if let Some(layout) = &layout {
        let scroll = layout.clamp_scroll(scroll);
        (
            Point {
                x: layout.area.x - scroll.x,
                y: layout.area.y - scroll.y,
            },
            layout.line_height,
        )
    } else {
        let height = size * 1.4;
        let alignment = TextAlignment::editor(node, direction);
        let offset = single_geometry.as_ref().map_or(0.0, |visual| {
            let caret = editor.map(|editor| {
                let mut caret = editor.position(source);
                if let Some(mask) = &displayed_mask {
                    caret.offset = mask.display_offset(caret.offset);
                }
                visual.caret_x(caret)
            });
            let aligned_x = viewport.x
                + alignment.single_line_view_offset(
                    viewport.width,
                    visual.width(),
                    visual.is_rtl(),
                    caret,
                    editor.and_then(TextEdit::horizontal_drag_scroll),
                );
            let draft_x = (|| {
                let completion = input.combo_completion(node, path)?;
                let (draft, _) = text_system?.inline_completion_positions(
                    node,
                    viewport,
                    visual,
                    aligned_x,
                    (completion.suffix(), clip),
                )?;
                Some(draft)
            })()
            .unwrap_or(aligned_x);
            draft_x - viewport.x
        });
        (
            Point {
                x: viewport.x + offset,
                y: text_editor_line_bounds(viewport, height).y,
            },
            height,
        )
    };
    let word_starts: Vec<_> = if mask.is_some() {
        Vec::new()
    } else {
        value
            .unicode_word_indices()
            .map(|(offset, _)| offset)
            .collect()
    };
    let mut document = TextDocument {
        runs: Vec::new(),
        path: path.to_vec(),
        scroll,
        cell_draft: input.table_cell_editing(path).then(|| source.to_owned()),
    };
    let mut nodes: Vec<(NodeId, AccessibleNode)> = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        let next = lines.get(index + 1);
        // The renderer omits line terminators from the visual line. AccessKit
        // needs them in its text runs, including CRLF as a single character.
        let range = line.start..next.map_or(line.end, |next| next.start);
        let newline = range.end > line.end;
        let offsets = character_offsets(value, range);
        let count = offsets.len() - 1;
        let y = origin.y + index as f32 * height;
        let positions = layout
            .as_ref()
            .and_then(|layout| layout.visual_line(index))
            .or(single_geometry.as_deref())
            .filter(|_| {
                y + height > viewport.y
                    && y < viewport.y + viewport.height
                    && editor.is_none_or(|editor| editor.preedit.is_empty())
            })
            .and_then(|visual| character_geometry(visual, count, newline.then_some(size * 0.45)));

        let mut first = 0;
        loop {
            let limit = (first + RUN_CHARACTERS).min(count);
            let end = positions
                .as_ref()
                .map_or(limit, |positions| contiguous_end(positions, first, limit));
            let offsets = &offsets[first..=end];
            let start_byte = offsets[0];
            let end_byte = offsets[end - first];
            let text = &value[start_byte..end_byte];
            let id = allocate_id(start_byte, text);
            let mut run = text_run(text, offsets, &word_starts);
            if first != 0
                && let Some((previous_id, previous)) = nodes.last_mut()
            {
                previous.set_next_on_line(id);
                run.set_previous_on_line(*previous_id);
            }
            if let Some(positions) = &positions {
                publish_geometry(
                    &mut run,
                    &positions[first..end],
                    Point {
                        x: origin.x
                            + layout
                                .as_ref()
                                .map_or(0.0, |layout| layout.line_offset(index)),
                        y,
                    },
                    height,
                    scale,
                    transform,
                );
            }
            nodes.push((id, run));
            document.runs.push(Run {
                id,
                offsets: offsets
                    .iter()
                    .map(|&offset| {
                        mask.as_ref()
                            .map_or(offset, |mask| mask.source_offset(offset))
                    })
                    .collect(),
                trailing_boundary: end < count || next.is_some_and(|next| next.start == line.end),
            });
            if end == count {
                break;
            }
            first = end;
        }
    }
    (document, nodes)
}

pub(crate) fn project_styled_text(
    node: &Node,
    layout: &xrack_layout::LayoutNode,
    path: &[usize],
    scale: f64,
    mut allocate_id: impl FnMut(usize, &str) -> NodeId,
) -> (TextDocument, Vec<(NodeId, AccessibleNode)>) {
    let value = text_input_value(node);
    let mut fragments = layout
        .styled_text
        .iter()
        .flat_map(|plan| &plan.fragments)
        .filter_map(|fragment| {
            let (visual, range, prefix) = fragment.selection_geometry()?;
            Some((range, fragment.bounds, visual, prefix))
        })
        .collect::<Vec<_>>();
    fragments.sort_by_key(|(range, ..)| range.start);
    let mut segments = Vec::new();
    let mut end = 0;
    for (range, bounds, visual, prefix) in fragments {
        if end < range.start {
            segments.push((end..range.start, None));
        }
        end = range.end;
        segments.push((range, Some((bounds, visual, prefix))));
    }
    if end < value.len() || segments.is_empty() {
        segments.push((end..value.len(), None));
    }

    let starts: Vec<_> = value.unicode_word_indices().map(|(byte, _)| byte).collect();
    let mut document = TextDocument {
        runs: Vec::new(),
        path: path.to_vec(),
        scroll: Point::default(),
        cell_draft: None,
    };
    let mut nodes: Vec<(NodeId, AccessibleNode)> = Vec::new();
    let mut previous_line = None;
    for (index, (range, geometry)) in segments.iter().enumerate() {
        let offsets = character_offsets(value, range.clone());
        let count = offsets.len() - 1;
        let positions = geometry
            .and_then(|(_, visual, prefix)| character_geometry_from(visual, count, None, prefix));
        let line = geometry.map(|(bounds, _, _)| bounds.y);
        let mut first = 0;
        loop {
            let limit = (first + RUN_CHARACTERS).min(count);
            let last = positions
                .as_ref()
                .map_or(limit, |positions| contiguous_end(positions, first, limit));
            let offsets = &offsets[first..=last];
            let start = offsets[0];
            let end = offsets[last - first];
            let text = &value[start..end];
            let id = allocate_id(start, text);
            let mut run = text_run(text, offsets, &starts);
            if let Some(((bounds, _, _), positions)) = geometry.as_ref().zip(positions.as_ref()) {
                publish_geometry(
                    &mut run,
                    &positions[first..last],
                    Point {
                        x: layout.bounds.x + bounds.x,
                        y: layout.bounds.y + bounds.y,
                    },
                    bounds.height,
                    scale,
                    layout.world_transform(),
                );
            }
            if (first != 0 || line.is_some() && line == previous_line)
                && let Some((previous_id, previous)) = nodes.last_mut()
            {
                previous.set_next_on_line(id);
                run.set_previous_on_line(*previous_id);
            }
            previous_line = line;
            let trailing_boundary = last < count
                || segments.get(index + 1).is_some_and(|(_, next)| {
                    line.is_some() && next.is_some_and(|(bounds, _, _)| line == Some(bounds.y))
                });
            document.runs.push(Run {
                id,
                offsets: offsets.to_vec(),
                trailing_boundary,
            });
            nodes.push((id, run));
            if last == count {
                break;
            }
            first = last;
        }
    }
    (document, nodes)
}

fn text_run(text: &str, offsets: &[usize], word_starts: &[usize]) -> AccessibleNode {
    let mut run = AccessibleNode::new(Role::TextRun);
    run.set_value(text);
    run.set_character_lengths(
        offsets
            .windows(2)
            .map(|pair| (pair[1] - pair[0]) as u8)
            .collect::<Vec<_>>(),
    );
    let words = word_starts.partition_point(|&offset| offset < offsets[0])
        ..word_starts.partition_point(|&offset| offset < offsets[offsets.len() - 1]);
    run.set_word_starts(
        word_starts[words]
            .iter()
            .filter_map(|offset| offsets.binary_search(offset).ok().map(|index| index as u8))
            .collect::<Vec<_>>(),
    );
    run
}

#[derive(Clone, Copy)]
struct CharacterGeometry {
    leading: f32,
    trailing: f32,
    rtl: bool,
}

fn character_geometry(
    visual: &VisualTextLine,
    count: usize,
    newline_width: Option<f32>,
) -> Option<Vec<CharacterGeometry>> {
    character_geometry_from(visual, count, newline_width, 0)
}

fn character_geometry_from(
    visual: &VisualTextLine,
    count: usize,
    newline_width: Option<f32>,
    prefix: usize,
) -> Option<Vec<CharacterGeometry>> {
    let cells = visual.graphemes();
    let cells = &cells[cells.partition_point(|cell| cell.bytes.end <= prefix)..];
    if cells.len() + usize::from(newline_width.is_some()) != count {
        return None;
    }
    let mut cells: Vec<_> = cells
        .iter()
        .map(|cell| CharacterGeometry {
            leading: cell.leading,
            trailing: cell.trailing,
            rtl: cell.rtl,
        })
        .collect();
    if let Some(width) = newline_width {
        let leading = cells.last().map_or(0.0, |cell| cell.trailing);
        let rtl = visual.is_rtl();
        cells.push(CharacterGeometry {
            leading,
            trailing: leading + if rtl { -width } else { width },
            rtl,
        });
    }
    Some(cells)
}

fn contiguous_end(cells: &[CharacterGeometry], first: usize, end: usize) -> usize {
    cells[first..end]
        .windows(2)
        .position(|pair| {
            pair[0].rtl != pair[1].rtl || (pair[0].trailing - pair[1].leading).abs() > 0.01
        })
        .map_or(end, |index| first + index + 1)
}

fn publish_geometry(
    run: &mut AccessibleNode,
    cells: &[CharacterGeometry],
    origin: Point,
    height: f32,
    scale: f64,
    transform: Affine,
) {
    let (Some(first), Some(last)) = (cells.first(), cells.last()) else {
        return;
    };
    let left = first.leading.min(last.trailing);
    let right = first.leading.max(last.trailing);
    let bounds = transform.map_rect(Rect::new(origin.x + left, origin.y, right - left, height));
    run.set_bounds(accesskit::Rect::new(
        f64::from(bounds.x) * scale,
        f64::from(bounds.y) * scale,
        f64::from(bounds.x + bounds.width) * scale,
        f64::from(bounds.y + bounds.height) * scale,
    ));
    run.set_text_direction(if first.rtl {
        TextDirection::RightToLeft
    } else {
        TextDirection::LeftToRight
    });
    let [a, b, c, d, _, _] = transform.0;
    if b == 0.0 && c == 0.0 && a > 0.0 && d > 0.0 {
        let scale = scale as f32 * a;
        run.set_character_positions(
            cells
                .iter()
                .map(|cell| (cell.leading - first.leading).abs() * scale)
                .collect::<Vec<_>>(),
        );
        run.set_character_widths(
            cells
                .iter()
                .map(|cell| (cell.trailing - cell.leading).abs() * scale)
                .collect::<Vec<_>>(),
        );
    }
}

fn character_offsets(value: &str, range: Range<usize>) -> Vec<usize> {
    let mut offsets = Vec::new();
    for (start, grapheme) in value[range.clone()].grapheme_indices(true) {
        let start = start + range.start;
        if u8::try_from(grapheme.len()).is_ok() {
            offsets.push(start);
        } else {
            // A single AccessKit character can encode at most 255 bytes.
            // The editor still clamps any incoming offset to the actual grapheme.
            offsets.extend(grapheme.char_indices().map(|(offset, _)| start + offset));
        }
    }
    offsets.push(range.end);
    offsets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_both_visual_sides_of_a_soft_wrap() {
        let value = "你好世界";
        let document = TextDocument {
            runs: vec![
                Run {
                    id: NodeId(1),
                    offsets: vec![0, 3, 6],
                    trailing_boundary: true,
                },
                Run {
                    id: NodeId(2),
                    offsets: vec![6, 9, 12],
                    trailing_boundary: false,
                },
            ],
            path: Vec::new(),
            scroll: Point::default(),
            cell_draft: None,
        };
        let mut editor = TextEdit::default();
        for (trailing, node, character_index) in [(true, NodeId(1), 2), (false, NodeId(2), 0)] {
            let position = TextPosition {
                offset: 6,
                trailing,
            };
            editor.place_caret(value, position, false);
            let native = document.selection(&editor, value);
            assert_eq!(native.anchor, native.focus);
            assert_eq!(
                native.focus,
                accesskit::TextPosition {
                    node,
                    character_index
                }
            );
            assert_eq!(document.decode(native), Some((6, position)));
        }
    }

    #[test]
    fn publishes_right_to_left_run_geometry_in_logical_character_order() {
        let visual = VisualTextLine::new("aאבb", [0, 3, 1, 5].map(|offset| (offset, 10.0)));
        let cells = character_geometry(&visual, 4, None).unwrap();
        assert_eq!(contiguous_end(&cells, 0, 4), 1);
        assert_eq!(contiguous_end(&cells, 1, 4), 3);
        let mut node = AccessibleNode::new(Role::TextRun);
        publish_geometry(
            &mut node,
            &cells[1..3],
            Point::default(),
            12.0,
            1.0,
            Affine::IDENTITY,
        );
        assert_eq!(node.text_direction(), Some(TextDirection::RightToLeft));
        assert_eq!(
            node.bounds(),
            Some(accesskit::Rect::new(10.0, 0.0, 30.0, 12.0))
        );
        assert_eq!(node.character_positions(), Some([0.0, 10.0].as_slice()));
        assert_eq!(node.character_widths(), Some([10.0, 10.0].as_slice()));
    }

    #[test]
    fn links_long_text_runs_on_the_same_line() {
        let value = format!("{} two", "a".repeat(RUN_CHARACTERS + 1));
        let mut field = Node::new("LineEdit", None);
        field.properties.insert("text", value.clone().into());
        let mut id = 0;
        let (document, runs) = project_text(
            &field,
            Rect::new(0.0, 0.0, 800.0, 32.0),
            &[],
            &Interaction::default(),
            (
                1.0,
                Affine::IDENTITY,
                None,
                Rect::new(0.0, 0.0, 800.0, 32.0),
            ),
            None,
            |_, _| {
                id += 1;
                NodeId(id)
            },
        );
        let [(first, left), (last, right)] = runs.as_slice() else {
            panic!("expected two runs")
        };
        assert_eq!(left.character_lengths().len(), RUN_CHARACTERS);
        assert_eq!(left.next_on_line(), Some(*last));
        assert_eq!(right.previous_on_line(), Some(*first));
        assert_eq!(right.word_starts(), &[2]);
        let selection = document.selection(&TextEdit::at_end(&value), &value);
        assert_eq!(selection.focus.node, *last);
        assert_eq!(document.decode(selection).unwrap().1.offset, value.len());
    }
}
