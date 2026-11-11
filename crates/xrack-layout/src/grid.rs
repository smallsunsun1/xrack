use crate::{
    Rect, Size, TextMeasurer, dimension,
    linear::{AxisInfo, Metrics, Padding, fit_axis, measure as measure_node},
    participates_in_flow,
};
use xrack_runtime::{GridCellSlot, GridPlanner, GridTrackInput, Node};

struct Cell<'a> {
    node: &'a Node,
    index: usize,
    child_index: Option<usize>,
    row: usize,
    col: usize,
    rows: usize,
    cols: usize,
}

impl<'a> Cell<'a> {
    fn from_slot(node: &'a Node, slot: GridCellSlot) -> Self {
        Self {
            node,
            index: slot.index,
            child_index: slot.child_index,
            row: slot.position.row,
            col: slot.position.col,
            rows: slot.position.rows,
            cols: slot.position.cols,
        }
    }
}

fn cells(node: &Node) -> Vec<Cell<'_>> {
    if let Some(plan) = node.static_grid()
        && let Some(cells) = plan
            .iter()
            .map(|&slot| {
                let mut child = node.children.get(slot.index)?;
                if let Some(index) = slot.child_index {
                    child = child.child_at(index)?;
                }
                Some(Cell::from_slot(child, slot))
            })
            .collect()
    {
        return cells;
    }

    let mut planner = GridPlanner::default();
    let mut cells = Vec::new();
    let mut add = |planner: &mut GridPlanner, node, index, child_index| {
        let node: &Node = node;
        if !participates_in_flow(node) {
            return;
        }
        let input = GridTrackInput {
            row: node.int("row"),
            col: node.int("col"),
            rowspan: node.int("rowspan"),
            colspan: node.int("colspan"),
        };
        if let Some(position) = planner.place(input) {
            cells.push(Cell::from_slot(
                node,
                GridCellSlot {
                    index,
                    child_index,
                    position,
                },
            ));
        }
    };
    for (index, child) in node.children.iter().enumerate() {
        if child.kind == "Row" {
            if !child.boolean("visible", true) {
                continue;
            }
            planner.begin_row();
            for (child_index, node) in child.children.iter().enumerate() {
                add(&mut planner, node, index, Some(child_index));
            }
            planner.end_row();
        } else {
            add(&mut planner, child, index, None);
        }
    }
    cells
}

struct Tracks {
    x: Vec<AxisInfo>,
    y: Vec<AxisInfo>,
    x_gap: f32,
    y_gap: f32,
}

