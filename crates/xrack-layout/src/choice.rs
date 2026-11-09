use crate::{LayoutNode, Point, Rect, Target, input::valid};
use std::borrow::Cow;
use unicode_segmentation::UnicodeSegmentation;
use xrack_runtime::{
    ChoiceIdentity, Node, RowKey, choice_identity, combo_count, combo_editable, combo_find_label,
    combo_index, combo_label, combo_labels, combo_row_index, combo_row_key, combo_text,
    search_suggestion, search_suggestions,
};

const ROW_HEIGHT: f32 = 28.0;
const MAX_ROWS: usize = 8;

/// The exact candidate and draft boundary displayed for one focused control.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComboCompletion {
    pub(crate) target: Target,
    pub(crate) value: String,
    pub(crate) draft_len: usize,
}

impl ComboCompletion {
    pub fn suffix(&self) -> &str {
        &self.value[self.draft_len..]
    }
}

pub(crate) fn completion_value<'a>(node: &'a Node, draft: &str) -> Option<Cow<'a, str>> {
    if draft.is_empty() || combo_find_label(node, |value| value == draft).is_some() {
        return None;
    }
    let index = combo_find_label(node, |value| {
        value.strip_prefix(draft).is_some_and(|suffix| {
            suffix.chars().all(|ch| !ch.is_control())
                && value
                    .grapheme_indices(true)
                    .any(|(index, _)| index == draft.len())
        })
    })?;
    combo_label(node, index)
}

#[cfg(test)]
mod tests {
    use super::completion_value;

    #[test]
    fn completion_skips_partial_graphemes_and_single_line_control_characters() {
        let mut node = xrack_runtime::Node::new("ComboBox", None);
        node.properties.insert(
            "model",
            vec!["e\u{301}lan".into(), "edit\n".into(), "editor".into()].into(),
        );
        assert_eq!(completion_value(&node, "e").as_deref(), Some("editor"));
        node.properties
            .insert("model", vec!["editor".into(), "e".into()].into());
        assert_eq!(completion_value(&node, "e"), None);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChoiceKey {
    Open,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Accept,
    Escape,
}

pub struct ComboGeometry {
    pub entry: Rect,
    pub trigger: Rect,
}

impl ComboGeometry {
    pub fn new(bounds: Rect) -> Self {
        let trigger = bounds.width.min(28.0);
        let entry = bounds.width - trigger;
        Self {
            entry: Rect::new(bounds.x, bounds.y, entry, bounds.height),
            trigger: Rect::new(bounds.x + entry, bounds.y, trigger, bounds.height),
        }
    }
}

pub struct ChoiceMenu<'a> {
    pub control: &'a Node,
    pub bounds: Rect,
    pub rows: Cow<'a, [String]>,
    pub first: usize,
    pub highlighted: Option<usize>,
    pub selected: Option<usize>,
}

impl ChoiceMenu<'_> {
    pub fn row_bounds(&self, offset: usize) -> Rect {
        let height = self.bounds.height / self.rows.len() as f32;
        Rect::new(
            self.bounds.x,
            self.bounds.y + height * offset as f32,
            self.bounds.width,
            height,
        )
    }
}

pub(crate) struct ChoiceGeometry {
    bounds: Rect,
    visible: usize,
}

fn geometry(anchor: Rect, viewport: Rect, len: usize) -> Option<ChoiceGeometry> {
    if len == 0 || viewport.width <= 0.0 || viewport.height <= 0.0 {
        return None;
    }
    let below = (viewport.y + viewport.height - anchor.y - anchor.height).max(0.0);
    let above = (anchor.y - viewport.y).max(0.0);
    let desired = ROW_HEIGHT * len.min(MAX_ROWS) as f32;
    let down = below >= desired || below >= above;
    let space = if down { below } else { above };
    let visible = ((space / ROW_HEIGHT) as usize).clamp(1, len.min(MAX_ROWS));
    let height = (visible as f32 * ROW_HEIGHT).min(viewport.height);
    let width = anchor.width.max(96.0).min(viewport.width);
    let x = anchor
        .x
        .clamp(viewport.x, viewport.x + viewport.width - width);
    let y = if down {
        anchor.y + anchor.height
    } else {
        anchor.y - height
    };
    let y = y.clamp(viewport.y, viewport.y + viewport.height - height);
    Some(ChoiceGeometry {
        bounds: Rect::new(x, y, width, height),
        visible,
    })
}

