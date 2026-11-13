use crate::{Date, Node, Orientation, Property, TableRows, Time, Value, VecModel};
use std::{
    cell::Cell,
    collections::HashMap,
    fmt,
    ops::Range,
    rc::{Rc, Weak},
    sync::atomic::{AtomicU64, Ordering},
};

pub(crate) const MAX_ROWS: usize = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RowKey {
    pub(crate) model: u64,
    pub(crate) row: u64,
    pub(crate) scope: u64,
}

/// Direction of an index lookup between the source model and a displayed view.
#[derive(Clone, Copy, Debug)]
#[doc(hidden)]
pub enum ViewIndex {
    FromSource,
    ToSource,
}

impl ViewIndex {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "view-index-for-source" => Some(Self::FromSource),
            "source-index-for-view" => Some(Self::ToSource),
            _ => None,
        }
    }

    pub fn try_query<E>(
        self,
        index: i64,
        lookup: impl FnOnce(usize, Self) -> Result<Option<usize>, E>,
    ) -> Result<i64, E> {
        let Ok(index) = usize::try_from(index) else {
            return Ok(-1);
        };
        Ok(lookup(index, self)?
            .and_then(|index| i64::try_from(index).ok())
            .unwrap_or(-1))
    }

    pub fn query(self, index: i64, lookup: impl FnOnce(usize, Self) -> Option<usize>) -> i64 {
        self.try_query(index, |index, direction| {
            Ok::<_, std::convert::Infallible>(lookup(index, direction))
        })
        .unwrap_or_else(|error| match error {})
    }
}

impl RowKey {
    pub(crate) fn in_view(self, scope: u64) -> Self {
        Self { scope, ..self }
    }

    #[doc(hidden)]
    pub fn has_view_scope(self) -> bool {
        self.scope != 0
    }
}

type RenderRows = dyn Fn(Range<usize>, &TableColumnWindow) -> Vec<Node>;

struct RowRenderer {
    scope: u64,
    rows: Box<RenderRows>,
    offsets: [Property<Option<f32>>; 2],
    selection: Cell<Option<(Option<usize>, usize)>>,
    tree_root: Cell<Option<(i64, Option<RowKey>)>>,
}

/// Materializes a single view in its component instance, including views in delegates.
#[doc(hidden)]
#[derive(Clone)]
pub struct ListRowRenderer(Rc<RowRenderer>);

impl Node {
    /// Render rows through the virtual view at this path in the current base scene.
    #[doc(hidden)]
    pub fn list_rows_at(&self, path: &[usize], rows: Range<usize>) -> Vec<Node> {
        let Some(node) = self.at_path(path) else {
            return Vec::new();
        };
        let end = node
            .list
            .map_or(rows.end, |list| rows.end.min(list.row_count));
        node.list_row_renderer
            .as_ref()
            .map_or_else(Vec::new, |renderer| renderer.render(rows.start..end))
    }

    /// Recover the visual index, source key and delegate from a prepared row.
    #[doc(hidden)]
    pub fn list_event_row(&self, slot: usize) -> Option<(usize, RowKey, &Node)> {
        let index = self.list?.first_row.checked_add(slot)?;
        let row = self.child_at(slot)?;
        Some((index, row.row_key?, row.child_at(0)?))
    }
}

impl ListRowRenderer {
    pub fn new(rows: impl Fn(Range<usize>) -> Vec<Node> + 'static) -> Self {
        Self::with_columns(move |range, _| rows(range))
    }

    pub fn with_columns(
        rows: impl Fn(Range<usize>, &TableColumnWindow) -> Vec<Node> + 'static,
    ) -> Self {
        static NEXT_VIEW: AtomicU64 = AtomicU64::new(1);
        let scope = NEXT_VIEW
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("virtual view identities exhausted");
        Self(Rc::new(RowRenderer {
            scope,
            rows: Box::new(rows),
            offsets: Default::default(),
            selection: Cell::default(),
            tree_root: Cell::default(),
        }))
    }

    pub fn render(&self, rows: Range<usize>) -> Vec<Node> {
        self.render_in_columns(rows, &TableColumnWindow::default())
    }

