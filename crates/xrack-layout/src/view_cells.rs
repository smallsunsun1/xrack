use crate::{LayoutNode, Rect, ScrollLayout};
use std::{ops::Range, sync::Arc};
use xrack_runtime::{ListMetrics, ListSection, Node, RowHeights, RowKey};

/// Visible inline or pinned section heading in a prepared list viewport.
#[derive(Clone, Debug, PartialEq)]
pub struct ListSectionHeader {
    pub first_row: usize,
    pub row_count: usize,
    pub key: RowKey,
    pub label: Arc<str>,
    pub bounds: Rect,
    pub pinned: bool,
    pub(crate) content: Option<Box<(Node, LayoutNode)>>,
}

impl ListSectionHeader {
    pub fn content(&self) -> Option<(&Node, &LayoutNode)> {
        self.content.as_deref().map(|(node, item)| (node, item))
    }
}

pub(crate) fn list_body_clip(clip: Rect, headers: &[ListSectionHeader]) -> Rect {
    let Some(header) = headers.first().filter(|header| header.pinned) else {
        return clip;
    };
    let y = (header.bounds.y + header.bounds.height).clamp(clip.y, clip.y + clip.height);
    Rect::new(clip.x, y, clip.width, clip.y + clip.height - y)
}

/// Geometry shared by virtualization, selection reveal, and keyboard navigation.
#[derive(Clone, Debug)]
pub(crate) struct ViewCells {
    pub(crate) columns: usize,
    height: f32,
    width: f32,
    spacing: f32,
    row_heights: Option<Arc<RowHeights>>,
    sections: Option<Arc<[ListSection]>>,
    section_height: f32,
    sticky_sections: bool,
}

impl ViewCells {
    pub(crate) fn new(node: &Node, viewport_width: f32) -> Self {
        if node.kind != "GridView" {
            return Self {
                columns: 1,
                height: ListMetrics::row_height(node),
                width: viewport_width,
                spacing: 0.0,
                row_heights: node.row_heights.clone(),
                sections: node
                    .list_sections
                    .as_ref()
                    .filter(|sections| !sections.is_empty())
                    .cloned(),
                section_height: node
                    .length("section-height")
                    .filter(|height| height.is_finite())
                    .unwrap_or(28.0)
                    .clamp(16.0, 128.0),
                sticky_sections: node.boolean("sticky-section-headers", false),
            };
        }
        let width = node
            .length("cell-width")
            .unwrap_or(144.0)
            .clamp(16.0, 1024.0);
        let height = node
            .length("cell-height")
            .unwrap_or(96.0)
            .clamp(12.0, 512.0);
        let spacing = node.length("spacing").unwrap_or(0.0).clamp(0.0, 128.0);
        let columns = ((viewport_width + spacing) / (width + spacing)).floor() as usize;
        Self {
            columns: columns.max(1),
            height,
            width: width.min(viewport_width),
            spacing,
            row_heights: None,
            sections: None,
            section_height: 0.0,
            sticky_sections: false,
        }
    }

    fn pitch(&self) -> f64 {
        f64::from(self.height) + f64::from(self.spacing)
    }

    pub(crate) fn top(&self, index: usize) -> f64 {
        self.base_top(index)
            + self.section_at(index).map_or(0.0, |section| {
                section.heading_count as f64 * f64::from(self.section_height)
            })
    }

    fn base_top(&self, index: usize) -> f64 {
        self.row_heights.as_ref().map_or_else(
            || (index / self.columns) as f64 * self.pitch(),
            |heights| heights.top(index),
        )
    }

    fn section_at(&self, index: usize) -> Option<&ListSection> {
        let sections = self.sections.as_deref()?;
        sections
            .partition_point(|section| section.index <= index)
            .checked_sub(1)
            .and_then(|index| sections.get(index))
    }

    pub(crate) fn leading_clearance(&self, index: usize, viewport: f32) -> f32 {
        if self.sticky_sections
            && self.section_height < viewport
            && self
                .section_at(index)
                .is_some_and(|section| !section.label.is_empty())
        {
            self.section_height
        } else {
            0.0
        }
    }

    pub(crate) fn reveal(&self, scroll: ScrollLayout, index: usize, viewport: f32) -> ScrollLayout {
        let leading = self.leading_clearance(index, viewport);
        let positioned = ScrollLayout::new(scroll.offset + leading, scroll.max_offset + leading)
            .revealing(
                self.top(index) as f32,
                self.height(index),
                viewport - leading,
            );
        ScrollLayout::new(positioned.offset - leading, scroll.max_offset)
    }