#[derive(Clone, Debug)]
pub(crate) struct ChoicePopup {
    pub target: Target,
    kind: &'static str,
    highlight: Option<usize>,
    highlighted_value: Option<String>,
    highlighted_row: Option<RowKey>,
    first: usize,
    press: Option<(usize, ChoiceIdentity)>,
}

impl ChoicePopup {
    pub fn open(target: Target, scene: &Node, layout: &LayoutNode) -> Option<Self> {
        let kind = scene.at_path(&target.path)?.kind;
        if !matches!(kind, "ComboBox" | "SearchField") {
            return None;
        }
        let mut popup = Self {
            target,
            kind,
            highlight: None,
            highlighted_value: None,
            highlighted_row: None,
            first: 0,
            press: None,
        };
        let node = popup.node(scene, layout)?;
        if node.kind == "SearchField" {
            return popup.reconcile(scene, layout).then_some(popup);
        }
        let current = combo_index(node);
        let index = node
            .text("edit-text")
            .filter(|draft| combo_editable(node) && !draft.is_empty())
            .filter(|draft| combo_text(node) != *draft)
            .and_then(|draft| {
                combo_find_label(node, |item| item == draft)
                    .or_else(|| combo_find_label(node, |item| item.starts_with(draft)))
            })
            .or(current)
            .unwrap_or(0);
        popup.highlight(node, index);
        popup.reconcile(scene, layout).then_some(popup)
    }

