//! Map AccessKit text positions to the editor's committed UTF-8 text.
use accesskit::{Node as AccessibleNode, NodeId, Role, TextDirection, TextSelection};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;
use xrack_layout::Affine;
use xrack_layout::{
    ComboGeometry, Interaction, PasswordMask, Point, Rect, TextAreaLayout, TextEdit, TextMeasurer,
    TextPosition, VisualTextLine, font_size,
};
use xrack_renderer_femtovg::TextSystem;
use xrack_runtime::{
    Node, TextWrap, combo_editable, is_multiline_text_input, is_text_input, text_input_value,
};

// AccessKit represents word start indices with u8. Runs on the same visual
// line are linked so splitting a long line doesn't change line navigation.
const RUN_CHARACTERS: usize = 256;

pub(crate) fn supports_ranges(node: &Node) -> bool {
    is_text_input(node.kind) || combo_editable(node)
}

pub(crate) fn focused_editor<'a>(input: &'a Interaction, path: &[usize]) -> Option<&'a TextEdit> {
    input.editor.as_ref().filter(|_| input.is_focused(path))
}

#[derive(PartialEq)]
pub(crate) struct EditorState {
    anchor: usize,
    focus: TextPosition,
    composing: bool,
}

impl EditorState {
    pub fn focused(scene: &Node, input: &Interaction) -> Option<Self> {
        let target = input.focused.as_ref()?;
        let node = scene
            .at_path(&target.path)
            .filter(|node| supports_ranges(node))?;
        let editor = input.editor.as_ref()?;
        let text = text_input_value(node);
        Some(Self {
            anchor: editor.anchor(text),
            focus: editor.position(text),
            composing: !editor.preedit.is_empty(),
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
}

impl TextDocument {
    pub fn scroll_matches(&self, input: &Interaction) -> bool {
        self.scroll == input.text_scroll(&self.path)
    }

    pub fn matches_value(&self, scene: &Node, value: &str) -> bool {
        scene
            .at_path(&self.path)
            .is_some_and(|node| text_input_value(node) == value)
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
                .expect("editor boundary"),
        }
    }

    pub fn selection(&self, editor: &TextEdit, value: &str) -> TextSelection {
        let position = editor.position(value);
        let anchor = editor.anchor(value);
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
    (scale, transform): (f64, Affine),
    text_system: Option<&TextSystem>,
    mut allocate_id: impl FnMut(usize, &str) -> NodeId,
) -> (TextDocument, Vec<(NodeId, AccessibleNode)>) {
    let source = text_input_value(node);
    let mask = node.is_password().then(|| PasswordMask::new(source));
    let value = mask.as_ref().map_or(source, PasswordMask::display);
    let revealed = mask.as_ref().filter(|_| !node.password_masked());
    let measured = |range: Range<usize>| {
        if let Some(mask) = revealed {
            &source[mask.source_offset(range.start)..mask.source_offset(range.end)]
        } else {
            &value[range]
        }
    };
    let editor = focused_editor(input, path);
    let scroll = input.text_scroll(path);
    let size = text_system.map_or_else(|| font_size(node), |system| system.font_size(node));
    let rect = if combo_editable(node) {
        ComboGeometry::new(rect).entry
    } else if node.kind == "SearchField" {
        xrack_layout::SearchGeometry::new(rect).entry
    } else {
        rect
    };
    let viewport = rect.inset(node.length("padding").unwrap_or(6.0).max(0.0));
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
            |text_system| text_system.text_area_layout(node, rect, value),
        )
    });
    let single = 0..value.len();
    let lines = layout
        .as_ref()
        .map_or(std::slice::from_ref(&single), |layout| &layout.lines);
    let single_geometry = if multiline {
        None
    } else {
        text_system.map(|system| system.single_line_layout(node, measured(single.clone())))
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
        let x_scroll = if let (Some(editor), Some(visual)) = (editor, &single_geometry) {
            let mut caret = editor.position(source);
            if let Some(mask) = mask.as_ref().filter(|_| node.password_masked()) {
                caret.offset = mask.display_offset(caret.offset);
            }
            (visual.caret_x(caret) + 1.0 - viewport.width).max(0.0)
        } else {
            0.0
        };
        (
            Point {
                x: viewport.x - x_scroll,
                y: rect.y + ((rect.height - height) * 0.5).max(0.0),
            },
            height,
        )
    };
    let word_starts: Vec<_> = value
        .unicode_word_indices()
        .map(|(offset, _)| offset)
        .collect();
    let mut document = TextDocument {
        runs: Vec::new(),
        path: path.to_vec(),
        scroll,
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
            let mut run = AccessibleNode::new(Role::TextRun);
            run.set_value(text);
            run.set_character_lengths(
                offsets
                    .windows(2)
                    .map(|pair| (pair[1] - pair[0]) as u8)
                    .collect::<Vec<_>>(),
            );
            let words = word_starts.partition_point(|&offset| offset < start_byte)
                ..word_starts.partition_point(|&offset| offset < end_byte);
            run.set_word_starts(
                word_starts[words]
                    .iter()
                    .filter_map(|offset| {
                        offsets.binary_search(offset).ok().map(|index| index as u8)
                    })
                    .collect::<Vec<_>>(),
            );
            if first != 0 {
                let (previous_id, previous) = nodes.last_mut().expect("previous run on line");
                previous.set_next_on_line(id);
                run.set_previous_on_line(*previous_id);
            }
            if let Some(positions) = &positions {
                publish_geometry(
                    &mut run,
                    &positions[first..end],
                    Point { x: origin.x, y },
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
    if visual.graphemes().len() + usize::from(newline_width.is_some()) != count {
        return None;
    }
    let mut cells: Vec<_> = visual
        .graphemes()
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
            (1.0, Affine::IDENTITY),
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
