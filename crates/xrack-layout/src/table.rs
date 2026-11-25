use crate::{
    LayoutNode, Point, Rect, ScrollLayout, TextMeasurer, TransformSpace,
    linear::{AxisInfo, fit_axis},
    place_in_direction,
};
use std::rc::Rc;
use xrack_runtime::{
    LayoutDirection, ListMetrics, Node, SharedNodeIdentity, TableColumnOrder, TableColumnWindow,
    TableHeader, resizable_table_column,
};

/// Shared by the row layout, header painting, and header hit testing.
#[derive(Clone, Debug, PartialEq)]
pub struct TableGeometry {
    pub header: Rect,
    pub body: Rect,
    pub row_header: Rect,
    pub columns: Vec<Rect>,
    pub horizontal: ScrollLayout,
    direction: LayoutDirection,
    order: TableColumnOrder,
    viewport_columns: Vec<usize>,
    hidden_positions: Vec<usize>,
    frozen: Vec<bool>,
    frozen_count: usize,
    frozen_end: usize,
    frozen_width: f32,
}

const MAX_COLUMN_WIDTH: f32 = 1_000_000.0;
const GRIP_RADIUS: f32 = 5.0;
const MIN_SCROLLABLE_WIDTH: f32 = 32.0;

pub(crate) struct PaneClip {
    pub bounds: Rect,
    pub space: Option<Rc<TransformSpace>>,
}

#[derive(Default)]
pub(super) struct ColumnSizing {
    input: Option<(usize, f32)>,
    tracks: Vec<ColumnTrack>,
    fitted_viewport: Option<f32>,
    fitted: Vec<f32>,
    widths: Vec<f32>,
    total: f32,
    geometry: ColumnGeometryCache,
    prepared_nodes: Option<PreparedTableNodes>,
    prepared_rows: Option<PreparedTableRows>,
    pub(super) used: bool,
}

struct PreparedTableNodes {
    source: SharedNodeIdentity,
    frames: [Option<Node>; 2],
    next: usize,
    used: bool,
}

pub(super) struct PreparedTableRows {
    order: TableColumnOrder,
    scope: u64,
    headers: bool,
    first: usize,
    previous_first: usize,
    current: Vec<Option<PreparedRow>>,
    previous: Vec<Option<PreparedRow>>,
    used: bool,
}

struct PreparedRow {
    source: SharedNodeIdentity,
    selection: [bool; 2],
    node: Node,
}

impl PreparedTableRows {
    pub(super) fn take(
        &mut self,
        position: usize,
        source: &Node,
        selection: [bool; 2],
    ) -> Option<Node> {
        let slot = self
            .previous
            .get_mut(position.checked_sub(self.previous_first)?)?;
        if !slot
            .as_ref()
            .is_some_and(|row| row.selection == selection && row.source.matches(source))
        {
            return None;
        }
        let row = slot.take()?;
        let node = row.node.clone();
        self.current.push(Some(row));
        Some(node)
    }

    pub(super) fn push(
        &mut self,
        source: Option<SharedNodeIdentity>,
        selection: [bool; 2],
        node: &Node,
    ) {
        self.current.push(source.map(|source| PreparedRow {
            source,
            selection,
            node: node.clone(),
        }));
    }
}

struct ColumnTrack {
    identity: Option<SharedNodeIdentity>,
    sizing: AxisInfo,
}

impl ColumnSizing {
    pub(super) fn begin_frame(&mut self) {
        if let Some(pool) = &mut self.prepared_nodes {
            pool.used = false;
        }
        if let Some(rows) = &mut self.prepared_rows {
            rows.used = false;
        }
    }

    pub(super) fn finish_frame(&mut self) {
        if self.prepared_nodes.as_ref().is_some_and(|pool| !pool.used) {
            self.prepared_nodes = None;
        }
        if let Some(rows) = &mut self.prepared_rows {
            if rows.used {
                std::mem::swap(&mut rows.previous, &mut rows.current);
                rows.previous_first = rows.first;
                rows.current.clear();
            } else {
                self.prepared_rows = None;
            }
        }
    }

    pub(super) fn invalidate_scene(&mut self) {
        self.used = false;
        self.input = None;
    }

