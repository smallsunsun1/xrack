use super::{DeferredLayout, Item, ItemLayout, ItemPaint, MeasureContext};
use crate::{MenuKey, Size, linear::Metrics};
use xrack_runtime::Node;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuRole {
    Bar,
    Menu,
    Action,
    Separator,
}

#[derive(Clone, Copy)]
pub(crate) enum MenuHeaderAction {
    Open,
    FocusSibling,
}

pub(crate) trait MenuItem {
    fn role(&self) -> MenuRole;

    fn header_key(&self, _key: MenuKey) -> Option<MenuHeaderAction> {
        None
    }

    fn navigate(&self, _menu: &Node, _highlight: Option<usize>, _key: MenuKey) -> Option<usize> {
        None
    }
}

pub(crate) fn menu_item(node: &Node) -> Option<&'static dyn MenuItem> {
    super::item(node.kind).menu()
}

pub(crate) fn role(node: &Node) -> Option<MenuRole> {
    menu_item(node).map(MenuItem::role)
}

pub(crate) fn entries(menu: &Node) -> impl Iterator<Item = (usize, &Node)> {
    menu.children.iter().enumerate().filter(|(_, child)| {
        child.is_visible()
            && matches!(
                role(child),
                Some(MenuRole::Menu | MenuRole::Action | MenuRole::Separator)
            )
    })
}

pub(crate) fn enabled(row: &Node) -> bool {
    role(row) != Some(MenuRole::Separator) && row.boolean("enabled", true)
}

pub(crate) fn label(row: &Node) -> &str {
    let property = if role(row) == Some(MenuRole::Menu) {
        "title"
    } else {
        "text"
    };
    row.text(property).unwrap_or("")
}

pub(super) struct Menu;

impl Item for Menu {
    fn size_policy(&self, _node: &xrack_runtime::Node) -> super::SizePolicy {
        super::SizePolicy {
            preferred_minimum: true,
            ..super::SizePolicy::INTRINSIC_HEIGHT
        }
    }

    fn menu(&self) -> Option<&dyn MenuItem> {
        Some(self)
    }

    fn layout(&self) -> ItemLayout {
        ItemLayout::Deferred(DeferredLayout::Menu)
    }

    fn focusable(&self, _node: &Node) -> bool {
        true
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> ItemPaint {
        ItemPaint::Menu
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> Metrics {
        let size = Size::new(
            cx.layout
                .measure_node(cx.node, label(cx.node), cx.available.width)
                .width
                + 24.0,
            (cx.layout.font_size(cx.node) * 1.4 + 8.0).max(32.0),
        );
        cx.intrinsic(size)
    }
}

impl MenuItem for Menu {
    fn role(&self) -> MenuRole {
        MenuRole::Menu
    }

    fn header_key(&self, key: MenuKey) -> Option<MenuHeaderAction> {
        match key {
            MenuKey::Up | MenuKey::Down | MenuKey::Accept => Some(MenuHeaderAction::Open),
            MenuKey::Left | MenuKey::Right => Some(MenuHeaderAction::FocusSibling),
            _ => None,
        }
    }

    fn navigate(&self, menu: &Node, highlight: Option<usize>, key: MenuKey) -> Option<usize> {
        let choices: Vec<_> = entries(menu)
            .filter(|(_, row)| enabled(row))
            .map(|(index, _)| index)
            .collect();
        if choices.is_empty() {
            return None;
        }
        let current = choices.iter().position(|&index| Some(index) == highlight);
        let next = match key {
            MenuKey::Up => current.map_or(choices.len() - 1, |pos| {
                (pos + choices.len() - 1) % choices.len()
            }),
            MenuKey::Down => current.map_or(0, |pos| (pos + 1) % choices.len()),
            MenuKey::Home => 0,
            MenuKey::End => choices.len() - 1,
            _ => return None,
        };
        Some(choices[next])
    }
}

pub(super) struct MenuEntry(pub(super) MenuRole);

impl Item for MenuEntry {
    fn menu(&self) -> Option<&dyn MenuItem> {
        Some(self)
    }
}

impl MenuItem for MenuEntry {
    fn role(&self) -> MenuRole {
        self.0
    }
}
