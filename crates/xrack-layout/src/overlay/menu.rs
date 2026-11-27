use crate::input::ElementId;
use crate::item::menu::{MenuRole, enabled, entries as rows, label, menu_item, role};
use crate::{
    ControlInput, LayoutNode, MenuShortcut, Point, Rect, Target, edit::SelectionState,
    input::valid, place_in_direction, popup::PopupTypography, window_safe_area,
};
use std::sync::atomic::{AtomicU64, Ordering};
use xrack_runtime::{
    ControlAction, ImageAsset, LayoutDirection, Node, TextCommand, TextCommandAvailability,
    is_window_overlay,
};

const ROW_HEIGHT: f32 = 28.0;
const TOUCH_ROW_HEIGHT: f32 = 44.0;
const MAX_VISIBLE_ROWS: usize = 14;
const DEFAULT_MENU_FONT_SIZE: f32 = 15.0;

fn menu_viewport(scene: &Node, layout: &LayoutNode) -> Rect {
    window_safe_area(scene, layout.clip).inset(4.0)
}

fn row_height(menu: &Node, touch: bool, typography: PopupTypography) -> f32 {
    let minimum = if touch { TOUCH_ROW_HEIGHT } else { ROW_HEIGHT };
    rows(menu)
        .filter(|(_, row)| role(row) != Some(MenuRole::Separator))
        .map(|(_, row)| typography.size(row, DEFAULT_MENU_FONT_SIZE) * 1.4 + 6.0)
        .fold(minimum, f32::max)
}

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
    pub font_size: f32,
    pub icon: Option<ImageAsset>,
    pub shortcut: Option<String>,
    pub enabled: bool,
    pub separator: bool,
    pub submenu: bool,
    pub checked: Option<bool>,
    index: usize,
}

pub struct MenuPanel<'a> {
    pub menu: &'a Node,
    pub bounds: Rect,
    pub layout_direction: LayoutDirection,
    pub rows: Vec<MenuRow<'a>>,
    pub highlighted: Option<usize>,
    icon_column: bool,
    first: usize,
    row_count: usize,
    path: Vec<usize>,
    session: u64,
}

/// Identity of a displayed menu row, including the label used to guard late actions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuEntry {
    pub path: Vec<usize>,
    pub label: String,
    pub kind: &'static str,
    session: u64,
}

impl MenuPanel<'_> {
    pub fn path(&self) -> &[usize] {
        &self.path
    }

    pub fn session(&self) -> u64 {
        self.session
    }

    pub fn can_scroll(&self, backward: bool) -> bool {
        if backward {
            self.first > 0
        } else {
            self.first + self.rows.len() < self.row_count
        }
    }

    #[doc(hidden)]
    pub fn visible_range(&self) -> std::ops::Range<usize> {
        self.first..self.first + self.rows.len()
    }

    #[doc(hidden)]
    pub fn row_count(&self) -> usize {
        self.row_count
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
            session: self.session,
        }
    }

    pub fn label_offset(&self) -> f32 {
        if self.icon_column { 48.0 } else { 26.0 }
    }

    pub fn row_content_bounds(&self, slot: usize, start: f32, width: f32) -> Rect {
        let row = self.row_bounds(slot);
        let item = Rect::new(row.x + start, row.y, width.max(0.0), row.height);
        place_in_direction(item, row, self.layout_direction)
    }

    pub fn icon_bounds(&self, slot: usize) -> Rect {
        let row = self.row_bounds(slot);
        let size = 16.0_f32.min(row.height).min((row.width - 26.0).max(0.0));
        let column = self.row_content_bounds(slot, 24.0, size);
        Rect::new(column.x, row.y + (row.height - size) * 0.5, size, size)
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
    scrolled: bool,
}

#[derive(Clone, Debug)]
struct PressedItem {
    level: usize,
    index: usize,
    label: String,
    kind: &'static str,
    id: Option<ElementId>,
    command: Option<xrack_runtime::ActionKey>,
}

#[derive(Clone, Debug)]
pub(crate) struct MenuPopup {
    pub trigger: Target,
    point: Option<Point>,
    levels: Vec<Level>,
    pressed: Option<PressedItem>,
    pub session: u64,
    pub text: Option<TextMenu>,
    pub(crate) touch: bool,
}

/// A text menu presented by a native host instead of the scene renderer.
#[doc(hidden)]
pub struct NativeTextMenu(pub(crate) MenuPopup);

impl NativeTextMenu {
    pub fn session(&self) -> u64 {
        self.0.session
    }