impl Tracks {
    fn measure(
        node: &Node,
        cells: &[Cell<'_>],
        available: Size,
        text: &mut impl TextMeasurer,
    ) -> (Self, Vec<Metrics>) {
        let gap = dimension(node, "spacing").unwrap_or(0.0);
        let mut tracks = Self {
            x: vec![
                AxisInfo::flexible(0.0);
                cells
                    .iter()
                    .map(|cell| cell.col + cell.cols)
                    .max()
                    .unwrap_or(0)
            ],
            y: vec![
                AxisInfo::flexible(0.0);
                cells
                    .iter()
                    .map(|cell| cell.row + cell.rows)
                    .max()
                    .unwrap_or(0)
            ],
            x_gap: dimension(node, "horizontal-spacing").unwrap_or(gap),
            y_gap: dimension(node, "vertical-spacing").unwrap_or(gap),
        };
        let metrics: Vec<_> = cells
            .iter()
            .map(|cell| measure_node(cell.node, available, text))
            .collect();
        for (grid_axis, gap, horizontal) in [
            (&mut tracks.x, tracks.x_gap, true),
            (&mut tracks.y, tracks.y_gap, false),
        ] {
            let mut contributions: Vec<_> = cells
                .iter()
                .zip(&metrics)
                .map(|(cell, info)| {
                    if horizontal {
                        (cell.col, cell.cols, info.x)
                    } else {
                        (cell.row, cell.rows, info.y)
                    }
                })
                .collect();
            contributions.sort_by_key(|(_, span, _)| *span);
            for (start, span, info) in contributions {
                let covered = &mut grid_axis[start..start + span];
                let internal_gaps = gap * (span - 1) as f32;
                let min = covered.iter().map(|track| track.min).sum::<f32>();
                let preferred = covered.iter().map(|track| track.preferred).sum::<f32>();
                let min_extra = (info.min - min - internal_gaps).max(0.0) / span as f32;
                let pref_extra =
                    (info.preferred - preferred - internal_gaps).max(0.0) / span as f32;
                for track in covered {
                    track.min += min_extra;
                    track.preferred = (track.preferred + pref_extra).max(track.min);
                    track.stretch = track.stretch.max(info.stretch / span as f32);
                }
            }
        }
        (tracks, metrics)
    }
}

pub(crate) fn measure(node: &Node, available: Size, text: &mut impl TextMeasurer) -> Metrics {
    let padding = Padding::read(node);
    let inner = padding.inner_size(available);
    let (tracks, _) = Tracks::measure(node, &cells(node), inner, text);
    let padding = padding.size();
    let aggregate = |tracks: &[AxisInfo], gap: f32, padding: f32| {
        let extra = gap * tracks.len().saturating_sub(1) as f32 + padding;
        let mut total =
            AxisInfo::flexible(tracks.iter().map(|track| track.preferred).sum::<f32>() + extra);
        total.min = tracks.iter().map(|track| track.min).sum::<f32>() + extra;
        total
    };
    Metrics {
        x: aggregate(&tracks.x, tracks.x_gap, padding.width),
        y: aggregate(&tracks.y, tracks.y_gap, padding.height),
    }
}

pub(crate) struct Placement {
    pub(crate) bounds: Rect,
    pub(crate) row_children: Option<Vec<CellPlacement>>,
    pub(crate) clip: Option<std::sync::Arc<crate::table::PaneClip>>,
}

impl Placement {
    pub(crate) fn plain(bounds: Rect) -> Self {
        Self {
            bounds,
            row_children: None,
            clip: None,
        }
    }
}

#[derive(Clone)]
pub(crate) struct CellPlacement {
    pub(crate) bounds: Rect,
    pub(crate) clip: Option<std::sync::Arc<crate::table::PaneClip>>,
}

pub(crate) fn children(node: &Node, bounds: Rect, text: &mut impl TextMeasurer) -> Vec<Placement> {
    let inner = Padding::read(node).inner_rect(bounds);
    let size = Size::new(inner.width, inner.height);
    let cells = cells(node);
    let (tracks, metrics) = Tracks::measure(node, &cells, size, text);
    let fit = |tracks: Vec<AxisInfo>, gap: f32, extent: f32| {
        let gaps = gap * tracks.len().saturating_sub(1) as f32;
        let sizes = fit_axis(tracks, extent - gaps);
        let mut offsets = Vec::with_capacity(sizes.len() + 1);
        offsets.push(0.0);
        for size in sizes {
            offsets.push(offsets.last().unwrap() + size + gap);
        }
        offsets
    };
    let x = fit(tracks.x, tracks.x_gap, inner.width);
    let y = fit(tracks.y, tracks.y_gap, inner.height);
    let hidden = Rect::new(inner.x, inner.y, 0.0, 0.0);
    let mut placements = node
        .children
        .iter()
        .map(|child| {
            if child.kind == "Row" && child.boolean("visible", true) {
                Placement {
                    bounds: inner,
                    row_children: Some(vec![
                        CellPlacement {
                            bounds: hidden,
                            clip: None
                        };
                        child.children.len()
                    ]),
                    clip: None,
                }
            } else {
                Placement::plain(hidden)
            }
        })
        .collect::<Vec<_>>();
    for (cell, info) in cells.into_iter().zip(metrics) {
        let width = x[cell.col + cell.cols] - x[cell.col] - tracks.x_gap;
        let height = y[cell.row + cell.rows] - y[cell.row] - tracks.y_gap;
        let rect = Rect::new(
            inner.x + x[cell.col],
            inner.y + y[cell.row],
            info.x.clamp(width),
            info.y.clamp(height),
        );
        let placement = &mut placements[cell.index];
        if let Some(child) = cell.child_index {
            placement.row_children.as_mut().unwrap()[child].bounds = rect;
        } else {
            placement.bounds = rect;
        }
    }
    placements
}