    pub fn render_in_columns(&self, rows: Range<usize>, columns: &TableColumnWindow) -> Vec<Node> {
        (self.0.rows)(rows, columns)
    }

    pub fn scope(&self) -> u64 {
        self.0.scope
    }

    pub fn offset(&self, axis: Orientation) -> Option<f32> {
        self.0.offsets[usize::from(axis == Orientation::Vertical)].get(|| None)
    }

    pub fn set_offset(&self, axis: Orientation, offset: f32) {
        self.0.offsets[usize::from(axis == Orientation::Vertical)].set_if_changed(Some(offset));
    }

    pub fn selection_changed(&self, selected: Option<usize>, columns: usize) -> bool {
        self.0
            .selection
            .get()
            .is_some_and(|previous| previous != (selected, columns))
    }

    pub fn remember_selection(&self, selected: Option<usize>, columns: usize) {
        self.0.selection.set(Some((selected, columns)));
    }

    pub fn tree_root_changed(&self, root: (i64, Option<RowKey>)) -> bool {
        self.0
            .tree_root
            .get()
            .is_some_and(|previous| previous != root)
    }

    pub fn remember_tree_root(&self, root: (i64, Option<RowKey>)) {
        self.0.tree_root.set(Some(root));
    }
}

/// Visible columns in declaration order. The default requests every column.
#[doc(hidden)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TableColumnWindow(Option<Rc<VisibleColumns>>);

#[derive(Debug, PartialEq, Eq)]
struct VisibleColumns {
    indices: Box<[usize]>,
    count: usize,
}

impl TableColumnWindow {
    pub fn new(visible: impl IntoIterator<Item = bool>) -> Self {
        let mut indices = Vec::new();
        let mut count = 0;
        for (index, visible) in visible.into_iter().enumerate() {
            if visible {
                indices.push(index);
            }
            count = index + 1;
        }
        Self((indices.len() != count).then(|| {
            Rc::new(VisibleColumns {
                indices: indices.into_boxed_slice(),
                count,
            })
        }))
    }

    pub fn includes(&self, column: usize) -> bool {
        self.dense_index(column).is_some()
    }

    pub fn dense_index(&self, column: usize) -> Option<usize> {
        self.0.as_ref().map_or(Some(column), |visible| {
            visible.indices.binary_search(&column).ok()
        })
    }

    pub fn logical_index(&self, index: usize) -> usize {
        self.0
            .as_ref()
            .map_or(index, |visible| visible.indices[index])
    }

    pub fn logical_len(&self, materialized: usize) -> usize {
        self.0
            .as_ref()
            .map_or(materialized, |visible| visible.count)
    }

    pub fn columns(&self, count: usize) -> impl Iterator<Item = usize> + '_ {
        self.0
            .iter()
            .flat_map(move |visible| {
                visible
                    .indices
                    .iter()
                    .copied()
                    .take_while(move |&index| index < count)
            })
            .chain(0..if self.0.is_none() { count } else { 0 })
    }
}

impl PartialEq for ListRowRenderer {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for ListRowRenderer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ListRowRenderer")
            .finish_non_exhaustive()
    }
}

pub struct RowLifetime(pub(crate) Option<Weak<()>>);

