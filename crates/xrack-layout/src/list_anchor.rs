use crate::{LayoutNode, view_cells::ViewCells};
use std::collections::BTreeMap;
use xrack_runtime::{ListMetrics, Node, Orientation, RowKey};

#[derive(Clone, Debug)]
pub(crate) struct ListAnchor {
    pub offset: f32,
    pub scope: Option<u64>,
    cells: ViewCells,
    selection: Option<i64>,
    rows: Vec<(RowKey, usize)>,
    tree_keys: Vec<i64>,
}

impl ListAnchor {
    fn capture(node: &Node, layout: &LayoutNode) -> Option<Self> {
        let list = node.list?;
        if !layout.visible
            || !node.boolean("preserve-scroll-anchor", false)
            || node.row_locator.is_none() && node.tree.is_none()
            || node
                .viewport_handle
                .as_ref()
                .is_some_and(|handle| handle.has_bound_offset(Orientation::Vertical))
        {
            return None;
        }
        let offset = layout.scroll?.offset;
        let width = layout
            .table
            .as_ref()
            .map_or(layout.bounds.width, |table| table.body.width);
        let rows: Vec<_> = node
            .children
            .iter()
            .enumerate()
            .filter_map(|(slot, row)| {
                if row.kind != "ListRow" {
                    return None;
                }
                Some((row.row_key?, list.first_row.checked_add(slot)?))
            })
            .collect();
        if rows.is_empty() {
            return None;
        }
        let tree_keys = if let Some(tree) = &node.tree {
            rows.iter()
                .map(|&(_, index)| tree.rows().get(index).map(|item| item.key))
                .collect::<Option<_>>()?
        } else {
            Vec::new()
        };
        Some(Self {
            offset,
            scope: layout
                .nested_list
                .as_ref()
                .map(xrack_runtime::ListRowRenderer::scope),
            cells: ViewCells::new(node, width),
            selection: ListMetrics::selection(node),
            rows,
            tree_keys,
        })
    }

    pub(crate) fn selection_changed(&self, node: &Node) -> bool {
        self.selection != ListMetrics::selection(node)
    }

    pub(crate) fn restore(&self, node: &Node, cells: &ViewCells, requested: f32) -> Option<f32> {
        if requested != self.offset {
            return None;
        }
        let (candidate, index) = if let Some(tree) = &node.tree {
            self.tree_keys
                .iter()
                .enumerate()
                .find_map(|(candidate, &key)| {
                    let index = tree.visible_index(key)?;
                    (tree.rows()[index].row == self.rows[candidate].0).then_some((candidate, index))
                })?
        } else {
            node.row_locator.as_ref()?.first_position(&self.rows)?
        };
        let (_, previous) = self.rows[candidate];
        let within = f64::from(self.offset) - self.cells.top(previous);
        // A shorter row still needs to intersect the viewport after anchoring.
        let height = cells.height(index);
        let within = if height < self.cells.height(previous) {
            within.min(f64::from(height) - 1.0)
        } else {
            within
        };
        Some((cells.top(index) + within) as f32)
    }
}

pub(crate) fn capture_list_anchors(
    scene: &Node,
    layout: &LayoutNode,
) -> BTreeMap<Vec<usize>, ListAnchor> {
    fn visit(
        scene: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        anchors: &mut BTreeMap<Vec<usize>, ListAnchor>,
    ) {
        if scene.list.is_some() {
            if let Some(renderer) = &layout.nested_list
                && layout.visible
            {
                let width = layout
                    .table
                    .as_ref()
                    .map_or(layout.bounds.width, |table| table.body.width);
                let cells = ViewCells::new(scene, width);
                renderer.remember_selection(crate::input::scroll_selection(scene), cells.columns);
                if let Some(tree) = &scene.tree {
                    renderer.remember_tree_root(tree.root_identity());
                }
                for axis in [Orientation::Horizontal, Orientation::Vertical] {
                    let bound = layout
                        .viewport_handle
                        .as_ref()
                        .is_some_and(|handle| handle.has_bound_offset(axis));
                    if !bound
                        && let Some(scroll) = layout.scroll_on(axis)
                        && (scroll.offset > 0.0 || renderer.offset(axis).is_some())
                    {
                        renderer.set_offset(axis, scroll.offset);
                    }
                }
            }
            if let Some(anchor) = ListAnchor::capture(scene, layout) {
                anchors.insert(path.clone(), anchor);
            }
        }
        for (index, child, item) in layout.children_in_scene_order(scene) {
            path.push(index);
            visit(child, item, path, anchors);
            path.pop();
        }
    }

    let mut anchors = BTreeMap::new();
    visit(scene, layout, &mut Vec::new(), &mut anchors);
    anchors
}
