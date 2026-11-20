use crate::{
    LayoutNode, Point, Rect, Size, Target, TextMeasurer, input::valid, place_in_direction,
    popup::PopupTypography, window_safe_area,
};
use std::borrow::Cow;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use unicode_bidi::BidiInfo;
use unicode_segmentation::UnicodeSegmentation;
use xrack_runtime::{
    ChoiceIdentity, ComboOptionInput, ComboPopupHandle, LayoutDirection, Node, RowKey,
    choice_enabled, choice_identity, combo_count, combo_editable, combo_find_enabled_label,
    combo_index, combo_label, combo_labels, combo_row_index, combo_row_key, combo_text,
    search_suggestion, search_suggestions,
};

const ROW_HEIGHT: f32 = 28.0;
pub(crate) const MAX_ROWS: usize = 8;

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

    pub(crate) fn continues_left(&self) -> bool {
        BidiInfo::new(&self.value[..self.draft_len], None)
            .paragraphs
            .first()
            .is_some_and(|paragraph| paragraph.level.is_rtl())
    }
}

pub(crate) fn completion_value<'a>(node: &'a Node, draft: &str) -> Option<Cow<'a, str>> {
    if draft.is_empty() || combo_find_enabled_label(node, 0, |value| value == draft).is_some() {
        return None;
    }
    let index = combo_find_enabled_label(node, 0, |value| {
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
    use super::{LayoutDirection, Node, PopupTypography, Rect, completion_value, geometry};

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

    #[test]
    fn popup_reduces_padding_in_tight_space_before_overlapping_the_anchor() {
        let mut combo = Node::new("ComboBox", None);
        combo
            .properties
            .insert("model", vec!["one".to_owned(); 10].into());
        let mut style = Node::new("ComboPopup", None);
        for (name, value) in [
            ("max-height", 120.0),
            ("row-height", 32.0),
            ("padding", 8.0),
        ] {
            style
                .properties
                .insert(name, xrack_runtime::Length(value).into());
        }
        combo.children.push(style);
        let popup = geometry(
            &combo,
            Rect::new(10.0, 10.0, 100.0, 35.0),
            Rect::new(0.0, 0.0, 180.0, 80.0),
            LayoutDirection::LeftToRight,
            PopupTypography::default(),
            None,
        )
        .unwrap();
        assert_eq!(popup.bounds, Rect::new(10.0, 45.0, 100.0, 35.0));
        assert_eq!(popup.visible, 1);
        assert_eq!(popup.rows.height, 32.0);

        let style = &mut combo.children[0];
        style
            .properties
            .insert("width", xrack_runtime::Percent(1.25).into());
        style
            .properties
            .insert("max-height", xrack_runtime::Percent(0.5).into());
        let popup = geometry(
            &combo,
            Rect::new(10.0, 10.0, 100.0, 20.0),
            Rect::new(0.0, 0.0, 180.0, 80.0),
            LayoutDirection::LeftToRight,
            PopupTypography::default(),
            None,
        )
        .unwrap();
        assert_eq!(popup.bounds, Rect::new(10.0, 30.0, 125.0, 40.0));
        assert_eq!(popup.rows.height, 32.0);
    }

    #[test]
    fn choice_popup_moves_sideways_when_it_can_fit_more_complete_rows() {
        let mut combo = Node::new("ComboBox", None);
        combo
            .properties
            .insert("model", vec!["one".to_owned(); 10].into());
        let viewport = Rect::new(0.0, 0.0, 400.0, 210.0);
        let anchor = Rect::new(120.0, 60.0, 100.0, 40.0);
        let keyboard = Rect::new(140.0, 30.0, 160.0, 160.0);
        let popup = geometry(
            &combo,
            anchor,
            viewport,
            LayoutDirection::LeftToRight,
            PopupTypography::default(),
            Some(keyboard),
        )
        .unwrap();
        assert!(popup.bounds.x + popup.bounds.width <= keyboard.x);
        assert_eq!(popup.bounds.y, anchor.y + anchor.height);
        assert_eq!(popup.bounds.width, anchor.width);
        assert_eq!(popup.visible, 3);
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

#[derive(Clone, Debug, Default)]
pub(crate) struct Typeahead {
    prefix: String,
    last: Option<Instant>,
}

impl Typeahead {
    pub fn find(
        &mut self,
        node: &Node,
        text: &str,
        now: Instant,
        current: Option<usize>,
    ) -> Option<usize> {
        self.find_with(text, now, current, |start, prefix| {
            combo_find_enabled_label(node, start, |label| matches_prefix(label, prefix))
        })
    }

    pub(crate) fn find_with(
        &mut self,
        text: &str,
        now: Instant,
        current: Option<usize>,
        find: impl Fn(usize, &str) -> Option<usize>,
    ) -> Option<usize> {
        if text.is_empty() || text.chars().any(char::is_control) {
            return None;
        }
        let text: String = text.chars().flat_map(char::to_lowercase).collect();
        let continuing = self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) <= Duration::from_millis(800));
        self.last = Some(now);
        if continuing && self.prefix != text && self.prefix.len() + text.len() <= 128 {
            self.prefix.push_str(&text);
            if let Some(index) = find(current.unwrap_or(0), &self.prefix) {
                return Some(index);
            }
        }
        self.prefix = text;
        find(current.map_or(0, |index| index + 1), &self.prefix)
    }
}

pub(crate) fn matches_prefix(label: &str, prefix: &str) -> bool {
    let mut label = label.chars().flat_map(char::to_lowercase);
    prefix
        .chars()
        .all(|character| label.next() == Some(character))
}

pub(crate) fn next_choice(
    node: &Node,
    current: Option<usize>,
    key: ChoiceKey,
    page: usize,
) -> Option<usize> {
    let count = choice_count(node);
    let last = count.checked_sub(1)?;
    let enabled = |&index: &usize| choice_enabled(node, index);
    match key {
        ChoiceKey::Up => (0..current.unwrap_or(count)).rev().find(enabled),
        ChoiceKey::Down => (current.map_or(0, |index| index + 1)..count).find(enabled),
        ChoiceKey::Home => (0..count).find(enabled),
        ChoiceKey::End => (0..count).rev().find(enabled),
        ChoiceKey::PageUp => {
            let Some(current) = current else {
                return (0..count).find(enabled);
            };
            let target = current.saturating_sub(page);
            (0..=target)
                .rev()
                .find(enabled)
                .or_else(|| (target..current).find(enabled))
        }
        ChoiceKey::PageDown => {
            let target = current.map_or(0, |index| (index + page).min(last));
            (target..count).find(enabled).or_else(|| {
                (current.map_or(0, |index| index + 1)..target)
                    .rev()
                    .find(enabled)
            })
        }
        _ => None,
    }
}

pub struct ComboGeometry {
    pub entry: Rect,
    pub trigger: Rect,
}

impl ComboGeometry {
    const TRIGGER_WIDTH: f32 = 28.0;
    const LABEL_INSET: f32 = 8.0;

    pub fn new(bounds: Rect) -> Self {
        Self::with_layout_direction(bounds, LayoutDirection::LeftToRight)
    }

    /// Default arrow geometry. For a control with replacement artwork, use `for_control`.
    pub fn for_layout(layout: &LayoutNode) -> Self {
        Self::with_layout_direction(layout.bounds, layout.layout_direction)
    }

    pub fn with_layout_direction(bounds: Rect, direction: LayoutDirection) -> Self {
        Self::with_indicator(bounds, direction, None)
    }

    /// Measure the replacement artwork against the same bounds used by painting and input.
    pub fn for_control(node: &Node, bounds: Rect, direction: LayoutDirection) -> Self {
        let indicator = node
            .control_part("ComboIndicator")
            .map(Self::indicator_size);
        Self::with_indicator(bounds, direction, indicator)
    }

    fn indicator_size(indicator: &Node) -> Size {
        Size::new(
            indicator.length("width").unwrap_or(16.0).max(0.0),
            indicator.length("height").unwrap_or(16.0).max(0.0),
        )
    }

    pub(crate) fn preferred_size(
        node: &Node,
        available: Size,
        text: &mut impl TextMeasurer,
    ) -> Size {
        let indicator = node
            .control_part("ComboIndicator")
            .map(Self::indicator_size);
        let trigger = Self::trigger_width(indicator);
        let content = node.combo_content().map_or_else(Size::default, |content| {
            let available = Size::new(
                (available.width - trigger - Self::LABEL_INSET).max(0.0),
                available.height,
            );
            crate::linear::measure_replacement_content(content, available, text)
        });
        let (entry_width, vertical_padding) = if xrack_runtime::combo_editable(node) {
            let padding = crate::linear::Padding::for_editor(node).size();
            (116.0 + padding.width, padding.height)
        } else {
            (128.0, 10.0)
        };
        Size::new(
            trigger + entry_width.max(content.width + Self::LABEL_INSET),
            (text.font_size(node) * 1.4 + vertical_padding)
                .max(34.0)
                .max(indicator.map_or(0.0, |size| size.height + 8.0))
                .max(content.height),
        )
    }

    /// The inset used by the selected label and replacement content.
    #[doc(hidden)]
    pub fn label_bounds(&self, direction: LayoutDirection) -> Rect {
        let entry = self.entry;
        let inset = entry.width.min(Self::LABEL_INSET);
        let x = if direction == LayoutDirection::RightToLeft {
            entry.x
        } else {
            entry.x + inset
        };
        Rect::new(x, entry.y, entry.width - inset, entry.height)
    }

    fn trigger_width(indicator: Option<Size>) -> f32 {
        Self::TRIGGER_WIDTH.max(indicator.map_or(0.0, |size| size.width + 12.0))
    }

    fn with_indicator(bounds: Rect, direction: LayoutDirection, indicator: Option<Size>) -> Self {
        let trigger = bounds.width.min(Self::trigger_width(indicator));
        let entry = bounds.width - trigger;
        let mirror = |rect| place_in_direction(rect, bounds, direction);
        Self {
            entry: mirror(Rect::new(bounds.x, bounds.y, entry, bounds.height)),
            trigger: mirror(Rect::new(
                bounds.x + entry,
                bounds.y,
                trigger,
                bounds.height,
            )),
        }
    }

    pub(crate) fn indicator_bounds(&self, part: &Node) -> Rect {
        let size = Self::indicator_size(part);
        let width = size.width.min(self.trigger.width);
        let height = size.height.min(self.trigger.height);
        Rect::new(
            self.trigger.x + (self.trigger.width - width) * 0.5,
            self.trigger.y + (self.trigger.height - height) * 0.5,
            width,
            height,
        )
    }
}

pub struct ChoiceMenu<'a> {
    pub control: &'a Node,
    pub bounds: Rect,
    pub layout_direction: LayoutDirection,
    pub rows: Cow<'a, [String]>,
    pub first: usize,
    pub highlighted: Option<usize>,
    pub selected: Option<usize>,
    pub font_size: f32,
    session: u64,
    rows_bounds: Rect,
    metrics: PopupMetrics,
}