impl RowLifetime {
    pub(crate) fn is_live(&self, key: RowKey, model: u64, count: usize) -> bool {
        key.model == model
            && match &self.0 {
                Some(lifetime) => lifetime.strong_count() != 0,
                None => key.row < count as u64,
            }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ListMetrics {
    pub row_count: usize,
    pub first_row: usize,
}

impl ListMetrics {
    pub fn row_height(node: &Node) -> f32 {
        Self::clamp_row_height(node.length("row-height").unwrap_or(32.0))
    }

    pub fn clamp_row_height(height: f32) -> f32 {
        height.clamp(12.0, 512.0)
    }

    pub fn row_height_at(node: &Node, index: usize) -> f32 {
        node.row_heights
            .as_ref()
            .map_or_else(|| Self::row_height(node), |heights| heights.height(index))
    }

    pub fn selection(node: &Node) -> Option<i64> {
        if let Some(tree) = &node.tree {
            return tree
                .visible_index(node.int("current-key")?)
                .map(|index| index as i64);
        }
        node.int(if node.kind == "TableView" {
            "current-row"
        } else {
            "current-index"
        })
    }

    /// Check that a selection position still belongs to the rendered model order.
    #[doc(hidden)]
    pub fn has_row_in_model(node: &Node, index: usize, live_stamp: Option<(u64, u64)>) -> bool {
        node.standard_list_stamp == live_stamp
            && node.list.is_some_and(|list| index < list.row_count)
    }
}

/// Resolves stable model row identities without constructing their delegates.
#[derive(Clone)]
#[doc(hidden)]
pub struct ListRowLocator(Rc<dyn RowPositions>);

trait RowPositions {
    fn first_position(&self, candidates: &[(RowKey, usize)]) -> Option<(usize, usize)>;
    fn key_at(&self, index: usize) -> Option<RowKey>;
    fn position_of_source_key(&self, key: RowKey, source_index: usize) -> Option<usize>;
}

impl<M: ListModel> RowPositions for M {
    fn position_of_source_key(&self, key: RowKey, source_index: usize) -> Option<usize> {
        self.index_of_source_key(key, source_index)
    }

    fn key_at(&self, index: usize) -> Option<RowKey> {
        ListModel::key_at(self, index)
    }

    fn first_position(&self, candidates: &[(RowKey, usize)]) -> Option<(usize, usize)> {
        let &(first, hint) = candidates.first()?;
        let count = self.row_count();
        if count == 0 || self.row_key(0).model != first.model {
            return None;
        }
        if hint < count && self.row_key(hint) == first {
            return Some((0, hint));
        }
        // Nearby insertions and removals are common, including in deep lists.
        let nearby = hint.saturating_sub(4)..count.min(hint.saturating_add(5));
        if let Some(index) = nearby
            .into_iter()
            .find(|&index| self.row_key(index) == first)
        {
            return Some((0, index));
        }

        // Resolve all fallback rows in one pass when a whole page was removed.
        let priorities: HashMap<_, _> = candidates
            .iter()
            .enumerate()
            .map(|(priority, &(key, _))| (key, priority))
            .collect();
        let mut best = None;
        let mut index = 0;
        self.visit_row_keys(0..count, |key| {
            if let Some(&priority) = priorities.get(&key)
                && best.is_none_or(|(previous, _)| priority < previous)
            {
                best = Some((priority, index));
            }
            index += 1;
        });
        best
    }
}

impl ListRowLocator {
    pub fn new(model: Rc<impl ListModel + 'static>) -> Self {
        Self(model)
    }

    pub fn first_position(&self, candidates: &[(RowKey, usize)]) -> Option<(usize, usize)> {
        self.0.first_position(candidates)
    }

    pub fn key_at(&self, index: usize) -> Option<RowKey> {
        self.0.key_at(index)
    }

    pub(crate) fn position_of_source_key(&self, key: RowKey, source_index: usize) -> Option<usize> {
        self.0.position_of_source_key(key, source_index)
    }
}

impl PartialEq for ListRowLocator {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for ListRowLocator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ListRowLocator")
            .finish_non_exhaustive()
    }
}

#[doc(hidden)]
pub fn standard_list_rows<T: Clone + PartialEq>(
    model: &VecModel<T>,
    range: Range<usize>,
    label: impl FnMut(T) -> String,
) -> Vec<Node> {
    standard_list_rows_at(model, range, label)
}

#[doc(hidden)]
pub fn standard_list_rows_in_view<T: Clone + PartialEq>(
    rows: &TableRows<VecModel<T>>,
    range: Range<usize>,
    label: impl FnMut(T) -> String,
) -> Vec<Node> {
    standard_list_rows_at(
        rows.source(),
        range.filter_map(|row| rows.positions().source_index(row)),
        label,
    )
}

fn standard_list_rows_at<T: Clone + PartialEq>(
    model: &VecModel<T>,
    indices: impl Iterator<Item = usize>,
    mut label: impl FnMut(T) -> String,
) -> Vec<Node> {
    indices
        .filter_map(|index| {
            let value = model.row(index)?;
            let key = model.row_key(index)?;
            let mut item = Node::new("StandardListItem", None);
            item.set_row_key(Some(key));
            item.properties.insert("text", label(value).into());
            Some(item)
        })
        .collect()
}

#[doc(hidden)]
pub fn standard_table_rows<T, const COLUMNS: usize>(
    model: &VecModel<T>,
    range: Range<usize>,
    project: impl Fn(&T) -> [String; COLUMNS],
) -> Vec<Node> {
    standard_table_rows_with_header(model, range, |item| (None, project(item)))
}

#[doc(hidden)]
pub fn standard_table_rows_with_header<T, const COLUMNS: usize>(
    model: &VecModel<T>,
    range: Range<usize>,
    project: impl Fn(&T) -> (Option<String>, [String; COLUMNS]),
) -> Vec<Node> {
    standard_table_rows_at(model, range, [None; COLUMNS], project)
}

#[doc(hidden)]
pub fn standard_table_rows_in_order<T, const COLUMNS: usize>(
    rows: &TableRows<VecModel<T>>,
    range: Range<usize>,
    project: impl Fn(&T) -> (Option<String>, [String; COLUMNS]),
) -> Vec<Node> {
    standard_table_rows_with_editors_in_order(rows, range, [None; COLUMNS], project)
}

#[doc(hidden)]
pub fn standard_table_rows_with_editors_in_order<T, const COLUMNS: usize>(
    rows: &TableRows<VecModel<T>>,
    range: Range<usize>,
    editors: [Option<TableCellEditor>; COLUMNS],
    project: impl Fn(&T) -> (Option<String>, [String; COLUMNS]),
) -> Vec<Node> {
    standard_table_rows_at(
        rows.source(),
        range.filter_map(|index| rows.positions().source_index(index)),
        editors,
        project,
    )
}

/// Build only the visible cells and retain their declaration-order addresses.
#[doc(hidden)]
pub fn project_standard_table_rows<T>(
    rows: &TableRows<VecModel<T>>,
    range: Range<usize>,
    columns: &TableColumnWindow,
    column_count: usize,
    header: impl Fn(&T) -> Option<String>,
    cell: impl Fn(&T, usize) -> String,
    editor: impl Fn(usize) -> Option<TableCellEditor>,
) -> Vec<Node> {
    try_project_standard_table_rows(rows, range, columns, column_count, header, cell, |column| {
        Ok::<_, std::convert::Infallible>(editor(column))
    })
    .unwrap_or_else(|error| match error {})
}

#[doc(hidden)]
pub fn try_project_standard_table_rows<T, E>(
    rows: &TableRows<VecModel<T>>,
    range: Range<usize>,
    columns: &TableColumnWindow,
    column_count: usize,
    header: impl Fn(&T) -> Option<String>,
    cell: impl Fn(&T, usize) -> String,
    editor: impl Fn(usize) -> Result<Option<TableCellEditor>, E>,
) -> Result<Vec<Node>, E> {
    let model = rows.source();
    let visible: Vec<_> = columns
        .columns(column_count)
        .map(|column| editor(column).map(|editor| (column, editor)))
        .collect::<Result<_, E>>()?;
    Ok(range
        .filter_map(|index| {
            let index = rows.positions().source_index(index)?;
            let key = model.row_key(index)?;
            model.project_row(index, |item| {
                let mut row = projected_table_row(key, header(item));
                row.table_column_window = columns.clone();
                row.children.extend(
                    visible
                        .iter()
                        .map(|&(column, editor)| projected_table_cell(cell(item, column), editor)),
                );
                row
            })
        })
        .collect())
}

/// The model field type used to validate a projected cell's text draft.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableCellEditor {
    String,
    Int,
    Float,
    Bool,
    Date,
    Time,
}

