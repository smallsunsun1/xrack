use crate::{LayoutNode, MenuShortcut, Point, Rect, Target, input::valid};
use xrack_runtime::{Image, Node, is_window_overlay};

const ROW_HEIGHT: f32 = 28.0;
const MAX_VISIBLE_ROWS: usize = 14;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKey {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Accept,
    Escape,
}

pub struct MenuRow<'a> {
    pub node: &'a Node,
    pub label: &'a str,
    pub icon: Option<Image>,
    pub shortcut: Option<String>,
    pub enabled: bool,
    pub separator: bool,
    pub submenu: bool,
    pub checked: Option<bool>,
    index: usize,
}

pub struct MenuPanel<'a> {
    pub bounds: Rect,
    pub rows: Vec<MenuRow<'a>>,
    pub highlighted: Option<usize>,
    icon_column: bool,
    path: Vec<usize>,
}

/// Identity of a displayed menu row, including the label used to guard late actions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuEntry {
    pub path: Vec<usize>,
    pub label: String,
    pub kind: &'static str,
}

impl MenuPanel<'_> {
    pub fn path(&self) -> &[usize] {
        &self.path
    }

    pub fn row_index(&self, slot: usize) -> usize {
        self.rows[slot].index
    }

    pub fn entry(&self, slot: usize) -> MenuEntry {
        let row = &self.rows[slot];
        let mut path = self.path.clone();
        path.push(row.index);
        MenuEntry {
            path,
            label: row.label.to_owned(),
            kind: row.node.kind,
        }
    }

    pub fn label_offset(&self) -> f32 {
        if self.icon_column { 48.0 } else { 26.0 }
    }

    pub fn icon_bounds(&self, slot: usize) -> Rect {
        let row = self.row_bounds(slot);
        let size = 16.0_f32.min(row.height).min((row.width - 26.0).max(0.0));
        Rect::new(row.x + 24.0, row.y + (row.height - size) * 0.5, size, size)
    }
    pub fn row_highlighted(&self, slot: usize) -> bool {
        self.highlighted == Some(self.rows[slot].index)
    }
    pub fn row_bounds(&self, slot: usize) -> Rect {
        let height = self.bounds.height / self.rows.len() as f32;
        Rect::new(
            self.bounds.x,
            self.bounds.y + slot as f32 * height,
            self.bounds.width,
            height,
        )
    }

    fn pick(&self, point: Point) -> Option<&MenuRow<'_>> {
        if !self.bounds.contains(point) {
            return None;
        }
        let slot =
            ((point.y - self.bounds.y) / self.bounds.height * self.rows.len() as f32) as usize;
        self.rows.get(slot)
    }
}

#[derive(Clone, Debug)]
struct Level {
    path: Vec<usize>,
    first: usize,
    highlight: Option<usize>,
}

#[derive(Clone, Debug)]
struct PressedItem {
    level: usize,
    index: usize,
    label: String,
    kind: &'static str,
}

#[derive(Clone, Debug)]
pub(crate) struct MenuPopup {
    pub trigger: Target,
    point: Option<Point>,
    levels: Vec<Level>,
    pressed: Option<PressedItem>,
}

fn rows(menu: &Node) -> impl Iterator<Item = (usize, &Node)> {
    menu.children.iter().enumerate().filter(|(_, row)| {
        row.is_visible() && matches!(row.kind, "Menu" | "MenuItem" | "MenuSeparator")
    })
}

fn enabled(row: &Node) -> bool {
    row.kind != "MenuSeparator" && row.boolean("enabled", true)
}

fn label(row: &Node) -> &str {
    if row.kind == "Menu" {
        row.text("title").unwrap_or("")
    } else {
        row.text("text").unwrap_or("")
    }
}

fn shortcut(row: &Node) -> Option<MenuShortcut> {
    row.text("shortcut")?.parse().ok()
}

fn icon(row: &Node) -> Option<Image> {
    row.image("icon").filter(|image| !image.size().contains(&0))
}

fn visit_menu_shortcuts(
    node: &Node,
    path: &mut Vec<usize>,
    visit: &mut impl FnMut(&Node, &[usize]),
) {
    if !node.is_visible() || !enabled(node) {
        return;
    }
    if node.kind == "MenuItem" {
        visit(node, path);
        return;
    }
    for (index, row) in rows(node) {
        path.push(index);
        visit_menu_shortcuts(row, path, visit);
        path.pop();
    }
}

