use crate::{LayoutNode, Point, Rect, Target, input::valid};
use xrack_runtime::{Node, combo_index};

const ROW_HEIGHT: f32 = 28.0;
const MAX_ROWS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChoiceKey {
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Accept,
    Escape,
}

pub struct ChoiceMenu<'a> {
    pub bounds: Rect,
    pub rows: &'a [String],
    pub first: usize,
    pub highlighted: usize,
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
    highlight: usize,
    first: usize,
    press: Option<(usize, String)>,
}

impl ChoicePopup {
    pub fn open(target: Target, scene: &Node, layout: &LayoutNode) -> Option<Self> {
        let mut popup = Self {
            target,
            highlight: 0,
            first: 0,
            press: None,
        };
        let node = popup.node(scene, layout)?;
        popup.highlight = combo_index(node).unwrap_or(0);
        popup.reconcile(scene, layout).then_some(popup)
    }

    fn node<'a>(&self, scene: &'a Node, layout: &LayoutNode) -> Option<&'a Node> {
        if !valid(&self.target, scene, layout) {
            return None;
        }
        scene
            .at_path(&self.target.path)
            .filter(|node| node.kind == "ComboBox")
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
            geometry(anchor, layout.bounds, node.strings("model").len())?,
        ))
    }

    pub fn view<'a>(&self, scene: &'a Node, layout: &LayoutNode) -> Option<ChoiceMenu<'a>> {
        let (node, geo) = self.resolve(scene, layout)?;
        let model = node.strings("model");
        let first = self.first.min(model.len() - geo.visible);
        Some(ChoiceMenu {
            bounds: geo.bounds,
            rows: &model[first..first + geo.visible],
            first,
            highlighted: self.highlight.min(model.len() - 1),
            selected: combo_index(node),
        })
    }

    pub fn reconcile(&mut self, scene: &Node, layout: &LayoutNode) -> bool {
        let Some((node, geo)) = self.resolve(scene, layout) else {
            return false;
        };
        let len = node.strings("model").len();
        self.highlight = self.highlight.min(len - 1);
        self.first = self.first.min(len - geo.visible);
        if self.highlight < self.first {
            self.first = self.highlight;
        } else if self.highlight >= self.first + geo.visible {
            self.first = self.highlight + 1 - geo.visible;
        }
        true
    }

    pub fn hover(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> bool {
        if let Some(index) = self.view(scene, layout).and_then(|menu| menu.pick(point)) {
            let changed = self.highlight != index;
            self.highlight = index;
            changed
        } else {
            false
        }
    }

    pub fn press(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> bool {
        let Some(menu) = self.view(scene, layout) else {
            return false;
        };
        let Some(index) = menu.pick(point) else {
            return false;
        };
        self.press = Some((index, menu.rows[index - menu.first].clone()));
        self.highlight = index;
        true
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
        let last = node.strings("model").len() - 1;
        self.highlight = match key {
            ChoiceKey::Up => self.highlight.saturating_sub(1),
            ChoiceKey::Down => (self.highlight + 1).min(last),
            ChoiceKey::Home => 0,
            ChoiceKey::End => last,
            ChoiceKey::PageUp => self.highlight.saturating_sub(geo.visible),
            ChoiceKey::PageDown => (self.highlight + geo.visible).min(last),
            _ => self.highlight,
        };
        self.reconcile(scene, layout);
    }

    pub fn selected(&self, scene: &Node, layout: &LayoutNode) -> Option<usize> {
        self.view(scene, layout).map(|menu| menu.highlighted)
    }

    pub fn scroll(&mut self, scene: &Node, layout: &LayoutNode, rows: isize) {
        let Some((node, geo)) = self.resolve(scene, layout) else {
            return;
        };
        self.first = self
            .first
            .saturating_add_signed(rows)
            .min(node.strings("model").len() - geo.visible);
        self.highlight = self
            .highlight
            .clamp(self.first, self.first + geo.visible - 1);
        self.press = None;
    }
}
