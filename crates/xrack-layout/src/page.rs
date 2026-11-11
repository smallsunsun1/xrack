use crate::{Rect, Size, TextMeasurer, linear, participates_in_flow};
use xrack_runtime::Node;

pub(crate) struct Regions {
    menu: Option<Rect>,
    header: Rect,
    pub(crate) body: Rect,
    footer: Rect,
}

impl Regions {
    pub(crate) fn new(page: &Node, bounds: Rect, text: &mut impl TextMeasurer) -> Self {
        let available = Size::new(bounds.width, bounds.height);
        let [menu, header, footer] = slots(page);
        let height = |slot: Option<usize>, text: &mut _| {
            slot.map(|index| &page.children[index])
                .filter(|child| participates_in_flow(child))
                .map_or(0.0, |child| {
                    linear::measure_with_width(child, available, Some(bounds.width), text)
                        .y
                        .preferred
                })
        };
        let menu_height = height(menu, text).min(bounds.height);
        let header_height = height(header, text).min(bounds.height - menu_height);
        let top = menu_height + header_height;
        let bottom = height(footer, text).min(bounds.height - top);
        let remaining = bounds.height - top - bottom;
        let spacing = linear::spacing(page);
        let above = gap(top, spacing).min(remaining);
        let below = gap(bottom, spacing).min(remaining - above);
        let body = Rect::new(
            bounds.x,
            bounds.y + top + above,
            bounds.width,
            remaining - above - below,
        );
        Self {
            menu: menu.map(|_| Rect::new(bounds.x, bounds.y, bounds.width, menu_height)),
            header: Rect::new(
                bounds.x,
                bounds.y + menu_height,
                bounds.width,
                header_height,
            ),
            body: linear::Padding::read(page).inner_rect(body),
            footer: Rect::new(
                bounds.x,
                bounds.y + bounds.height - bottom,
                bounds.width,
                bottom,
            ),
        }
    }

    pub(crate) fn band(&self, child: &Node) -> Option<Rect> {
        match child.kind {
            "MenuBar" => self.menu,
            "PageHeader" => Some(self.header),
            "PageFooter" => Some(self.footer),
            _ => None,
        }
    }
}

pub(crate) fn preferred_size(page: &Node, available: Size, text: &mut impl TextMeasurer) -> Size {
    let padding = linear::Padding::read(page);
    let inner = padding.inner_size(available);
    let spacing = linear::spacing(page);
    let mut chrome = Size::default();
    let mut body = Size::default();
    for child in page
        .children
        .iter()
        .filter(|child| participates_in_flow(child))
    {
        if matches!(child.kind, "PageHeader" | "PageFooter") {
            let metrics = linear::measure_with_width(child, available, Some(available.width), text);
            chrome.width = chrome.width.max(metrics.x.preferred);
            chrome.height += metrics.y.preferred + gap(metrics.y.preferred, spacing);
        } else {
            let metrics = linear::measure(child, inner, text);
            body.width = body
                .width
                .max(child.length("x").unwrap_or(0.0) + metrics.x.preferred);
            body.height = body
                .height
                .max(child.length("y").unwrap_or(0.0) + metrics.y.preferred);
        }
    }
    let padding = padding.size();
    Size::new(
        chrome.width.max(body.width + padding.width),
        chrome.height + body.height + padding.height,
    )
}

pub(crate) fn slots(node: &Node) -> [Option<usize>; 3] {
    if node.kind != "Page" && !node.application_window {
        return [None; 3];
    }
    let index = |kind| node.children.iter().position(|child| child.kind == kind);
    [
        node.application_window.then(|| index("MenuBar")).flatten(),
        index("PageHeader"),
        index("PageFooter"),
    ]
}

fn gap(height: f32, spacing: f32) -> f32 {
    if height > 0.0 { spacing } else { 0.0 }
}