    pub(super) fn reuse_prepared_node(&mut self, node: &mut Node) -> usize {
        let pool = self
            .prepared_nodes
            .get_or_insert_with(|| PreparedTableNodes {
                source: node.snapshot_identity(),
                frames: [None, None],
                next: 0,
                used: true,
            });
        if !pool.source.matches(node) {
            pool.source = node.snapshot_identity();
            pool.frames = [None, None];
            pool.next = 0;
        }
        let slot = pool
            .frames
            .iter()
            .position(|frame| frame.as_ref().is_none_or(Node::owns_storage))
            .unwrap_or(pool.next);
        if let Some(recycled) = pool.frames[slot].take().filter(Node::owns_storage) {
            // The caller replaces the prepared rows and list metrics. All
            // other fields still belong to the same immutable base node.
            *node = recycled;
        }
        pool.next = (slot + 1) % pool.frames.len();
        pool.used = true;
        slot
    }

    pub(super) fn remember_prepared_node(&mut self, slot: usize, node: &Node) {
        if let Some(pool) = &mut self.prepared_nodes {
            pool.frames[slot] = Some(node.clone());
        }
    }

    pub(super) fn prepare_rows(
        &mut self,
        order: &TableColumnOrder,
        scope: u64,
        headers: bool,
        first: usize,
    ) -> &mut PreparedTableRows {
        let rows = self.prepared_rows.get_or_insert_with(|| PreparedTableRows {
            order: order.clone(),
            scope,
            headers,
            first,
            previous_first: first,
            current: Vec::new(),
            previous: Vec::new(),
            used: false,
        });
        if &rows.order != order || rows.scope != scope || rows.headers != headers {
            rows.previous.clear();
            rows.order = order.clone();
            rows.scope = scope;
            rows.headers = headers;
        }
        rows.first = first;
        rows.current.clear();
        rows.used = true;
        rows
    }

    fn resolve(&mut self, header: &TableHeader, width: f32) {
        self.used = true;
        let input = (std::ptr::from_ref(header).addr(), width);
        if self.input != Some(input) {
            self.fit(&header.columns, width);
            // Manual overrides are applied after fitting the declared tracks.
            // A drag can change the overrides without invalidating that fit.
            self.widths.clone_from(&self.fitted);
            apply_width_overrides(&mut self.widths, header, width);
            self.total = self.widths.iter().sum();
            self.input = Some(input);
            self.geometry = ColumnGeometryCache::default();
        }
    }

    fn fit(&mut self, columns: &[Node], viewport: f32) {
        let same_viewport = self.fitted_viewport == Some(viewport);
        let mut refit = !same_viewport || self.tracks.len() != columns.len();
        self.tracks.truncate(columns.len());
        for (track, column) in self.tracks.iter_mut().zip(columns) {
            let shared = track
                .identity
                .as_ref()
                .is_some_and(|identity| identity.matches(column));
            if shared && same_viewport {
                continue;
            }
            let sizing = column_track(column, viewport);
            refit |= track.sizing != sizing;
            track.sizing = sizing;
            if !shared {
                track.identity = column.shared_identity();
            }
        }
        self.tracks.extend(
            columns[self.tracks.len()..]
                .iter()
                .map(|column| ColumnTrack {
                    identity: column.shared_identity(),
                    sizing: column_track(column, viewport),
                }),
        );
        if refit {
            self.fitted = fit_axis(self.tracks.iter().map(|track| track.sizing), viewport);
            self.fitted_viewport = Some(viewport);
        }
    }
}

pub(crate) fn column_limits(column: &Node, viewport_width: f32) -> (f32, f32) {
    column_limits_with_width(
        column,
        viewport_width,
        column.relative_length("width", viewport_width),
    )
}

fn column_limits_with_width(column: &Node, viewport_width: f32, width: Option<f32>) -> (f32, f32) {
    let default_min = width.map_or(24.0, |width| width.clamp(0.0, 24.0));
    let min = column
        .relative_length("min-width", viewport_width)
        .unwrap_or(default_min)
        .max(0.0);
    let max = column
        .relative_length("max-width", viewport_width)
        .unwrap_or(MAX_COLUMN_WIDTH)
        .max(min);
    (min, max)
}