    pub(crate) fn headers(
        &self,
        viewport: Rect,
        offset: f32,
        row_count: usize,
    ) -> Option<Box<[ListSectionHeader]>> {
        let sections = self.sections.as_deref()?;
        let h = f64::from(self.section_height);
        let offset = f64::from(offset);
        let end = offset + f64::from(viewport.height);
        let start = |section: &ListSection| {
            self.base_top(section.index)
                + (section.heading_count - usize::from(!section.label.is_empty())) as f64 * h
        };
        let first = sections
            .partition_point(|section| start(section) <= offset)
            .saturating_sub(1);
        let mut headers = Vec::new();
        for (index, section) in sections.iter().enumerate().skip(first) {
            let natural = start(section);
            if natural >= end {
                break;
            }
            if section.label.is_empty() {
                continue;
            }
            let pinned = index == first
                && natural < offset
                && self.sticky_sections
                && self.section_height < viewport.height;
            let top = if pinned {
                sections
                    .get(index + 1)
                    .map_or(offset, |next| offset.min(start(next) - h))
            } else {
                natural
            };
            if top + h > offset {
                headers.push(ListSectionHeader {
                    first_row: section.index,
                    row_count: sections
                        .get(index + 1)
                        .map_or(row_count, |next| next.index)
                        .saturating_sub(section.index),
                    key: section.key,
                    label: section.label.clone(),
                    bounds: Rect::new(
                        viewport.x,
                        viewport.y + (top - offset) as f32,
                        viewport.width,
                        self.section_height,
                    ),
                    pinned,
                    content: None,
                });
            }
        }
        (!headers.is_empty()).then(|| headers.into_boxed_slice())
    }

    pub(crate) fn height(&self, index: usize) -> f32 {
        self.row_heights
            .as_ref()
            .map_or(self.height, |heights| heights.height(index))
    }

    pub(crate) fn extent(&self, count: usize) -> f32 {
        count.checked_sub(1).map_or(0.0, |last| {
            (self.top(last) + f64::from(self.height(last))) as f32
        })
    }

    pub(crate) fn bounds(&self, index: usize, viewport: Rect, offset: f32) -> Rect {
        let x = (index % self.columns) as f32 * (self.width + self.spacing);
        let y = (self.top(index) - f64::from(offset)) as f32;
        Rect::new(
            viewport.x + x,
            viewport.y + y,
            self.width,
            self.height(index),
        )
    }

    pub(crate) fn visible(&self, top: f64, bottom: f64, count: usize) -> Range<usize> {
        if self.row_heights.is_some() || self.sections.is_some() {
            let first = self.row_at(top, count);
            let end = boundary(count, |index| self.top(index) < bottom);
            return first..end;
        }
        let first = ((top + f64::from(self.spacing)) / self.pitch()).floor() as usize;
        let end = (bottom / self.pitch()).ceil() as usize;
        first.saturating_mul(self.columns).min(count)..end.saturating_mul(self.columns).min(count)
    }

    fn row_at(&self, y: f64, count: usize) -> usize {
        boundary(count, |index| {
            self.top(index) + f64::from(self.height(index)) <= y
        })
    }

    pub(crate) fn page(&self, row: usize, viewport: f32, count: usize, forward: bool) -> usize {
        if self.row_heights.is_some() || self.sections.is_some() {
            let height = f64::from(viewport);
            let y = self.top(row) + if forward { height } else { -height };
            let next = self.row_at(y, count);
            return if forward {
                next.max(row.saturating_add(1)).min(count - 1)
            } else {
                next.min(row.saturating_sub(1))
            };
        }
        let step = ((f64::from(viewport) / self.pitch()).floor() as usize)
            .max(1)
            .saturating_mul(self.columns);
        if forward {
            row.saturating_add(step).min(count - 1)
        } else {
            row.saturating_sub(step)
        }
    }
}

fn boundary(count: usize, before: impl Fn(usize) -> bool) -> usize {
    let mut first = 0;
    let mut end = count;
    while first < end {
        let middle = first + (end - first) / 2;
        if before(middle) {
            first = middle + 1;
        } else {
            end = middle;
        }
    }
    first
}