fn visit_presentations(
    node: &Node,
    layout: &LayoutNode,
    path: &mut Vec<usize>,
    visit: &mut impl FnMut(&Node, &[usize]),
) {
    if !layout.visible || !layout.enabled {
        return;
    }
    if node.kind == "Shortcut" {
        visit(node, path);
        return;
    }
    let visible = layout.bounds.intersect(layout.clip);
    if visible.width <= 0.0 || visible.height <= 0.0 {
        return;
    }
    if matches!(node.kind, "Button" | "ToolButton") && node.action.is_some() {
        visit(node, path);
    }
    for (index, (child, item)) in node.children.iter().zip(&layout.children).enumerate() {
        if is_window_overlay(child.kind) {
            continue;
        }
        path.push(index);
        if node.kind == "MenuBar" {
            let visible = item.bounds.intersect(item.clip);
            if item.visible && item.enabled && visible.width > 0.0 && visible.height > 0.0 {
                visit_menu_shortcuts(child, path, visit);
            }
        } else {
            visit_presentations(child, item, path, visit);
        }
        path.pop();
    }
}

pub(crate) fn visit_shortcut_presentations(
    scene: &Node,
    layout: &LayoutNode,
    scope: &[usize],
    popup: Option<&MenuPopup>,
    mut visit: impl FnMut(&Node, &[usize]),
) {
    if let Some(popup) = popup.filter(|popup| popup.point.is_some()) {
        if popup.trigger.path.starts_with(scope)
            && valid(&popup.trigger, scene, layout)
            && let Some(level) = popup.levels.first()
            && let Some(node) = scene.interactive_at_path(&level.path)
        {
            visit_menu_shortcuts(node, &mut level.path.clone(), &mut visit);
        }
    } else if let Some((node, item)) = scene.at_path(scope).zip(layout.at_path(scope)) {
        visit_presentations(node, item, &mut scope.to_vec(), &mut visit);
    }
}

fn panel_bounds(anchor: Rect, viewport: Rect, width: f32, height: f32, submenu: bool) -> Rect {
    let right = viewport.x + viewport.width;
    let bottom = viewport.y + viewport.height;
    let (x, y) = if submenu {
        let next = anchor.x + anchor.width;
        (
            if next + width <= right {
                next
            } else {
                anchor.x - width
            },
            anchor.y,
        )
    } else {
        let next = anchor.y + anchor.height;
        (
            anchor.x,
            if next + height <= bottom || next - viewport.y <= bottom - anchor.y {
                next
            } else {
                anchor.y - height
            },
        )
    };
    Rect::new(
        x.clamp(viewport.x, right - width),
        y.clamp(viewport.y, bottom - height),
        width,
        height,
    )
}

impl MenuPopup {
    pub fn open(
        trigger: Target,
        path: Vec<usize>,
        point: Option<Point>,
        scene: &Node,
        layout: &LayoutNode,
    ) -> Option<Self> {
        let mut popup = Self {
            trigger,
            point,
            levels: vec![Level {
                path,
                first: 0,
                highlight: None,
            }],
            pressed: None,
        };
        popup.reconcile(scene, layout).then_some(popup)
    }

    pub fn bar_header(&self) -> Option<&[usize]> {
        self.point.is_none().then_some(&self.trigger.path)
    }
    pub fn at_root(&self) -> bool {
        self.levels.len() == 1
    }

    fn visible_count(viewport: Rect, count: usize) -> usize {
        ((viewport.height / ROW_HEIGHT) as usize)
            .clamp(1, MAX_VISIBLE_ROWS)
            .min(count)
    }