fn column_track(column: &Node, viewport: f32) -> AxisInfo {
    if !column.is_visible() {
        return AxisInfo::fixed(0.0);
    }
    let width = column.relative_length("width", viewport);
    let (min, max) = column_limits_with_width(column, viewport, width);
    if let Some(width) = width {
        return AxisInfo::fixed(width.clamp(min, max));
    }
    let preferred = column
        .relative_length("preferred-width", viewport)
        .unwrap_or(min)
        .clamp(min, max);
    let stretch = column.float("horizontal-stretch").unwrap_or(1.0).max(0.0);
    AxisInfo {
        min,
        preferred,
        max: if stretch == 0.0 { preferred } else { max },
        stretch,
    }
}

fn apply_width_overrides(widths: &mut [f32], header: &TableHeader, viewport: f32) {
    for ((width, override_width), column) in widths
        .iter_mut()
        .zip(&header.column_widths)
        .zip(&header.columns)
    {
        if let Some(override_width) = override_width
            && column.is_visible()
        {
            let (min, max) = column_limits(column, viewport);
            *width = (*override_width as f32).clamp(min, max);
        }
    }
}

#[doc(hidden)]
pub fn table_heading_font_size(text: &impl TextMeasurer) -> f32 {
    (text.default_font_size().unwrap_or(15.0) * text.text_scale()).clamp(1.0, 512.0)
}

pub(crate) fn header_height(node: &Node, height: f32, preferred: f32) -> f32 {
    const BASELINE: f32 = 34.0;
    if let Some(explicit) = node.length("header-height") {
        return explicit.clamp(16.0, 128.0).min(height);
    }
    // Keep room for a row when the default header grows in a short viewport.
    let available = if preferred > BASELINE {
        height
            - (height - BASELINE)
                .max(0.0)
                .min(ListMetrics::row_height(node))
    } else {
        height
    };
    preferred.clamp(16.0, 128.0).min(available)
}

#[derive(Default)]
struct ColumnGeometryCache {
    horizontal_offset: Option<f32>,
    placement: Option<TablePlacement>,
    layout: Option<TableGeometry>,
}

#[derive(PartialEq)]
struct TablePlacement {
    header: Rect,
    body: Rect,
    row_header: Rect,
    horizontal: ScrollLayout,
    direction: LayoutDirection,
    frozen_columns: usize,
    frozen_limit: f32,
    order: TableColumnOrder,
}

impl TableGeometry {
    pub fn new(node: &Node, bounds: Rect) -> Option<Self> {
        Self::with_offset(
            node,
            bounds,
            bounds,
            0.0,
            node.layout_direction().unwrap_or_default(),
            header_height(node, bounds.height, 34.0),
            None,
        )
    }