impl TableCellEditor {
    pub fn parse(self, text: &str) -> Option<Value> {
        let trimmed = text.trim();
        match self {
            Self::String => Some(Value::String(text.into())),
            Self::Int => trimmed.parse().ok().map(Value::Int),
            Self::Float => trimmed
                .parse::<f64>()
                .ok()
                .filter(|number| number.is_finite())
                .map(Value::Float),
            Self::Bool => crate::table_paste_bool(text).map(Value::Bool),
            Self::Date => {
                let [year, month, day] = parse_cell_parts(trimmed, '-')?;
                Date::new(year, month, day).map(Value::Date)
            }
            Self::Time => {
                let [hour, minute, second] = parse_cell_parts(trimmed, ':')?;
                Time::new(hour, minute, second).map(Value::Time)
            }
        }
    }

    pub fn accepts(self, text: &str) -> bool {
        self == Self::String || self.parse(text).is_some()
    }
}

fn parse_cell_parts(text: &str, separator: char) -> Option<[i64; 3]> {
    let mut parts = text.split(separator);
    let values = [
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ];
    parts.next().is_none().then_some(values)
}

fn standard_table_rows_at<T, const COLUMNS: usize>(
    model: &VecModel<T>,
    indices: impl Iterator<Item = usize>,
    editors: [Option<TableCellEditor>; COLUMNS],
    project: impl Fn(&T) -> (Option<String>, [String; COLUMNS]),
) -> Vec<Node> {
    indices
        .filter_map(|index| {
            let key = model.row_key(index)?;
            let (header, cells) = model.project_row(index, &project)?;
            let mut row = projected_table_row(key, header);
            row.children.extend(
                cells
                    .into_iter()
                    .zip(editors)
                    .map(|(text, editor)| projected_table_cell(text, editor)),
            );
            Some(row)
        })
        .collect()
}