impl ChoiceMenu<'_> {
    #[doc(hidden)]
    pub fn session(&self) -> u64 {
        self.session
    }

    #[doc(hidden)]
    pub fn row_count(&self) -> usize {
        choice_count(self.control)
    }

    #[doc(hidden)]
    pub fn popup_style(&self) -> Option<&Node> {
        popup_style(self.control)
    }

    pub fn rows_bounds(&self) -> Rect {
        self.rows_bounds
    }

    pub fn scroll_indicator(&self) -> Option<Rect> {
        let metrics = &self.metrics;
        let total = self.row_count();
        let visible = self.rows.len();
        if !metrics.scroll_indicator || visible == 0 || visible >= total {
            return None;
        }
        let inner = metrics.interior(self.bounds);
        let gutter = PopupMetrics::gutter(inner.width);
        let width = (gutter * 0.5).min(4.0);
        let height = (inner.height * visible as f32 / total as f32)
            .max(10.0)
            .min(inner.height);
        let offset = self.first as f32 / (total - visible) as f32;
        let x = inner.x + inner.width - (gutter + width) * 0.5;
        let thumb = Rect::new(x, inner.y + (inner.height - height) * offset, width, height);
        Some(place_in_direction(thumb, inner, self.layout_direction))
    }

    #[doc(hidden)]
    pub fn option_input(&self, offset: usize) -> Option<ComboOptionInput> {
        let text = self.rows.get(offset)?.clone();
        let index = self.first + offset;
        Some(ComboOptionInput::new(
            self.control,
            index,
            text,
            self.selected == Some(index),
            self.highlighted == Some(index),
        ))
    }

    pub fn row_enabled(&self, offset: usize) -> bool {
        offset < self.rows.len() && choice_enabled(self.control, self.first + offset)
    }

    #[doc(hidden)]
    pub fn can_scroll(&self, backward: bool) -> bool {
        if backward {
            self.first > 0
        } else {
            self.first + self.rows.len() < self.row_count()
        }
    }

    pub fn row_bounds(&self, offset: usize) -> Rect {
        let rows = self.rows_bounds();
        let height = rows.height / self.rows.len() as f32;
        Rect::new(rows.x, rows.y + height * offset as f32, rows.width, height)
    }
}

