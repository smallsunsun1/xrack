use crate::{LayoutNode, Point, Rect, Target, input::valid};
use unicode_segmentation::UnicodeSegmentation;
use xrack_runtime::{Node, combo_editable, combo_index, search_suggestion, search_suggestions};

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

pub(crate) fn completion_value<'a>(model: &'a [String], draft: &str) -> Option<&'a str> {
    if draft.is_empty() || model.iter().any(|value| value == draft) {
        return None;
    }
    model.iter().find_map(|value| {
        let suffix = value.strip_prefix(draft)?;
        (suffix.chars().all(|ch| !ch.is_control())
            && value
                .grapheme_indices(true)
                .any(|(index, _)| index == draft.len()))
        .then_some(value.as_str())
    })
}

#[cfg(test)]
mod tests {
    use super::completion_value;

    #[test]
    fn completion_skips_partial_graphemes_and_single_line_control_characters() {
        let model = ["e\u{301}lan", "edit\n", "editor"].map(str::to_owned);
        assert_eq!(completion_value(&model, "e"), Some("editor"));
        let model = ["editor", "e"].map(str::to_owned);
        assert_eq!(completion_value(&model, "e"), None);
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
    pub bounds: Rect,
    pub rows: &'a [String],
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

    pub(crate) fn pick(&self, point: Point) -> Option<usize> {
        if !self.bounds.contains(point) {
            return None;
        }
        let offset =
            ((point.y - self.bounds.y) / self.bounds.height * self.rows.len() as f32) as usize;
        (offset < self.rows.len()).then_some(self.first + offset)
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
    first: usize,
    press: Option<(usize, String)>,
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
            first: 0,
            press: None,
        };
        let node = popup.node(scene, layout)?;
        let model = choice_values(node);
        if node.kind == "SearchField" {
            return popup.reconcile(scene, layout).then_some(popup);
        }
        let current = combo_index(node);
        popup.highlight = Some(
            node.text("edit-text")
                .filter(|draft| combo_editable(node) && !draft.is_empty())
                .filter(|draft| current.is_none_or(|index| model[index] != *draft))
                .and_then(|draft| {
                    model
                        .iter()
                        .position(|item| item == draft)
                        .or_else(|| model.iter().position(|item| item.starts_with(draft)))
                })
                .or(current)
                .unwrap_or(0),
        );
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
        Some((
            node,
            geometry(anchor, layout.bounds, choice_values(node).len())?,
        ))
    }

    pub fn view<'a>(&self, scene: &'a Node, layout: &LayoutNode) -> Option<ChoiceMenu<'a>> {
        let (node, geo) = self.resolve(scene, layout)?;
        let model = choice_values(node);
        let first = self.first.min(model.len() - geo.visible);
        Some(ChoiceMenu {
            bounds: geo.bounds,
            rows: &model[first..first + geo.visible],
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
        let len = choice_values(node).len();
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
        } else {
            Some(index.min(choice_values(node).len().saturating_sub(1)))
        }
    }

    fn highlight(&mut self, node: &Node, index: usize) {
        if node.kind == "SearchField" {
            self.highlighted_value = search_suggestion(node, index).map(str::to_owned);
            self.highlight = self.highlighted_value.as_ref().map(|_| index);
        } else {
            self.highlight = Some(index);
        }
    }

    pub fn hover(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> bool {
        let Some(index) = self.view(scene, layout).and_then(|menu| menu.pick(point)) else {
            return false;
        };
        let Some(node) = self.node(scene, layout) else {
            return false;
        };
        let previous = self.highlighted(node);
        self.highlight(node, index);
        previous != self.highlight
    }

    pub fn press(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> bool {
        let Some(menu) = self.view(scene, layout) else {
            return false;
        };
        let Some(index) = menu.pick(point) else {
            return false;
        };
        let value = menu.rows[index - menu.first].clone();
        let Some(node) = self.node(scene, layout) else {
            return false;
        };
        self.highlight(node, index);
        self.press = self.highlight.map(|index| (index, value));
        self.press.is_some()
    }

    pub fn release(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> Option<usize> {
        let (pressed, previous) = self.press.take()?;
        let menu = self.view(scene, layout)?;
        let index = menu.pick(point)?;
        (index == pressed && menu.rows[index - menu.first] == previous).then_some(index)
    }

    pub fn move_highlight(&mut self, scene: &Node, layout: &LayoutNode, key: ChoiceKey) {
        let Some((node, geo)) = self.resolve(scene, layout) else {
            return;
        };
        let last = choice_values(node).len() - 1;
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
        self.view(scene, layout).and_then(|menu| menu.highlighted)
    }

    pub fn scroll(&mut self, scene: &Node, layout: &LayoutNode, rows: isize) {
        let Some((node, geo)) = self.resolve(scene, layout) else {
            return;
        };
        self.first = self
            .first
            .saturating_add_signed(rows)
            .min(choice_values(node).len() - geo.visible);
        if let Some(highlight) = self.highlighted(node) {
            self.highlight(
                node,
                highlight.clamp(self.first, self.first + geo.visible - 1),
            );
        }
        self.press = None;
    }
}

fn choice_values(node: &Node) -> &[String] {
    if node.kind == "SearchField" {
        search_suggestions(node)
    } else {
        node.strings("model")
    }
}