fn projected_table_row(key: RowKey, header: Option<String>) -> Node {
    let mut row = Node::new("TableRow", None);
    row.set_row_key(Some(key));
    if let Some(header) = header {
        row.properties.insert("row-header-text", header.into());
    }
    row
}

fn projected_table_cell(text: String, editor: Option<TableCellEditor>) -> Node {
    let mut cell = Node::new("TableCell", None);
    cell.properties.insert("text", text.into());
    if editor.is_some() {
        cell.properties.insert("editable", true.into());
        cell.table_cell_editor = editor;
    }
    cell
}

pub trait ListModel {
    type Item: Clone + PartialEq;

    fn row_count(&self) -> usize;
    fn item(&self, row: usize) -> Self::Item;
    fn row_key(&self, row: usize) -> RowKey;
    #[doc(hidden)]
    fn key_at(&self, index: usize) -> Option<RowKey> {
        (index < self.row_count()).then(|| self.row_key(index))
    }
    fn visit_items(&self, mut visit: impl FnMut(usize, RowKey, &Self::Item)) {
        for index in 0..self.row_count() {
            visit(index, self.row_key(index), &self.item(index));
        }
    }
    fn visit_row_keys(&self, range: Range<usize>, mut visit: impl FnMut(RowKey)) {
        for index in range {
            visit(self.row_key(index));
        }
    }
    fn index_of_key(&self, key: RowKey) -> Option<usize> {
        (0..self.row_count()).find(|&index| self.row_key(index) == key)
    }
    #[doc(hidden)]
    fn index_of_source_key(&self, key: RowKey, source_index: usize) -> Option<usize> {
        if source_index < self.row_count() && self.row_key(source_index) == key {
            Some(source_index)
        } else {
            self.index_of_key(key)
        }
    }
    fn row_lifetime(&self, key: RowKey) -> RowLifetime;
    fn structure_stamp(&self) -> (u64, u64);
    /// Identify the model revision, including changes to its row values.
    #[doc(hidden)]
    fn contents_stamp(&self) -> (u64, u64) {
        self.structure_stamp()
    }
    #[doc(hidden)]
    fn source_structure_stamp(&self) -> (u64, u64) {
        self.structure_stamp()
    }
    #[doc(hidden)]
    fn visit_source_row_keys(&self, visit: impl FnMut(RowKey)) {
        self.visit_row_keys(0..self.row_count(), visit);
    }
}

impl ListModel for i64 {
    type Item = i64;

    fn row_count(&self) -> usize {
        (*self).clamp(0, MAX_ROWS as i64) as usize
    }

    fn item(&self, row: usize) -> Self::Item {
        row as i64
    }

    fn row_key(&self, row: usize) -> RowKey {
        RowKey {
            model: 0,
            row: row as u64,
            scope: 0,
        }
    }
    fn row_lifetime(&self, _: RowKey) -> RowLifetime {
        RowLifetime(None)
    }
    fn index_of_key(&self, key: RowKey) -> Option<usize> {
        let index = usize::try_from(key.row).ok()?;
        (key.model == 0 && index < self.row_count()).then_some(index)
    }
    fn structure_stamp(&self) -> (u64, u64) {
        (0, self.row_count() as u64)
    }
}