fn popup_style(node: &Node) -> Option<&Node> {
    (node.kind == "ComboBox")
        .then(|| node.control_part("ComboPopup"))
        .flatten()
}

struct PopupMetrics {
    row_height: f32,
    padding: f32,
    scroll_indicator: bool,
}

impl PopupMetrics {
    fn new(style: Option<&Node>, font_size: f32, scale: f32) -> Self {
        let positive = |name| {
            style
                .and_then(|style| style.length(name))
                .filter(|&value| value > 0.0)
        };
        let height = positive("row-height").unwrap_or(ROW_HEIGHT);
        let row_height = if scale > 1.0 {
            (height * scale).max(font_size * 1.4 + 8.0)
        } else {
            height.max(1.0)
        };
        let padding = positive("padding").unwrap_or(0.0);
        Self {
            row_height,
            padding,
            scroll_indicator: style.is_some_and(|style| style.boolean("scroll-indicator", false)),
        }
    }

    fn vertical_padding(&self, height: f32) -> f32 {
        self.padding.min((height - self.row_height).max(0.0) * 0.5)
    }

    fn interior(&self, bounds: Rect) -> Rect {
        let x = self.padding.min((bounds.width - 1.0).max(0.0) * 0.5);
        let y = self.vertical_padding(bounds.height);
        Rect::new(
            bounds.x + x,
            bounds.y + y,
            bounds.width - 2.0 * x,
            bounds.height - 2.0 * y,
        )
    }