    pub(crate) fn with_offset(
        node: &Node,
        bounds: Rect,
        clip: Rect,
        requested: f32,
        direction: LayoutDirection,
        header_height: f32,
        sizing: Option<&mut ColumnSizing>,
    ) -> Option<Self> {
        let header = node.table_header.as_ref()?;
        let rows = Rect::new(
            bounds.x,
            bounds.y + header_height,
            bounds.width,
            bounds.height - header_height,
        );
        let gutter = node
            .relative_length("row-header-width", bounds.width)
            .filter(|width| width.is_finite())
            .unwrap_or_default()
            .clamp(0.0, (bounds.width - MIN_SCROLLABLE_WIDTH).max(0.0));
        let body = place_in_direction(
            Rect::new(rows.x + gutter, rows.y, rows.width - gutter, rows.height),
            rows,
            direction,
        );
        let row_header = place_in_direction(
            Rect::new(rows.x, rows.y, gutter, rows.height),
            rows,
            direction,
        );
        let header_bounds = Rect::new(body.x, bounds.y, body.width, body.y - bounds.y);
        let mut widths;
        let (widths, total_width, cache) = if let Some(sizing) = sizing {
            sizing.resolve(header, body.width);
            (
                sizing.widths.as_slice(),
                sizing.total,
                Some(&mut sizing.geometry),
            )
        } else {
            let tracks = header
                .columns
                .iter()
                .map(|column| column_track(column, body.width));
            widths = fit_axis(tracks, body.width);
            apply_width_overrides(&mut widths, header, body.width);
            (widths.as_slice(), widths.iter().sum(), None)
        };
        let horizontal = ScrollLayout::new(requested, total_width - body.width);
        let order = node.table_column_order.clone();
        let requested_frozen =
            usize::try_from(node.int("frozen-columns").unwrap_or(0)).unwrap_or(0);
        let visible = place_in_direction(body.intersect(clip), body, direction);
        let visible_right = (visible.x + visible.width - body.x).clamp(0.0, body.width);
        let limit = if total_width > visible_right {
            (visible_right - MIN_SCROLLABLE_WIDTH).max(0.0)
        } else {
            visible_right
        };
        // Keep a snapshot once the placement repeats. During continuous
        // horizontal motion, check the offset before constructing a key.
        let cache = cache.and_then(|cache| {
            if cache.horizontal_offset.replace(horizontal.offset) != Some(horizontal.offset) {
                cache.placement = None;
                cache.layout = None;
                return None;
            }
            let placement = TablePlacement {
                header: header_bounds,
                body,
                row_header,
                horizontal,
                direction,
                frozen_columns: requested_frozen,
                frozen_limit: limit,
                order: order.clone(),
            };
            if cache.placement.as_ref() == Some(&placement) {
                Some(cache)
            } else {
                cache.placement = Some(placement);
                cache.layout = None;
                None
            }
        });
        if let Some(layout) = cache.as_ref().and_then(|cache| cache.layout.as_ref()) {
            return Some(layout.clone());
        }
        let mut frozen_width = 0.0;
        let mut frozen_count = 0;
        let mut frozen_end = 0;
        let mut frozen = Vec::new();
        for (visual, column) in order
            .iter(widths.len())
            .enumerate()
            .filter(|&(_, column)| widths[column] > 0.0)
            .take(requested_frozen)
        {
            let end = frozen_width + widths[column];
            if end > limit {
                break;
            }
            if frozen.is_empty() {
                frozen.resize(widths.len(), false);
            }
            frozen[column] = true;
            frozen_width = end;
            frozen_count += 1;
            frozen_end = visual + 1;
        }
        let mut x = body.x;
        let mut viewport_columns = Vec::new();
        let mut hidden_positions = Vec::new();
        let mut place_column = |visual, index| {
            if visual == frozen_end {
                x -= horizontal.offset;
            }
            let width = widths[index];
            if width == 0.0 {
                hidden_positions.push(visual);
            }
            let column = place_in_direction(
                Rect::new(x, bounds.y, width, header_bounds.height),
                body,
                direction,
            );
            if width > 0.0 && column.x <= body.x + body.width && column.x + width >= body.x {
                viewport_columns.push(index);
            }
            x += width;
            column
        };
        let columns = if let Some(permutation) = order.as_slice(widths.len()) {
            let mut columns = vec![Rect::default(); widths.len()];
            for (visual, &index) in permutation.iter().enumerate() {
                columns[index] = place_column(visual, index);
            }
            viewport_columns.sort_unstable();
            columns
        } else {
            // Declaration order needs no placeholder rectangles or sort.
            (0..widths.len())
                .map(|index| place_column(index, index))
                .collect()
        };
        let layout = Self {
            header: header_bounds,
            body,
            row_header,
            columns,
            horizontal,
            direction,
            order,
            viewport_columns,
            hidden_positions,
            frozen,
            frozen_count,
            frozen_end,
            frozen_width,
        };
        if let Some(cache) = cache {
            cache.layout = Some(layout.clone());
        }
        Some(layout)
    }

    pub(crate) fn rows_area(&self) -> Rect {
        Rect::new(
            self.row_header.x.min(self.body.x),
            self.body.y,
            self.row_header.width + self.body.width,
            self.body.height,
        )
    }

    pub(crate) fn column_window(&self, clip: Rect) -> TableColumnWindow {
        let clip = self.body.intersect(clip);
        let columns = self.columns_in_viewport().filter(|&index| {
            let column = self.columns[index];
            if column.width <= 0.0
                || column.x + column.width <= clip.x
                || column.x >= clip.x + clip.width
            {
                return false;
            }
            let cell = Rect::new(column.x, self.body.y, column.width, self.body.height);
            let visible = cell.intersect(self.column_clip(index, clip));
            visible.width > 0.0 && visible.height > 0.0
        });
        TableColumnWindow::from_indices(self.columns.len(), columns.collect())
    }

