use super::{
    ContainerPaint, Item, ItemAction, ItemEvent, ItemLayout, ItemPaint, ItemRef, MeasureContext,
    PageLayout, RowLayout,
};
use crate::{
    NavigationKey, Rect, Size, TableGeometry, TableSelectionKey, TreeRowGeometry, ViewCells,
    grid::{Placement, RowPlacements},
    linear::Metrics,
    place_in_direction,
    table::PaneClip,
    transform::TransformSpace,
};
use std::rc::Rc;
use xrack_runtime::{
    ControlAction, LayoutDirection, ListMetrics, Node, RowSelection, SelectionTarget,
    TableSelectionAction, TreeAction, TreeSelectionAction,
};

pub(super) struct Rows(pub(super) RowLayout);

impl Item for Rows {
    fn hit_behavior(&self, _node: &Node) -> super::HitBehavior {
        if self.0 == RowLayout::Row {
            super::HitBehavior::ProxyTarget
        } else {
            super::HitBehavior::Normal
        }
    }

    fn layout(&self) -> ItemLayout {
        ItemLayout::Rows(self.0)
    }

    fn focusable(&self, _node: &Node) -> bool {
        self.0 != RowLayout::Row
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> Metrics {
        let size = match self.0 {
            RowLayout::List | RowLayout::StandardList | RowLayout::Tree => Size::new(180.0, 144.0),
            RowLayout::Grid => Size::new(320.0, 200.0),
            RowLayout::Table => Size::new(280.0, 178.0),
            RowLayout::Row => Size::default(),
        };
        cx.intrinsic(size)
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> ItemPaint {
        match self.0 {
            RowLayout::Row => ItemPaint::ListRow,
            RowLayout::Table => ItemPaint::TableView,
            _ => ItemPaint::Rows,
        }
    }

    fn event(&self, item: ItemRef<'_, '_>, event: ItemEvent) -> Option<ItemAction> {
        let action = match event {
            ItemEvent::HitAfterScroll(point) if self.0 == RowLayout::Table => {
                let slot = item.layout.table.as_ref()?.row_resize_at(
                    item.node,
                    item.layout,
                    item.layout.local_to_layout(point),
                )?;
                return Some(ItemAction::TableRowGrip(slot));
            }
            ItemEvent::Navigate(key) if self.0 == RowLayout::Tree => navigate_tree(item, key)?,
            ItemEvent::Activate if self.0 == RowLayout::Tree => toggle_tree(item.node)?,
            ItemEvent::SelectRows(key) if self.0 == RowLayout::Tree => {
                select_tree_rows(item.node, key)?
            }
            ItemEvent::NavigateColumn {
                key,
                focused_column,
            } if self.0 == RowLayout::Table => {
                return navigate_table_column(item, key, focused_column);
            }
            ItemEvent::NavigateRow(key) if self.0 != RowLayout::Row => {
                ControlAction::Select(navigate_row(item, key, self.0 == RowLayout::Grid)?)
            }
            ItemEvent::SelectRows(key)
                if matches!(
                    self.0,
                    RowLayout::List | RowLayout::StandardList | RowLayout::Grid
                ) =>
            {
                ControlAction::TableSelection(select_rows(item.node, key)?)
            }
            _ => return None,
        };
        Some(ItemAction::Control(action))
    }
}

pub(super) struct PageView(pub(super) PageLayout);

impl Item for PageView {
    fn layout(&self) -> ItemLayout {
        ItemLayout::Pages(self.0)
    }

    fn focusable(&self, _node: &Node) -> bool {
        true
    }

    fn paint(&self, _node: &xrack_runtime::Node) -> ItemPaint {
        if self.0 == PageLayout::Swipe {
            ItemPaint::SwipeView
        } else {
            ItemPaint::Default
        }
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> Metrics {
        cx.intrinsic(Size::new(240.0, 136.0))
    }

    fn event(&self, item: ItemRef<'_, '_>, event: ItemEvent) -> Option<ItemAction> {
        let ItemEvent::Navigate(key) = event else {
            return None;
        };
        if self.0 != PageLayout::Swipe || !item.node.boolean("interactive", true) {
            return None;
        }
        let index = crate::swipe_view_selection(item.node, item.layout, key)?;
        Some(ItemAction::Control(xrack_runtime::ControlAction::Select(
            index,
        )))
    }
}

pub(super) struct SplitView;

impl Item for SplitView {
    fn reading_order(&self, node: &Node, _count: usize) -> Option<Vec<usize>> {
        let count = node.split_pane_count();
        Some(
            (0..count)
                .flat_map(|slot| [slot, count + slot])
                .filter(|&slot| slot < node.children.len())
                .collect(),
        )
    }

    fn paint(&self, _node: &Node) -> ItemPaint {
        ItemPaint::Container(ContainerPaint::Split)
    }

    fn layout(&self) -> ItemLayout {
        ItemLayout::Split
    }

    fn measure(&self, cx: &mut MeasureContext<'_>) -> Metrics {
        cx.intrinsic(Size::new(320.0, 180.0))
    }
}

pub(super) struct TableRow;

impl Item for TableRow {
    fn paint(&self, _node: &Node) -> ItemPaint {
        ItemPaint::TableRow
    }

    fn reading_order(&self, node: &Node, count: usize) -> Option<Vec<usize>> {
        node.table_column_order.as_slice(count)?;
        let mut columns: Vec<_> = node
            .materialized_children()
            .map(|(index, _)| index)
            .collect();
        columns.sort_by_key(|&index| node.table_column_order.visual_index(count, index));
        Some(columns)
    }
}

pub(crate) fn arrange_row(
    node: &Node,
    bounds: Rect,
    direction: LayoutDirection,
    mut row_children: Option<RowPlacements>,
) -> Vec<Placement> {
    let tree = TreeRowGeometry::with_layout_direction(node, bounds, direction);
    node.children
        .iter()
        .map(|child| {
            if let Some(geometry) = tree
                && child.kind == "TreeRow"
                && child.control_part("TreeDisclosure").is_some()
            {
                Placement {
                    bounds,
                    clip: None,
                    row_children: Some(RowPlacements::Tree {
                        geometry,
                        has_children: node.boolean("tree-has-children", false),
                    }),
                }
            } else {
                Placement {
                    bounds: tree.map_or(bounds, |tree| tree.content),
                    clip: None,
                    row_children: row_children.take(),
                }
            }
        })
        .collect()
}

fn navigate_row(
    ItemRef { node, layout }: ItemRef<'_, '_>,
    key: NavigationKey,
    grid: bool,
) -> Option<usize> {
    let list = node.list?;
    let last = list.row_count.checked_sub(1)?;
    layout.scroll?;
    let viewport = layout
        .table
        .as_ref()
        .map_or(layout.bounds, |table| table.body);
    let cells = ViewCells::new(node, viewport.width);
    let columns = cells.columns;
    let current = ListMetrics::selection(node)
        .and_then(|value| usize::try_from(value).ok())
        .filter(|&row| row <= last);
    let key = if grid && layout.layout_direction == LayoutDirection::RightToLeft {
        match key {
            NavigationKey::Left => NavigationKey::Right,
            NavigationKey::Right => NavigationKey::Left,
            key => key,
        }
    } else {
        key
    };
    Some(match key {
        NavigationKey::Left if grid => current.map_or(0, |index| {
            if index.is_multiple_of(columns) {
                index
            } else {
                index - 1
            }
        }),
        NavigationKey::Right if grid => current.map_or(0, |index| {
            if (index + 1).is_multiple_of(columns) {
                index
            } else {
                (index + 1).min(last)
            }
        }),
        NavigationKey::Up => current.map_or(0, |index| index.checked_sub(columns).unwrap_or(index)),
        NavigationKey::Down => current.map_or(0, |index| {
            if index / columns == last / columns {
                index
            } else {
                index.saturating_add(columns).min(last)
            }
        }),
        NavigationKey::Home => 0,
        NavigationKey::End => last,
        NavigationKey::PageUp => current.map_or(0, |row| {
            cells.page(row, viewport.height, list.row_count, false)
        }),
        NavigationKey::PageDown => current.map_or(0, |row| {
            cells.page(row, viewport.height, list.row_count, true)
        }),
        _ => return None,
    })
}

fn select_rows(node: &Node, key: TableSelectionKey) -> Option<TableSelectionAction> {
    let selection = node.table_selection.as_ref()?;
    let count = node.list?.row_count;
    Some(match key {
        TableSelectionKey::All if count > 0 => TableSelectionAction::All,
        TableSelectionKey::Clear if !selection.is_empty() => TableSelectionAction::Clear,
        TableSelectionKey::Toggle => {
            let index = usize::try_from(ListMetrics::selection(node)?).ok()?;
            if index >= count {
                return None;
            }
            TableSelectionAction::Row {
                index,
                operation: RowSelection::Toggle,
            }
        }
        _ => return None,
    })
}

pub(crate) struct VisibleRows<'a> {
    pub node: &'a Node,
    pub viewport: Rect,
    pub cells: &'a ViewCells,
    pub offset: f32,
    pub direction: LayoutDirection,
    pub grid: bool,
    pub table: Option<&'a TableGeometry>,
    pub space: &'a Option<Rc<TransformSpace>>,
}

impl VisibleRows<'_> {
    pub(crate) fn arrange(self, list: xrack_runtime::ListMetrics) -> Vec<Placement> {
        let Self {
            node,
            viewport,
            cells,
            offset,
            direction,
            grid,
            table,
            space,
        } = self;
        let panes = table.and_then(|table| {
            if table.row_header.width == 0.0 {
                table.frozen_boundary()?;
            }
            let pane = |bounds| {
                Rc::new(PaneClip {
                    bounds,
                    space: space.clone(),
                })
            };
            Some((pane(table.frozen_body()), pane(table.scrollable_body())))
        });
        node.children
            .iter()
            .enumerate()
            .map(|(slot, row)| {
                if list.row_count == 0 && row.kind == "EmptyState" {
                    return Placement::plain(viewport);
                }
                let bounds = cells.bounds(list.first_row + slot, viewport, offset);
                let bounds = if grid {
                    place_in_direction(bounds, viewport, direction)
                } else {
                    bounds
                };
                let delegate = if row.kind == "ListRow" {
                    row.children.first()
                } else {
                    Some(row)
                };
                Placement {
                    bounds,
                    clip: None,
                    row_children: table.zip(delegate).map(|(table, row)| {
                        RowPlacements::Cells(
                            row.materialized_children()
                                .filter_map(|(index, _)| {
                                    let column = table.columns.get(index)?;
                                    Some(Placement {
                                        bounds: Rect::new(
                                            column.x,
                                            bounds.y,
                                            column.width,
                                            bounds.height,
                                        ),
                                        clip: panes.as_ref().map(|(frozen, scrolling)| {
                                            if table.is_frozen(index) {
                                                frozen.clone()
                                            } else {
                                                scrolling.clone()
                                            }
                                        }),
                                        row_children: None,
                                    })
                                })
                                .collect(),
                        )
                    }),
                }
            })
            .collect()
    }
}

fn navigate_tree(
    ItemRef { node, layout }: ItemRef<'_, '_>,
    key: NavigationKey,
) -> Option<ControlAction> {
    if !matches!(key, NavigationKey::Left | NavigationKey::Right) {
        return None;
    }
    let forward =
        (key == NavigationKey::Right) != (layout.layout_direction == LayoutDirection::RightToLeft);
    let tree = node.tree.as_ref()?;
    let index = tree.visible_index(node.int("current-key")?)?;
    let item = tree.rows().get(index)?;
    if !tree.is_filtering() && item.has_children && forward != item.expanded {
        return toggle_tree(node);
    }
    let next = if !forward {
        tree.visible_index(item.parent?)?
    } else {
        index.checked_add(1).filter(|&next| {
            tree.rows()
                .get(next)
                .is_some_and(|next| next.parent == Some(item.key))
        })?
    };
    Some(ControlAction::Select(next))
}

fn toggle_tree(node: &Node) -> Option<ControlAction> {
    let tree = node.tree.as_ref()?;
    if tree.is_filtering() {
        return None;
    }
    let index = tree.visible_index(node.int("current-key")?)?;
    let row = tree.rows().get(index)?;
    row.has_children
        .then_some(ControlAction::Tree(TreeAction::SetExpanded {
            key: row.key,
            row: row.row,
            expanded: !row.expanded,
        }))
}

fn select_tree_rows(node: &Node, key: TableSelectionKey) -> Option<ControlAction> {
    let selection = node.tree_selection.as_ref()?;
    let tree = node.tree.as_ref()?;
    Some(match key {
        TableSelectionKey::All if !tree.rows().is_empty() => {
            ControlAction::Tree(TreeAction::Selection(TreeSelectionAction::All))
        }
        TableSelectionKey::Clear if !selection.is_empty() => {
            ControlAction::Tree(TreeAction::Selection(TreeSelectionAction::Clear))
        }
        TableSelectionKey::Toggle => {
            let index = tree.visible_index(node.int("current-key")?)?;
            crate::tree::tree_row_action(node, index, RowSelection::Toggle)?
        }
        _ => return None,
    })
}

fn navigate_table_column(
    ItemRef { node, layout }: ItemRef<'_, '_>,
    key: NavigationKey,
    focused_column: Option<usize>,
) -> Option<ItemAction> {
    let view = node.table_selection.as_ref()?;
    if view.target() == SelectionTarget::Rows {
        return None;
    }
    let table = layout.table.as_ref()?;
    let current = view
        .current_column()
        .or(focused_column)
        .filter(|&column| column < table.columns.len());
    let current_visual = current.and_then(|column| table.visual_index(column));
    let visible = |&column: &usize| table.is_column_visible(column);
    let visible_current = current.filter(visible);
    let (previous, following) = crate::input::navigation_axis(false, layout.layout_direction);
    let next = match key {
        key if key == previous => table
            .visual_columns()
            .take(current_visual.unwrap_or(table.columns.len()))
            .rev()
            .find(visible)
            .or(visible_current)
            .or_else(|| table.visible_columns().next()),
        key if key == following => table
            .visual_columns()
            .skip(current_visual.map_or(0, |column| column + 1))
            .find(visible)
            .or(visible_current)
            .or_else(|| table.visible_columns().next_back()),
        NavigationKey::Home if view.target() == SelectionTarget::Columns => {
            table.visible_columns().next()
        }
        NavigationKey::End if view.target() == SelectionTarget::Columns => {
            table.visible_columns().next_back()
        }
        _ => None,
    };
    Some(ItemAction::TableColumnNavigation {
        next,
        current_visible: visible_current,
    })
}