    fn gutter(width: f32) -> f32 {
        (width * 0.25).min(10.0)
    }

    fn rows(&self, bounds: Rect, scrolling: bool, direction: LayoutDirection) -> Rect {
        let inner = self.interior(bounds);
        let width = if self.scroll_indicator && scrolling {
            inner.width - Self::gutter(inner.width)
        } else {
            inner.width
        };
        place_in_direction(
            Rect::new(inner.x, inner.y, width, inner.height),
            inner,
            direction,
        )
    }
}

pub(crate) struct ChoiceGeometry {
    bounds: Rect,
    rows: Rect,
    visible: usize,
    direction: LayoutDirection,
    metrics: PopupMetrics,
    font_size: f32,
}

impl ChoiceGeometry {
    fn row_at(&self, point: Point) -> Option<usize> {
        if !self.rows.contains(point) {
            return None;
        }
        let offset = ((point.y - self.rows.y) / self.rows.height * self.visible as f32) as usize;
        (offset < self.visible).then_some(offset)
    }
}

fn geometry(
    node: &Node,
    anchor: Rect,
    viewport: Rect,
    direction: LayoutDirection,
    typography: PopupTypography,
    floating_keyboard: Option<Rect>,
) -> Option<ChoiceGeometry> {
    let len = choice_count(node);
    if len == 0 || viewport.width <= 0.0 || viewport.height <= 0.0 {
        return None;
    }
    let style = popup_style(node);
    let font_size = typography.size(node, 14.0);
    let metrics = PopupMetrics::new(style, font_size, typography.scale);
    let dimension = |name, reference| {
        style
            .and_then(|style| style.relative_length(name, reference))
            .filter(|&value| value > 0.0)
    };
    let limit = dimension("max-height", viewport.height)
        .unwrap_or(metrics.row_height * MAX_ROWS as f32 + 2.0 * metrics.padding)
        .min(viewport.height);
    let desired =
        (metrics.row_height * len as f32 + 2.0 * metrics.vertical_padding(limit)).min(limit);
    let width = dimension("width", anchor.width)
        .unwrap_or_else(|| anchor.width.max(96.0) * typography.scale.max(1.0))
        .min(viewport.width);
    let aligned_x = if direction == LayoutDirection::RightToLeft {
        anchor.x + anchor.width - width
    } else {
        anchor.x
    };
    let place = |area: Rect, avoid_anchor: bool| {
        if area.width < width || area.height <= 0.0 {
            return None;
        }
        let bottom = area.y + area.height;
        let below = (bottom - (anchor.y + anchor.height).max(area.y)).max(0.0);
        let above = (anchor.y.min(bottom) - area.y).max(0.0);
        let down = below >= desired || below >= above;
        let space = if down { below } else { above };
        let min_row = metrics.row_height.min(limit);
        if avoid_anchor && space < min_row {
            return None;
        }
        let available = space.min(limit).max(min_row);
        let padding = 2.0 * metrics.vertical_padding(available);
        let visible = (((available - padding) / metrics.row_height) as usize).clamp(1, len);
        let height = (visible as f32 * metrics.row_height + padding).min(available);
        let x = aligned_x.clamp(area.x, area.x + (area.width - width));
        let y = if down {
            anchor.y + anchor.height
        } else {
            anchor.y - height
        };
        let y = y.clamp(area.y, area.y + (area.height - height).max(0.0));
        Some((Rect::new(x, y, width, height), visible))
    };
    let (original, visible) = place(viewport, false)?;
    let keyboard = floating_keyboard.map(|keyboard| keyboard.intersect(viewport));
    let (bounds, visible) = if let Some(keyboard) = keyboard.filter(|keyboard| {
        let covered = original.intersect(*keyboard);
        covered.width > 0.0 && covered.height > 0.0
    }) {
        let right = keyboard.x + keyboard.width;
        let bottom = keyboard.y + keyboard.height;
        let free_areas = [
            Rect::new(
                viewport.x,
                viewport.y,
                viewport.width,
                keyboard.y - viewport.y,
            ),
            Rect::new(
                viewport.x,
                bottom,
                viewport.width,
                viewport.y + viewport.height - bottom,
            ),
            Rect::new(
                viewport.x,
                viewport.y,
                keyboard.x - viewport.x,
                viewport.height,
            ),
            Rect::new(
                right,
                viewport.y,
                viewport.x + viewport.width - right,
                viewport.height,
            ),
        ];
        let distance = |bounds: Rect| (bounds.x - original.x).abs() + (bounds.y - original.y).abs();
        free_areas
            .into_iter()
            .filter_map(|area| place(area, true))
            .max_by(|(left, left_rows), (right, right_rows)| {
                left_rows
                    .cmp(right_rows)
                    .then_with(|| distance(*right).total_cmp(&distance(*left)))
            })?
    } else {
        (original, visible)
    };
    Some(ChoiceGeometry {
        bounds,
        rows: metrics.rows(bounds, len > visible, direction),
        visible,
        direction,
        metrics,
        font_size,
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
    handle: Option<ComboPopupHandle>,
    session: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct ChoicePan {
    session: u64,
    remainder: f32,
}

impl ChoicePan {
    pub(crate) fn advance(
        &mut self,
        popup: &mut ChoicePopup,
        scene: &Node,
        layout: &LayoutNode,
        delta: f32,
    ) -> bool {
        if self.session != popup.session {
            return false;
        }
        let Some((_, geometry)) = popup.resolve(scene, layout) else {
            return false;
        };
        let row_height = geometry.rows.height / geometry.visible as f32;
        self.remainder += delta;
        let rows = (self.remainder / row_height).trunc() as isize;
        self.remainder %= row_height;
        rows != 0 && popup.scroll(scene, layout, rows)
    }
}

impl ChoicePopup {
    pub(crate) fn artwork_dirty(&self, scene: &Node, layout: &LayoutNode) -> bool {
        self.node(scene, layout)
            .and_then(|node| node.combo_option_renderer.as_ref())
            .is_some_and(|renderer| renderer.artwork_dirty())
    }

    pub fn open(target: Target, scene: &Node, layout: &LayoutNode) -> Option<Self> {
        static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
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
            handle: None,
            session: NEXT_SESSION.fetch_add(1, Ordering::Relaxed),
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
                combo_find_enabled_label(node, 0, |item| item == draft)
                    .or_else(|| combo_find_enabled_label(node, 0, |item| item.starts_with(draft)))
            })
            .or_else(|| current.filter(|&index| choice_enabled(node, index)))
            .or_else(|| combo_find_enabled_label(node, current.unwrap_or(0), |_| true));
        if let Some(index) = index {
            popup.highlight(node, index);
        }
        popup.reconcile(scene, layout).then_some(popup)
    }

    pub(crate) fn mounted(&mut self, scene: &Node) {
        if let Some(node) = scene.at_path(&self.target.path) {
            self.handle = node.combo_popup_handle.clone();
            self.publish(node);
        }
    }

    pub(crate) fn closed(self) {
        if let Some(handle) = self.handle {
            handle.publish(false, None);
        }
    }

    fn publish(&self, node: &Node) {
        if let Some(handle) = &self.handle {
            handle.publish(true, self.highlighted(node));
        }
    }

    fn node<'a>(&self, scene: &'a Node, layout: &LayoutNode) -> Option<&'a Node> {
        if !valid(&self.target, scene, layout) {
            return None;
        }
        scene.at_path(&self.target.path).filter(|node| {
            node.kind == self.kind
                && self
                    .handle
                    .as_ref()
                    .is_none_or(|handle| node.combo_popup_handle.as_ref() == Some(handle))
        })
    }

    fn resolve<'a>(
        &self,
        scene: &'a Node,
        layout: &LayoutNode,
    ) -> Option<(&'a Node, ChoiceGeometry)> {
        let node = self.node(scene, layout)?;
        let item = layout.at_path(&self.target.path)?;
        let anchor = item.window_bounds(item.bounds);
        Some((
            node,
            geometry(
                node,
                anchor,
                window_safe_area(scene, layout.bounds),
                item.layout_direction,
                layout.popup_typography(),
                layout.floating_keyboard(),
            )?,
        ))
    }

    pub(crate) fn pan_at(
        &self,
        scene: &Node,
        layout: &LayoutNode,
        point: Point,
    ) -> Option<ChoicePan> {
        let (_, geometry) = self.resolve(scene, layout)?;
        geometry.bounds.contains(point).then_some(ChoicePan {
            session: self.session,
            remainder: 0.0,
        })
    }

    pub fn view<'a>(&self, scene: &'a Node, layout: &LayoutNode) -> Option<ChoiceMenu<'a>> {
        let (node, geo) = self.resolve(scene, layout)?;
        let first = self.first.min(choice_count(node) - geo.visible);
        let range = first..first + geo.visible;
        Some(ChoiceMenu {
            control: node,
            bounds: geo.bounds,
            layout_direction: geo.direction,
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
            font_size: geo.font_size,
            session: self.session,
            rows_bounds: geo.rows,
            metrics: geo.metrics,
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
        self.publish(node);
        true
    }

    fn highlighted(&self, node: &Node) -> Option<usize> {
        let index = self.highlight?;
        if node.kind == "SearchField" {
            (search_suggestion(node, index) == self.highlighted_value.as_deref()
                && self.highlighted_value.is_some())
            .then_some(index)
        } else {
            let index = match self.highlighted_row {
                Some(key) => combo_row_index(node, key)?,
                None => index.min(choice_count(node).saturating_sub(1)),
            };
            choice_enabled(node, index).then_some(index)
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
        self.publish(node);
    }

    fn pick<'a>(
        &self,
        scene: &'a Node,
        layout: &LayoutNode,
        point: Point,
    ) -> Option<(&'a Node, usize)> {
        let (node, geo) = self.resolve(scene, layout)?;
        let offset = geo.row_at(point)?;
        let first = self.first.min(choice_count(node) - geo.visible);
        Some((node, first + offset))
    }

    pub fn hover(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> bool {
        let Some((node, index)) = self.pick(scene, layout, point) else {
            return false;
        };
        if !choice_enabled(node, index) {
            return false;
        }
        let previous = self.highlighted(node);
        self.highlight(node, index);
        previous != self.highlight
    }

    pub fn press(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> bool {
        self.press = None;
        let Some((node, geo)) = self.resolve(scene, layout) else {
            return false;
        };
        if let Some(offset) = geo.row_at(point) {
            let index = self.first.min(choice_count(node) - geo.visible) + offset;
            if choice_enabled(node, index) {
                self.highlight(node, index);
                self.press = choice_identity(node, index).map(|identity| (index, identity));
            }
        }
        geo.bounds.contains(point)
    }

    pub fn release(&mut self, scene: &Node, layout: &LayoutNode, point: Point) -> Option<usize> {
        let (pressed, previous) = self.press.take()?;
        let (node, index) = self.pick(scene, layout, point)?;
        (index == pressed
            && choice_enabled(node, index)
            && choice_identity(node, index).as_ref() == Some(&previous))
        .then_some(index)
    }

    pub fn move_highlight(&mut self, scene: &Node, layout: &LayoutNode, key: ChoiceKey) {
        let Some((node, geo)) = self.resolve(scene, layout) else {
            return;
        };
        if let Some(index) = next_choice(node, self.highlighted(node), key, geo.visible) {
            self.highlight(node, index);
            self.reconcile(scene, layout);
        }
    }

    pub fn typeahead(
        &mut self,
        scene: &Node,
        layout: &LayoutNode,
        search: &mut Typeahead,
        text: &str,
        now: Instant,
    ) {
        let Some(node) = self.node(scene, layout) else {
            return;
        };
        if let Some(index) = search.find(node, text, now, self.highlighted(node)) {
            self.highlight(node, index);
            self.reconcile(scene, layout);
        }
    }

    pub fn selected(&self, scene: &Node, layout: &LayoutNode) -> Option<usize> {
        self.highlighted(self.resolve(scene, layout)?.0)
    }

    pub fn scroll(&mut self, scene: &Node, layout: &LayoutNode, rows: isize) -> bool {
        let Some((node, geo)) = self.resolve(scene, layout) else {
            return false;
        };
        let before = self.first;
        self.first = self
            .first
            .saturating_add_signed(rows)
            .min(choice_count(node) - geo.visible);
        if let Some(highlight) = self.highlighted(node) {
            let end = self.first + geo.visible;
            let index = if highlight >= end {
                (self.first..end)
                    .rev()
                    .find(|&index| choice_enabled(node, index))
            } else {
                (highlight.max(self.first)..end).find(|&index| choice_enabled(node, index))
            };
            if let Some(index) = index {
                self.highlight(node, index);
            } else {
                self.highlight = None;
            }
        }
        self.press = None;
        self.publish(node);
        self.first != before
    }
}

fn choice_count(node: &Node) -> usize {
    if node.kind == "SearchField" {
        search_suggestions(node).len()
    } else {
        combo_count(node)
    }
}