    pub fn panels<'a>(&self, scene: &'a Node, layout: &LayoutNode) -> Vec<MenuPanel<'a>> {
        let viewport = layout.clip.inset(4.0);
        if viewport.width <= 0.0 || viewport.height <= 0.0 || !valid(&self.trigger, scene, layout) {
            return Vec::new();
        }
        let mut anchor = self.point.map_or_else(
            || layout.at_path(&self.trigger.path).unwrap().bounds,
            |point| Rect::new(point.x, point.y, 0.0, 0.0),
        );
        let mut panels = Vec::new();
        for (index, level) in self.levels.iter().enumerate() {
            let Some(menu) = scene
                .interactive_at_path(&level.path)
                .filter(|menu| menu.kind == "Menu")
            else {
                break;
            };
            let count = rows(menu).count();
            if count == 0 {
                break;
            }
            let visible = Self::visible_count(viewport, count);
            let first = level.first.min(count - visible);
            let icon_column = rows(menu).any(|(_, row)| icon(row).is_some());
            let width = rows(menu)
                .map(|(_, row)| {
                    let shortcut_width =
                        shortcut(row).map_or(0.0, |key| key.to_string().len() as f32 * 8.0 + 14.0);
                    label(row)
                        .chars()
                        .map(|ch| if ch.is_ascii() { 8.0 } else { 16.0 })
                        .sum::<f32>()
                        + 54.0
                        + if icon_column { 22.0 } else { 0.0 }
                        + shortcut_width
                })
                .fold(174.0, f32::max)
                .min(viewport.width);
            let bounds = panel_bounds(
                anchor,
                viewport,
                width,
                (visible as f32 * ROW_HEIGHT).min(viewport.height),
                index > 0,
            );
            let panel = MenuPanel {
                bounds,
                highlighted: level.highlight,
                icon_column,
                path: level.path.clone(),
                rows: rows(menu)
                    .skip(first)
                    .take(visible)
                    .map(|(index, row)| MenuRow {
                        index,
                        node: row,
                        label: label(row),
                        icon: icon(row),
                        shortcut: shortcut(row).map(|key| key.to_string()),
                        enabled: enabled(row),
                        separator: row.kind == "MenuSeparator",
                        submenu: row.kind == "Menu",
                        checked: row
                            .boolean("checkable", false)
                            .then(|| row.boolean("checked", false)),
                    })
                    .collect(),
            };
            let next = self
                .levels
                .get(index + 1)
                .and_then(|child| child.path.last())
                .copied();
            let slot = panel
                .rows
                .iter()
                .position(|row| Some(row.index) == next && row.submenu && row.enabled);
            if let Some(slot) = slot {
                anchor = panel.row_bounds(slot);
            }
            panels.push(panel);
            if slot.is_none() {
                break;
            }
        }
        panels
    }

    pub fn reconcile(&mut self, scene: &Node, layout: &LayoutNode) -> bool {
        let viewport = layout.clip.inset(4.0);
        for level in &mut self.levels {
            let Some(menu) = scene.at_path(&level.path) else {
                break;
            };
            if !level.highlight.is_some_and(|index| {
                menu.children
                    .get(index)
                    .is_some_and(|row| row.is_visible() && enabled(row))
            }) {
                level.highlight = rows(menu)
                    .find(|(_, row)| enabled(row))
                    .map(|(index, _)| index);
            }
            let count = rows(menu).count();
            let visible = Self::visible_count(viewport, count);
            level.first = level.first.min(count.saturating_sub(visible));
            if let Some(slot) = rows(menu).position(|(index, _)| Some(index) == level.highlight) {
                level.first = level
                    .first
                    .max(slot.saturating_add(1).saturating_sub(visible))
                    .min(slot);
            }
        }
        let count = self.panels(scene, layout).len();
        self.levels.truncate(count);
        count != 0
    }

    pub fn hover(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> bool {
        let panels = self.panels(scene, layout);
        let Some((level, row)) = panels
            .iter()
            .enumerate()
            .rev()
            .find_map(|(level, panel)| panel.pick(point).map(|row| (level, row)))
        else {
            return false;
        };
        if !row.enabled || self.levels[level].highlight == Some(row.index) {
            return false;
        }
        self.levels.truncate(level + 1);
        self.levels[level].highlight = Some(row.index);
        true
    }

    pub fn press(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> bool {
        self.pressed = None;
        self.hover(scene, layout, point);
        let panels = self.panels(scene, layout);
        let Some((level, row)) = panels
            .iter()
            .enumerate()
            .rev()
            .find_map(|(level, panel)| panel.pick(point).map(|row| (level, row)))
        else {
            return false;
        };
        if row.enabled {
            self.pressed = Some(PressedItem {
                level,
                index: row.index,
                label: row.label.into(),
                kind: row.node.kind,
            });
        }
        true
    }

    pub fn release(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
    ) -> Option<Vec<usize>> {
        let pressed = self.pressed.take()?;
        let panels = self.panels(scene, layout);
        let row = panels.get(pressed.level)?.pick(point)?;
        if !row.enabled
            || row.index != pressed.index
            || row.label != pressed.label
            || row.node.kind != pressed.kind
        {
            return None;
        }
        self.activate(scene, layout, pressed.level, pressed.index)
    }

    fn activate(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        level: usize,
        index: usize,
    ) -> Option<Vec<usize>> {
        let mut path = self.levels[level].path.clone();
        path.push(index);
        let node = scene.interactive_at_path(&path)?;
        match node.kind {
            "MenuItem" => Some(path),
            "Menu" => {
                self.levels.truncate(level + 1);
                self.levels.push(Level {
                    path,
                    first: 0,
                    highlight: None,
                });
                self.reconcile(scene, layout);
                None
            }
            _ => None,
        }
    }

    pub fn accessibility_entry(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        trigger: &Target,
        entry: &MenuEntry,
        activate: bool,
    ) -> Option<Vec<usize>> {
        if &self.trigger != trigger {
            return None;
        }
        let (&index, parent) = entry.path.split_last()?;
        let depth = self.panels(scene, layout).iter().position(|panel| {
            panel.path == parent
                && panel.rows.iter().any(|row| {
                    row.enabled
                        && row.index == index
                        && row.label == entry.label
                        && row.node.kind == entry.kind
                })
        })?;
        self.pressed = None;
        self.levels.truncate(depth + 1);
        self.levels[depth].highlight = Some(index);
        if activate {
            self.activate(scene, layout, depth, index)
        } else {
            None
        }
    }

    pub fn accessibility_close(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        trigger: &Target,
        path: &[usize],
    ) -> Option<bool> {
        if &self.trigger != trigger {
            return None;
        }
        let depth = self
            .panels(scene, layout)
            .iter()
            .position(|panel| panel.path == path)?;
        self.pressed = None;
        self.levels.truncate(depth);
        Some(depth == 0)
    }

    pub fn enter_submenu(&mut self, scene: &Node, layout: &LayoutNode) -> bool {
        let Some(level) = self.levels.last() else {
            return false;
        };
        let Some(index) = level.highlight else {
            return false;
        };
        if !scene
            .at_path(&level.path)
            .and_then(|menu| menu.children.get(index))
            .is_some_and(|row| row.kind == "Menu" && row.is_visible() && enabled(row))
        {
            return false;
        }
        self.activate(scene, layout, self.levels.len() - 1, index);
        true
    }

    pub fn key(&mut self, scene: &Node, layout: &LayoutNode, key: MenuKey) -> Option<Vec<usize>> {
        let depth = self.levels.len().checked_sub(1)?;
        let level = &mut self.levels[depth];
        let menu = scene.at_path(&level.path)?;
        match key {
            MenuKey::Accept => {
                let index = level.highlight?;
                return self.activate(scene, layout, depth, index);
            }
            MenuKey::Left | MenuKey::Escape if depth > 0 => {
                self.levels.pop();
            }
            MenuKey::Up | MenuKey::Down | MenuKey::Home | MenuKey::End => {
                let choices: Vec<_> = rows(menu)
                    .filter(|(_, row)| enabled(row))
                    .map(|(index, _)| index)
                    .collect();
                if choices.is_empty() {
                    return None;
                }
                let current = choices
                    .iter()
                    .position(|index| Some(*index) == level.highlight);
                let next = match key {
                    MenuKey::Up => current.map_or(choices.len() - 1, |current| {
                        (current + choices.len() - 1) % choices.len()
                    }),
                    MenuKey::Down => current.map_or(0, |current| (current + 1) % choices.len()),
                    MenuKey::End => choices.len() - 1,
                    _ => 0,
                };
                level.highlight = Some(choices[next]);
                self.reconcile(scene, layout);
            }
            _ => {}
        }
        None
    }

    pub fn scroll(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        point: Option<Point>,
        delta: isize,
    ) {
        let panels = self.panels(scene, layout);
        let Some(depth) = panels
            .iter()
            .rposition(|panel| point.is_some_and(|point| panel.bounds.contains(point)))
            .or_else(|| panels.len().checked_sub(1))
        else {
            return;
        };
        let level = &mut self.levels[depth];
        let Some(menu) = scene.at_path(&level.path) else {
            return;
        };
        let count = rows(menu).count();
        let visible = Self::visible_count(layout.clip.inset(4.0), count);
        level.first = level
            .first
            .saturating_add_signed(delta)
            .min(count - visible);
        level.highlight = rows(menu)
            .skip(level.first)
            .take(visible)
            .find(|(_, row)| enabled(row))
            .map(|(index, _)| index);
        self.levels.truncate(depth + 1);
    }
}