    fn node<'a>(&self, scene: &'a Node, layout: &LayoutNode) -> Option<&'a Node> {
        if !valid(&self.target, scene, layout) {
            return None;
        }
        scene
            .at_path(&self.target.path)
            .filter(|node| node.kind == self.kind)
    }

    fn resolve<'a>(
        &self,
        scene: &'a Node,
        layout: &LayoutNode,
    ) -> Option<(&'a Node, ChoiceGeometry)> {
        let node = self.node(scene, layout)?;
        let anchor = layout.at_path(&self.target.path)?.bounds;
        Some((node, geometry(anchor, layout.bounds, choice_count(node))?))
    }

    pub fn view<'a>(&self, scene: &'a Node, layout: &LayoutNode) -> Option<ChoiceMenu<'a>> {
        let (node, geo) = self.resolve(scene, layout)?;
        let first = self.first.min(choice_count(node) - geo.visible);
        let range = first..first + geo.visible;
        Some(ChoiceMenu {
            control: node,
            bounds: geo.bounds,
            rows: if node.kind == "SearchField" {
                Cow::Borrowed(&search_suggestions(node)[range])
            } else {
                combo_labels(node, range)
            },
            first,
            highlighted: self.highlighted(node),
            selected: (node.kind == "ComboBox")
                .then(|| combo_index(node))
                .flatten(),
        })
    }

    pub fn reconcile(&mut self, scene: &Node, layout: &LayoutNode) -> bool {
        let Some((node, geo)) = self.resolve(scene, layout) else {
            return false;
        };
        let len = choice_count(node);
        self.highlight = self.highlighted(node);
        self.first = self.first.min(len - geo.visible);
        if let Some(highlight) = self.highlight {
            if highlight < self.first {
                self.first = highlight;
            } else if highlight >= self.first + geo.visible {
                self.first = highlight + 1 - geo.visible;
            }
        }
        true
    }

    fn highlighted(&self, node: &Node) -> Option<usize> {
        let index = self.highlight?;
        if node.kind == "SearchField" {
            (search_suggestion(node, index) == self.highlighted_value.as_deref()
                && self.highlighted_value.is_some())
            .then_some(index)
        } else if let Some(key) = self.highlighted_row {
            combo_row_index(node, key)
        } else {
            Some(index.min(choice_count(node).saturating_sub(1)))
        }
    }

    fn highlight(&mut self, node: &Node, index: usize) {
        if node.kind == "SearchField" {
            self.highlighted_value = search_suggestion(node, index).map(str::to_owned);
            self.highlight = self.highlighted_value.as_ref().map(|_| index);
        } else {
            self.highlighted_row = combo_row_key(node, index);
            self.highlight = Some(index);
        }
    }

    fn pick<'a>(
        &self,
        scene: &'a Node,
        layout: &LayoutNode,
        point: Point,
    ) -> Option<(&'a Node, usize)> {
        let (node, geo) = self.resolve(scene, layout)?;
        if !geo.bounds.contains(point) {
            return None;
        }
        let offset = ((point.y - geo.bounds.y) / geo.bounds.height * geo.visible as f32) as usize;
        let first = self.first.min(choice_count(node) - geo.visible);
        (offset < geo.visible).then_some((node, first + offset))
    }

    pub fn hover(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> bool {
        let Some((node, index)) = self.pick(scene, layout, point) else {
            return false;
        };
        let previous = self.highlighted(node);
        self.highlight(node, index);
        previous != self.highlight
    }

    pub fn press(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> bool {
        let Some((node, index)) = self.pick(scene, layout, point) else {
            return false;
        };
        self.highlight(node, index);
        self.press = choice_identity(node, index).map(|identity| (index, identity));
        self.press.is_some()
    }

    pub fn release(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> Option<usize> {
        let (pressed, previous) = self.press.take()?;
        let (node, index) = self.pick(scene, layout, point)?;
        (index == pressed && choice_identity(node, index).as_ref() == Some(&previous))
            .then_some(index)
    }

    pub fn move_highlight(&mut self, scene: &Node, layout: &LayoutNode, key: ChoiceKey) {
        let Some((node, geo)) = self.resolve(scene, layout) else {
            return;
        };
        let last = choice_count(node) - 1;
        let highlight = self.highlighted(node);
        let next = match key {
            ChoiceKey::Up => highlight.map_or(last, |index| index.saturating_sub(1)),
            ChoiceKey::Down => highlight.map_or(0, |index| (index + 1).min(last)),
            ChoiceKey::Home => 0,
            ChoiceKey::End => last,
            ChoiceKey::PageUp => highlight.map_or(0, |index| index.saturating_sub(geo.visible)),
            ChoiceKey::PageDown => highlight.map_or(0, |index| (index + geo.visible).min(last)),
            _ => return,
        };
        let next = if node.kind == "SearchField" {
            let selectable = |&index: &usize| search_suggestion(node, index).is_some();
            let lower = || (0..=next).rev().find(selectable);
            let higher = || (next..=last).find(selectable);
            let candidate = if matches!(key, ChoiceKey::Up | ChoiceKey::End | ChoiceKey::PageUp) {
                lower().or_else(higher)
            } else {
                higher().or_else(lower)
            };
            let Some(index) = candidate else {
                return;
            };
            index
        } else {
            next
        };
        self.highlight(node, next);
        self.reconcile(scene, layout);
    }

    pub fn selected(&self, scene: &Node, layout: &LayoutNode) -> Option<usize> {
        self.highlighted(self.resolve(scene, layout)?.0)
    }

    pub fn scroll(&mut self, scene: &Node, layout: &LayoutNode, rows: isize) {
        let Some((node, geo)) = self.resolve(scene, layout) else {
            return;
        };
        self.first = self
            .first
            .saturating_add_signed(rows)
            .min(choice_count(node) - geo.visible);
        if let Some(highlight) = self.highlighted(node) {
            self.highlight(
                node,
                highlight.clamp(self.first, self.first + geo.visible - 1),
            );
        }
        self.press = None;
    }
}

fn choice_count(node: &Node) -> usize {
    if node.kind == "SearchField" {
        search_suggestions(node).len()
    } else {
        combo_count(node)
    }
}