    /// Heading and cell candidates in declaration order, before pane clipping.
    #[doc(hidden)]
    pub fn columns_in_viewport(&self) -> impl Iterator<Item = usize> + '_ {
        self.viewport_columns
            .iter()
            .copied()
            .take_while(|&index| index < self.columns.len())
    }

    pub fn row_header_cell(&self, row: Rect) -> Rect {
        Rect::new(self.row_header.x, row.y, self.row_header.width, row.height)
    }

    /// Visible part of a column heading, constrained to its table pane.
    #[doc(hidden)]
    pub fn header_cell_bounds(&self, column: usize, clip: Rect) -> Option<Rect> {
        let cell = self
            .columns
            .get(column)?
            .intersect(self.column_clip(column, self.header.intersect(clip)));
        (cell.width > 0.0 && cell.height > 0.0).then_some(cell)
    }

    pub fn visual_columns(&self) -> impl DoubleEndedIterator<Item = usize> + ExactSizeIterator {
        self.order.iter(self.columns.len())
    }

    pub fn visual_index(&self, column: usize) -> Option<usize> {
        self.order.visual_index(self.columns.len(), column)
    }

    pub fn is_column_visible(&self, column: usize) -> bool {
        self.columns
            .get(column)
            .is_some_and(|bounds| bounds.width > 0.0)
    }

    pub fn visible_columns(&self) -> impl DoubleEndedIterator<Item = usize> {
        self.visual_columns()
            .filter(|&column| self.is_column_visible(column))
    }

    pub fn visible_column_count(&self) -> usize {
        self.columns.len() - self.hidden_positions.len()
    }

    pub fn visible_column_index(&self, column: usize) -> Option<usize> {
        if !self.is_column_visible(column) {
            return None;
        }
        let visual = self.visual_index(column)?;
        Some(
            visual
                - self
                    .hidden_positions
                    .partition_point(|&hidden| hidden < visual),
        )
    }

    pub fn is_frozen(&self, column: usize) -> bool {
        self.frozen.get(column).copied().unwrap_or(false)
    }

    pub fn mirrored(&self) -> bool {
        self.direction == LayoutDirection::RightToLeft
    }

    pub(crate) fn logical_delta(&self, delta: f32) -> f32 {
        if self.mirrored() { -delta } else { delta }
    }

    fn mirror_x(&self, x: f32) -> f32 {
        if self.mirrored() {
            self.body.x + self.body.width - (x - self.body.x)
        } else {
            x
        }
    }

    fn mirror_rect(&self, rect: Rect) -> Rect {
        place_in_direction(rect, self.body, self.direction)
    }

    /// Position row or header content from the logical leading edge of its cell.
    #[doc(hidden)]
    pub fn content_bounds(&self, cell: Rect, start: f32, width: f32) -> Rect {
        place_in_direction(
            Rect::new(cell.x + start, cell.y, width, cell.height),
            cell,
            self.direction,
        )
    }

    fn scrollable_band(&self, band: Rect) -> Rect {
        let width = if self.frozen_count == self.visible_column_count() {
            0.0
        } else {
            (band.width - self.frozen_width).max(0.0)
        };
        self.content_bounds(band, self.frozen_width, width)
    }

    pub fn scrollable_header(&self) -> Rect {
        self.scrollable_band(self.header)
    }

    pub fn scrollable_body(&self) -> Rect {
        self.scrollable_band(self.body)
    }

    pub(crate) fn frozen_body(&self) -> Rect {
        self.content_bounds(self.body, 0.0, self.frozen_width)
    }

    /// Clip a logical column to the pane it currently belongs to.
    pub fn column_clip(&self, column: usize, band: Rect) -> Rect {
        if self.frozen_width == 0.0 {
            return band;
        }
        let pane = if self.is_frozen(column) {
            self.frozen_body()
        } else {
            self.scrollable_body()
        };
        Rect::new(pane.x, band.y, pane.width, band.height).intersect(band)
    }

    pub fn frozen_boundary(&self) -> Option<f32> {
        (self.frozen_width > 0.0 && self.frozen_count < self.visible_column_count())
            .then(|| self.mirror_x(self.header.x + self.frozen_width))
    }

    pub(crate) fn column_scroll_extent(&self, column: usize) -> Option<(f32, f32)> {
        let bounds = self
            .columns
            .get(column)
            .filter(|bounds| bounds.width > 0.0 && !self.is_frozen(column))?;
        let viewport = self.mirror_rect(self.scrollable_body());
        let bounds = self.mirror_rect(*bounds);
        Some((bounds.x - viewport.x + self.horizontal.offset, bounds.width))
    }

    pub(crate) fn reveal_column(&self, column: usize) -> f32 {
        let Some((start, width)) = self.column_scroll_extent(column) else {
            return self.horizontal.offset;
        };
        let available = self.scrollable_body().width;
        self.horizontal
            .revealing(start, width.min(available), available)
            .offset
    }

    pub(crate) fn column_drop(&self, from: usize, point: Point) -> Option<(usize, f32)> {
        if !self.header.contains(point) {
            return None;
        }
        let x = self.mirror_x(point.x);
        let frozen = self.frozen_width > 0.0 && x < self.header.x + self.frozen_width;
        let (start, end, pane) = if frozen {
            (
                0,
                self.frozen_end,
                Rect::new(
                    self.header.x,
                    self.header.y,
                    self.frozen_width,
                    self.header.height,
                ),
            )
        } else {
            (
                self.frozen_end,
                self.columns.len(),
                self.mirror_rect(self.scrollable_header()),
            )
        };
        let mut insertion = None;
        for (visual, column) in self
            .visual_columns()
            .enumerate()
            .skip(start)
            .take(end - start)
        {
            if !self.is_column_visible(column) {
                continue;
            }
            let bounds = self.mirror_rect(self.columns[column]);
            let visible = if self.frozen_width == 0.0 {
                bounds
            } else {
                bounds.intersect(pane)
            };
            if bounds.x >= pane.x + pane.width
                || visible.width > 0.0 && x < visible.x + visible.width * 0.5
            {
                insertion = Some((visual, bounds.x));
                break;
            }
            insertion = Some((visual + 1, bounds.x + bounds.width));
        }
        let (slot, marker) = insertion?;
        let to = slot - usize::from(slot > from);
        if from == to {
            return None;
        }
        Some((to, self.mirror_x(marker.clamp(pane.x, pane.x + pane.width))))
    }

    pub(crate) fn column_at(&self, point: Point) -> Option<usize> {
        if !self.header.contains(point) {
            return None;
        }
        self.columns_in_viewport()
            .filter(|&column| {
                self.columns[column]
                    .intersect(self.column_clip(column, self.header))
                    .contains(point)
            })
            .min_by_key(|&column| self.visual_index(column))
    }

    pub(crate) fn body_column_at(&self, point: Point) -> Option<usize> {
        if !self.body.contains(point) {
            return None;
        }
        self.column_at(Point {
            x: point.x,
            y: self.header.y,
        })
    }

    pub(crate) fn resize_at(&self, node: &Node, clip: Rect, point: Point) -> Option<usize> {
        let visible = clip.intersect(self.header);
        if !visible.contains(point) || !node.boolean("resizable-columns", false) {
            return None;
        }
        let pointer_x = self.mirror_x(point.x);
        // The grip must be inside its pane. The viewport index includes
        // columns touching either viewport edge, even if their handles are
        // the only part reachable from inside the header.
        self.columns_in_viewport()
            .filter_map(|index| {
                let bounds = self.mirror_rect(self.columns[index]);
                let edge = bounds.x + bounds.width;
                let distance = (pointer_x - edge).abs();
                if !(bounds.width > 0.0 && distance <= GRIP_RADIUS) {
                    return None;
                }
                let pane = self.mirror_rect(self.column_clip(index, visible));
                if !(edge > pane.x && edge <= pane.x + pane.width) {
                    return None;
                }
                let column = resizable_table_column(node, index)?;
                let (min, max) = column_limits(column, self.header.width);
                (max > min).then_some((index, distance))
            })
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(index, _)| index)
    }

    pub(crate) fn row_resize_at(
        &self,
        node: &Node,
        layout: &LayoutNode,
        point: Point,
    ) -> Option<usize> {
        let visible = self.rows_area().intersect(layout.clip);
        if !visible.contains(point) || !node.boolean("resizable-rows", false) {
            return None;
        }
        node.children
            .iter()
            .zip(&layout.children)
            .enumerate()
            .filter_map(|(slot, (row, item))| {
                row.row_key?;
                let end = item.bounds.y + item.bounds.height;
                let distance = (end - point.y).abs();
                (item.visible
                    && item.bounds.height > 0.0
                    && end > visible.y
                    && end <= visible.y + visible.height
                    && distance <= GRIP_RADIUS)
                    .then_some((slot, distance))
            })
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(slot, _)| slot)
    }
}