    pub fn entries(&self) -> impl Iterator<Item = (usize, &'static str)> + '_ {
        self.0.text.iter().flat_map(|text| {
            TEXT_ENTRIES
                .iter()
                .zip(&text.menu.children)
                .enumerate()
                .filter_map(|(index, (entry, row))| {
                    let (_, label, _) = (*entry)?;
                    enabled(row).then_some((index, label))
                })
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct MenuPan {
    session: u64,
    path: Vec<usize>,
    remainder: f32,
}

impl MenuPan {
    pub(crate) fn advance(
        &mut self,
        menu: &mut MenuPopup,
        scene: &Node,
        layout: &LayoutNode,
        delta: f32,
    ) -> bool {
        if menu.session != self.session {
            return false;
        }
        let Some(depth) = menu.levels.iter().position(|level| level.path == self.path) else {
            return false;
        };
        let Some(node) = menu_at(scene, menu.text.as_ref(), &self.path) else {
            return false;
        };
        let height = menu_viewport(scene, layout).height.max(1.0);
        let row_height = row_height(node, menu.touch, layout.popup_typography()).min(height);
        self.remainder += delta;
        let rows = (self.remainder / row_height).trunc() as isize;
        self.remainder %= row_height;
        rows != 0 && menu.scroll_level(scene, layout, depth, rows, false)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TextMenu {
    menu: Node,
    pub value: String,
    pub selection: SelectionState,
    pub kind: &'static str,
    pub writable: bool,
    pub password: bool,
    pub commands: TextCommandAvailability,
}

const TEXT_ENTRIES: [Option<(TextCommand, &str, &str)>; 8] = [
    Some((TextCommand::Undo, "Undo", "Primary+Z")),
    Some((TextCommand::Redo, "Redo", "Primary+Shift+Z")),
    None,
    Some((TextCommand::Cut, "Cut", "Primary+X")),
    Some((TextCommand::Copy, "Copy", "Primary+C")),
    Some((TextCommand::Paste, "Paste", "Primary+V")),
    None,
    Some((TextCommand::SelectAll, "Select All", "Primary+A")),
];

impl TextMenu {
    pub fn new(
        node: &Node,
        value: String,
        selection: SelectionState,
        commands: TextCommandAvailability,
    ) -> Self {
        let writable = !node.text_read_only();
        let password = node.is_password();
        let mut menu = Node::new("Menu", None);
        menu.properties
            .insert("title", "Text editing".to_owned().into());
        for entry in TEXT_ENTRIES {
            let Some((command, label, shortcut)) = entry else {
                menu.children.push(Node::new("MenuSeparator", None));
                continue;
            };
            let enabled = match command {
                TextCommand::Undo => commands.undo,
                TextCommand::Redo => commands.redo,
                TextCommand::Cut => commands.cut,
                TextCommand::Copy => commands.copy,
                TextCommand::Paste => writable,
                TextCommand::SelectAll => !value.is_empty(),
                _ => unreachable!(),
            };
            let mut item = Node::new("MenuItem", None);
            item.properties.insert("text", label.to_owned().into());
            item.properties
                .insert("shortcut", shortcut.to_owned().into());
            item.properties.insert("enabled", enabled.into());
            menu.children.push(item);
        }
        Self {
            menu,
            value,
            selection,
            kind: node.kind,
            writable,
            password,
            commands,
        }
    }
}

fn menu_at<'a>(scene: &'a Node, text: Option<&'a TextMenu>, path: &[usize]) -> Option<&'a Node> {
    match text {
        Some(text) => Some(&text.menu),
        None => scene.at_path(path),
    }
}

fn shortcut(row: &Node) -> Option<MenuShortcut> {
    row.text("shortcut")?.parse().ok()
}

fn icon(row: &Node) -> Option<ImageAsset> {
    row.image_asset("icon")
        .filter(|image| !image.size().contains(&0))
}

fn visit_menu_shortcuts(
    node: &Node,
    path: &mut Vec<usize>,
    visit: &mut impl FnMut(&Node, &[usize]),
) {
    if !node.is_visible() || !enabled(node) {
        return;
    }
    if role(node) == Some(MenuRole::Action) {
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
    if !layout.visible || !layout.enabled || Node::SCROLL_PARTS.contains(&node.kind) {
        return;
    }
    if node.kind == "Shortcut" {
        visit(node, path);
        return;
    }
    let visible = layout.clip_rect(layout.bounds);
    if visible.width <= 0.0 || visible.height <= 0.0 {
        return;
    }
    if matches!(node.kind, "Button" | "ToolButton" | "RoundButton") && node.action.is_some() {
        visit(node, path);
    }
    for (index, child, item) in layout.children_in_scene_order(node) {
        if is_window_overlay(child.kind) {
            continue;
        }
        path.push(index);
        if role(node) == Some(MenuRole::Bar) {
            let visible = item.clip_rect(item.bounds);
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
            && popup.text.is_none()
            && let Some(node) = scene.interactive_at_path(&level.path)
        {
            visit_menu_shortcuts(node, &mut level.path.clone(), &mut visit);
        }
    } else if let Some((node, item)) = scene.at_path(scope).zip(layout.at_path(scope)) {
        visit_presentations(node, item, &mut scope.to_vec(), &mut visit);
    }
}

fn panel_bounds(
    anchor: Rect,
    viewport: Rect,
    width: f32,
    height: f32,
    submenu: bool,
    direction: LayoutDirection,
) -> Rect {
    let right = viewport.x + viewport.width;
    let bottom = viewport.y + viewport.height;
    let rtl = direction == LayoutDirection::RightToLeft;
    let (x, y) = if submenu {
        let edges = [anchor.x - width, anchor.x + anchor.width];
        let spaces = [anchor.x - viewport.x, right - anchor.x - anchor.width];
        let preferred = usize::from(!rtl);
        let opposite = 1 - preferred;
        let side = if spaces[preferred] < width && spaces[opposite] > spaces[preferred] {
            opposite
        } else {
            preferred
        };
        (edges[side], anchor.y)
    } else {
        let next = anchor.y + anchor.height;
        (
            if rtl {
                anchor.x + anchor.width - width
            } else {
                anchor.x
            },
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
        text: Option<TextMenu>,
        touch: bool,
    ) -> Option<Self> {
        static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
        let mut popup = Self {
            trigger,
            point,
            levels: vec![Level {
                path,
                first: 0,
                highlight: None,
                scrolled: false,
            }],
            pressed: None,
            session: NEXT_SESSION.fetch_add(1, Ordering::Relaxed),
            text,
            touch,
        };
        popup.reconcile(scene, layout).then_some(popup)
    }

    pub fn bar_header(&self) -> Option<&[usize]> {
        self.point.is_none().then_some(&self.trigger.path)
    }
    pub fn at_root(&self) -> bool {
        self.levels.len() == 1
    }

    fn trigger_direction(&self, layout: &LayoutNode) -> LayoutDirection {
        layout
            .at_path(&self.trigger.path)
            .map_or(LayoutDirection::LeftToRight, |item| item.layout_direction)
    }

    pub fn forward_key(&self, scene: &Node, layout: &LayoutNode) -> MenuKey {
        let direction = self
            .levels
            .iter()
            .rev()
            .find_map(|level| menu_at(scene, self.text.as_ref(), &level.path)?.layout_direction())
            .unwrap_or_else(|| self.trigger_direction(layout));
        if direction == LayoutDirection::RightToLeft {
            MenuKey::Left
        } else {
            MenuKey::Right
        }
    }

    fn visible_count(viewport: Rect, count: usize, row_height: f32) -> usize {
        ((viewport.height / row_height) as usize)
            .clamp(1, MAX_VISIBLE_ROWS)
            .min(count)
    }

    pub fn panels<'a>(&'a self, scene: &'a Node, layout: &LayoutNode) -> Vec<MenuPanel<'a>> {
        let viewport = menu_viewport(scene, layout);
        let typography = layout.popup_typography();
        let show_shortcuts = !self.touch;
        if viewport.width <= 0.0 || viewport.height <= 0.0 || !valid(&self.trigger, scene, layout) {
            return Vec::new();
        }
        let mut anchor = if let Some(point) = self.point {
            Rect::new(point.x, point.y, 0.0, 0.0)
        } else if let Some(item) = layout.at_path(&self.trigger.path) {
            item.window_bounds(item.bounds)
        } else {
            return Vec::new();
        };
        let mut panels = Vec::new();
        let mut parent_direction = self.trigger_direction(layout);
        for (index, level) in self.levels.iter().enumerate() {
            let menu = self.text.as_ref().map_or_else(
                || scene.interactive_at_path(&level.path),
                |text| Some(&text.menu),
            );
            let Some(menu) = menu.filter(|menu| role(menu) == Some(MenuRole::Menu)) else {
                break;
            };
            let count = rows(menu).count();
            if count == 0 {
                break;
            }
            let row_height = row_height(menu, self.touch, typography);
            let visible = Self::visible_count(viewport, count, row_height);
            let first = level.first.min(count - visible);
            let icon_column = rows(menu).any(|(_, row)| icon(row).is_some());
            let width = rows(menu)
                .map(|(_, row)| {
                    let text_scale =
                        typography.size(row, DEFAULT_MENU_FONT_SIZE) / DEFAULT_MENU_FONT_SIZE;
                    let shortcut_width = if show_shortcuts {
                        shortcut(row).map_or(0.0, |key| {
                            key.to_string().len() as f32 * 8.0 * text_scale + 14.0
                        })
                    } else {
                        0.0
                    };
                    label(row)
                        .chars()
                        .map(|ch| if ch.is_ascii() { 8.0 } else { 16.0 })
                        .sum::<f32>()
                        * text_scale
                        + 54.0
                        + if icon_column { 22.0 } else { 0.0 }
                        + shortcut_width
                })
                .fold(174.0, f32::max)
                .min(viewport.width);
            let direction = menu.layout_direction().unwrap_or(parent_direction);
            let bounds = panel_bounds(
                anchor,
                viewport,
                width,
                (visible as f32 * row_height).min(viewport.height),
                index > 0,
                if index == 0 {
                    direction
                } else {
                    parent_direction
                },
            );
            let panel = MenuPanel {
                menu,
                bounds,
                layout_direction: direction,
                highlighted: level.highlight,
                icon_column,
                first,
                row_count: count,
                path: level.path.clone(),
                session: self.session,
                rows: rows(menu)
                    .skip(first)
                    .take(visible)
                    .map(|(index, row)| MenuRow {
                        index,
                        node: row,
                        label: label(row),
                        font_size: typography.size(row, DEFAULT_MENU_FONT_SIZE),
                        icon: icon(row),
                        shortcut: show_shortcuts
                            .then(|| shortcut(row).map(|key| key.to_string()))
                            .flatten(),
                        enabled: enabled(row),
                        separator: role(row) == Some(MenuRole::Separator),
                        submenu: role(row) == Some(MenuRole::Menu),
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
            parent_direction = direction;
            if slot.is_none() {
                break;
            }
        }
        panels
    }

    pub fn reconcile(&mut self, scene: &Node, layout: &LayoutNode) -> bool {
        let viewport = menu_viewport(scene, layout);
        let typography = layout.popup_typography();
        for level in &mut self.levels {
            let Some(menu) = menu_at(scene, self.text.as_ref(), &level.path) else {
                break;
            };
            let count = rows(menu).count();
            let visible =
                Self::visible_count(viewport, count, row_height(menu, self.touch, typography));
            level.first = level.first.min(count.saturating_sub(visible));
            if !level.highlight.is_some_and(|index| {
                menu.children
                    .get(index)
                    .is_some_and(|row| row.is_visible() && enabled(row))
            }) {
                let (first, limit) = if self.touch || level.scrolled {
                    (level.first, visible)
                } else {
                    (0, count)
                };
                level.highlight = rows(menu)
                    .skip(first)
                    .take(limit)
                    .find(|(_, row)| enabled(row))
                    .map(|(index, _)| index);
            }
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
        let index = row.index;
        if !row.enabled || self.levels[level].highlight == Some(index) {
            return false;
        }
        self.levels.truncate(level + 1);
        self.levels[level].highlight = Some(index);
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
                id: ElementId::of(row.node),
                command: row.node.action,
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
            || row.node.element_id() != pressed.id.as_ref().map(ElementId::as_str)
            || row.node.action != pressed.command
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
        if let Some(text) = &self.text {
            let node = text.menu.children.get(index)?;
            if !enabled(node) {
                return None;
            }
            path.push(index);
            return Some(path);
        }
        path.push(index);
        let node = scene.interactive_at_path(&path)?;
        match role(node) {
            Some(MenuRole::Action) => Some(path),
            Some(MenuRole::Menu) => {
                self.levels.truncate(level + 1);
                self.levels.push(Level {
                    path,
                    first: 0,
                    highlight: None,
                    scrolled: false,
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
        if &self.trigger != trigger || self.session != entry.session {
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
        session: u64,
    ) -> Option<bool> {
        if &self.trigger != trigger || self.session != session {
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

    pub fn accessibility_page(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        trigger: &Target,
        path: &[usize],
        session: u64,
        backward: bool,
    ) -> bool {
        if &self.trigger != trigger || self.session != session {
            return false;
        }
        let panels = self.panels(scene, layout);
        let Some((depth, panel)) = panels
            .iter()
            .enumerate()
            .find(|(_, panel)| panel.path == path && panel.can_scroll(backward))
        else {
            return false;
        };
        let page = panel.rows.len() as isize;
        self.pressed = None;
        self.scroll_level(
            scene,
            layout,
            depth,
            if backward { -page } else { page },
            backward,
        )
    }

    pub fn enter_submenu(&mut self, scene: &Node, layout: &LayoutNode) -> bool {
        let Some(level) = self.levels.last() else {
            return false;
        };
        let Some(index) = level.highlight else {
            return false;
        };
        if !menu_at(scene, self.text.as_ref(), &level.path)
            .and_then(|menu| menu.children.get(index))
            .is_some_and(|row| {
                role(row) == Some(MenuRole::Menu) && row.is_visible() && enabled(row)
            })
        {
            return false;
        }
        self.activate(scene, layout, self.levels.len() - 1, index);
        true
    }

    pub fn key(&mut self, scene: &Node, layout: &LayoutNode, key: MenuKey) -> Option<Vec<usize>> {
        let depth = self.levels.len().checked_sub(1)?;
        let forward = self.forward_key(scene, layout);
        let level = &mut self.levels[depth];
        let menu = menu_at(scene, self.text.as_ref(), &level.path)?;
        match key {
            MenuKey::Accept => {
                let index = level.highlight?;
                return self.activate(scene, layout, depth, index);
            }
            MenuKey::Left | MenuKey::Right | MenuKey::Escape if depth > 0 && key != forward => {
                self.levels.pop();
            }
            MenuKey::Up | MenuKey::Down | MenuKey::Home | MenuKey::End => {
                level.highlight = Some(menu_item(menu)?.navigate(menu, level.highlight, key)?);
                self.reconcile(scene, layout);
            }
            _ => {}
        }
        None
    }

    pub fn pan_at(&self, scene: &Node, layout: &LayoutNode, point: Point) -> Option<MenuPan> {
        let panel = self
            .panels(scene, layout)
            .into_iter()
            .rfind(|panel| panel.bounds.contains(point))?;
        Some(MenuPan {
            session: self.session,
            path: panel.path,
            remainder: 0.0,
        })
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
        self.scroll_level(scene, layout, depth, delta, false);
    }

    fn scroll_level(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        depth: usize,
        delta: isize,
        focus_last: bool,
    ) -> bool {
        let level = &mut self.levels[depth];
        let Some(menu) = menu_at(scene, self.text.as_ref(), &level.path) else {
            return false;
        };
        let count = rows(menu).count();
        let visible = Self::visible_count(
            menu_viewport(scene, layout),
            count,
            row_height(menu, self.touch, layout.popup_typography()),
        );
        let first = level.first;
        let highlight = level.highlight;
        level.first = first.saturating_add_signed(delta).min(count - visible);
        level.scrolled |= level.first != first;
        let mut choices = rows(menu)
            .skip(level.first)
            .take(visible)
            .filter(|(_, row)| enabled(row))
            .map(|(index, _)| index);
        level.highlight = if focus_last {
            choices.last()
        } else {
            choices.next()
        };
        let changed = level.first != first || level.highlight != highlight;
        let removed = self.levels.len() > depth + 1;
        self.levels.truncate(depth + 1);
        changed || removed
    }

    pub fn input(&self, mut path: Vec<usize>) -> Option<ControlInput> {
        let action = if self.text.is_some() {
            let index = path.pop()?;
            let (command, _, _) = TEXT_ENTRIES.get(index)?.as_ref()?;
            ControlAction::TextCommand(*command)
        } else {
            ControlAction::ActivateMenuItem
        };
        Some(ControlInput { path, action })
    }

    pub fn text_shortcut(&self, key: MenuShortcut) -> Option<ControlInput> {
        let text = self.text.as_ref()?;
        let (index, _) =
            rows(&text.menu).find(|(_, row)| enabled(row) && shortcut(row) == Some(key))?;
        let mut path = self.trigger.path.clone();
        path.push(index);
        self.input(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cascading_menu_prefers_the_inherited_side_and_fits_at_the_edge() {
        let viewport = Rect::new(4.0, 4.0, 420.0, 250.0);
        let rtl = LayoutDirection::RightToLeft;
        let ltr = LayoutDirection::LeftToRight;
        let width = 174.0;
        let middle = Rect::new(207.0, 40.0, 14.0, 28.0);
        assert_eq!(
            panel_bounds(middle, viewport, width, 56.0, true, rtl).x,
            33.0
        );
        assert_eq!(
            panel_bounds(middle, viewport, width, 56.0, true, ltr).x,
            221.0
        );
        let edge = Rect::new(6.0, 40.0, 174.0, 28.0);
        assert_eq!(
            panel_bounds(edge, viewport, width, 56.0, true, rtl).x,
            180.0
        );
        let far_edge = Rect::new(243.0, 40.0, 174.0, 28.0);
        assert_eq!(
            panel_bounds(far_edge, viewport, width, 56.0, true, ltr).x,
            69.0
        );
    }
}