impl<T: Clone + PartialEq> ListModel for VecModel<T> {
    type Item = T;

    fn row_count(&self) -> usize {
        self.len().min(MAX_ROWS)
    }
    fn visit_items(&self, visit: impl FnMut(usize, RowKey, &Self::Item)) {
        self.visit_row_data(self.row_count(), visit);
    }

    fn item(&self, row: usize) -> Self::Item {
        self.row(row).expect("row is inside the model range")
    }

    fn row_key(&self, row: usize) -> RowKey {
        self.row_key(row).expect("row is inside the model range")
    }

    fn row_lifetime(&self, key: RowKey) -> RowLifetime {
        self.row_lifetime(key)
    }
    fn index_of_key(&self, key: RowKey) -> Option<usize> {
        self.row_index(key).filter(|&index| index < MAX_ROWS)
    }
    fn visit_row_keys(&self, range: Range<usize>, visit: impl FnMut(RowKey)) {
        self.visit_row_keys(range, visit);
    }
    fn structure_stamp(&self) -> (u64, u64) {
        self.structure_stamp()
    }
    fn contents_stamp(&self) -> (u64, u64) {
        self.contents_stamp()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_table_projects_values_and_editor_flags_only_for_requested_columns() {
        use std::cell::RefCell;

        let model = Rc::new(VecModel::from(vec!["A".to_owned(), "B".to_owned()]));
        let rows = crate::TableRowOrder::default().rows(model.clone());
        let values = RefCell::new(Vec::new());
        let flags = RefCell::new(Vec::new());
        let view = project_standard_table_rows(
            &rows,
            0..2,
            &TableColumnWindow::new(vec![true, false, true, false]),
            4,
            |item| Some(item.clone()),
            |item, column| {
                values.borrow_mut().push((item.clone(), column));
                format!("{item}:{column}")
            },
            |column| {
                flags.borrow_mut().push(column);
                Some(TableCellEditor::String)
            },
        );
        assert_eq!(*flags.borrow(), [0, 2]);
        assert_eq!(
            values
                .borrow()
                .iter()
                .map(|(_, column)| *column)
                .collect::<Vec<_>>(),
            [0, 2, 0, 2]
        );
        assert_eq!(view[1].row_key, model.row_key(1));
        assert_eq!(view[1].text("row-header-text"), Some("B"));
        assert_eq!(view[1].child_at(2).unwrap().text("text"), Some("B:2"));
        assert_eq!(
            view[1].child_at(2).unwrap().table_cell_editor,
            Some(TableCellEditor::String)
        );
        assert!(view[1].child_at(1).is_none());
        assert_eq!(view[1].children.len(), 2);
    }

    #[test]
    fn projected_cell_values_validate_scalar_ranges_and_preserve_strings() {
        use TableCellEditor::*;
        assert_eq!(
            String.parse("  device  "),
            Some(Value::String("  device  ".into()))
        );
        assert_eq!(Int.parse(" -42 "), Some(Value::Int(-42)));
        assert!(!Int.accepts("9223372036854775808"));
        assert_eq!(Float.parse("1.25e2"), Some(Value::Float(125.0)));
        for text in ["NaN", "inf", "1e500"] {
            assert!(!Float.accepts(text));
        }
        assert_eq!(Bool.parse("true"), Some(Value::Bool(true)));
        assert_eq!(Bool.parse("0"), Some(Value::Bool(false)));
        assert!(!Bool.accepts("2"));
        assert_eq!(
            Date.parse("2000-02-29"),
            crate::Date::new(2000, 2, 29).map(Value::Date)
        );
        assert!(!Date.accepts("1900-02-29"));
        assert!(!Date.accepts("2000-02-29-01"));
        assert_eq!(
            Time.parse("23:59:59"),
            crate::Time::new(23, 59, 59).map(Value::Time)
        );
        assert!(!Time.accepts("24:00:00"));
        assert!(!Time.accepts("12:34"));
    }
}
