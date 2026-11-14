use crate::*;
use std::{cell::OnceCell, ops::Range, rc::Weak};
use xrack_runtime::{
    ComponentRepeater, Length, ListMetrics, ListModel, ListRowLocator, ListRowRenderer, Property,
    RecordValue, Repeater, RowContext, RowHeightProjection, RowKey, RowLifetime, TableColumnWindow,
    TableRowOrder, TableRowPositions, TableRows, VecModel,
};

#[derive(Clone)]
pub(crate) enum Model {
    Count(i64),
    Strings(VecModel<String>),
    Records(xrack_runtime::RecordList),
    Touches(xrack_runtime::TouchPoints),
}

impl ListModel for Model {
    type Item = Value;

    fn row_count(&self) -> usize {
        match self {
            Self::Count(count) => count.row_count(),
            Self::Strings(rows) => rows.row_count(),
            Self::Records(records) => records.model().row_count(),
            Self::Touches(points) => points.row_count(),
        }
    }

    fn item(&self, row: usize) -> Value {
        match self {
            Self::Count(count) => Value::Int(count.item(row)),
            Self::Strings(rows) => rows.item(row).into(),
            Self::Records(records) => Value::Record(records.model().item(row)),
            Self::Touches(points) => Value::TouchPoint(points.item(row)),
        }
    }

    fn track_item(&self, row: usize) {
        match self {
            Self::Count(count) => count.track_item(row),
            Self::Strings(rows) => rows.track_item(row),
            Self::Records(records) => records.model().track_item(row),
            Self::Touches(points) => points.track_item(row),
        }
    }

    fn row_key(&self, row: usize) -> RowKey {
        match self {
            Self::Count(count) => count.row_key(row),
            Self::Strings(rows) => ListModel::row_key(rows, row),
            Self::Records(records) => ListModel::row_key(records.model(), row),
            Self::Touches(points) => points.row_key(row),
        }
    }

    fn visit_row_keys(&self, range: Range<usize>, visit: impl FnMut(RowKey)) {
        match self {
            Self::Count(count) => count.visit_row_keys(range, visit),
            Self::Strings(rows) => ListModel::visit_row_keys(rows, range, visit),
            Self::Records(records) => ListModel::visit_row_keys(records.model(), range, visit),
            Self::Touches(points) => points.visit_row_keys(range, visit),
        }
    }

    fn index_of_key(&self, key: RowKey) -> Option<usize> {
        match self {
            Self::Count(count) => count.index_of_key(key),
            Self::Strings(rows) => ListModel::index_of_key(rows, key),
            Self::Records(records) => ListModel::index_of_key(records.model(), key),
            Self::Touches(points) => points.index_of_key(key),
        }
    }

    fn row_lifetime(&self, key: RowKey) -> RowLifetime {
        match self {
            Self::Count(count) => count.row_lifetime(key),
            Self::Strings(rows) => ListModel::row_lifetime(rows, key),
            Self::Records(records) => ListModel::row_lifetime(records.model(), key),
            Self::Touches(points) => points.row_lifetime(key),
        }
    }

    fn structure_stamp(&self) -> (u64, u64) {
        match self {
            Self::Count(count) => count.structure_stamp(),
            Self::Strings(rows) => ListModel::structure_stamp(rows),
            Self::Records(records) => ListModel::structure_stamp(records.model()),
            Self::Touches(points) => points.structure_stamp(),
        }
    }

    fn contents_stamp(&self) -> (u64, u64) {
        match self {
            Self::Count(count) => count.contents_stamp(),
            Self::Strings(rows) => rows.contents_stamp(),
            Self::Records(records) => records.model().contents_stamp(),
            Self::Touches(points) => points.contents_stamp(),
        }
    }
}

pub(crate) struct RowScope {
    pub owner: Weak<evaluate::State>,
    pub root_key: String,
    pub item: String,
    pub index: Option<String>,
    pub context: RowContext<Value>,
    pub sampled: bool,
}

#[derive(Default)]
pub(crate) struct RepeatState {
    model: Property<Rc<Model>>,
    rows: ComponentRepeater<Model, (), Instance>,
    display_rows: Repeater<Model, OnceCell<Instance>>,
    scene_columns: Property<TableColumnWindow>,
    renderer: OnceCell<ListRowRenderer>,
    pub(crate) row_order: TableRowOrder,
    table: OnceCell<table::State>,
    tree: OnceCell<xrack_runtime::TreeState>,
    row_heights: RowHeightProjection,
}

impl RepeatState {
    pub(crate) fn table_state(&self) -> &table::State {
        self.table.get_or_init(Default::default)
    }

    pub(crate) fn tree_state(&self) -> &xrack_runtime::TreeState {
        self.tree.get_or_init(Default::default)
    }

    fn model(&self, owner: &Instance, source: &Expr) -> Result<Rc<Model>> {
        let model = self.model.try_get(|| {
            let model = match owner.evaluate(source, &[])? {
                Value::Int(count) => Ok(Model::Count(count)),
                Value::StringModel(model) => Ok(Model::Strings(model)),
                Value::StringList(items) => Ok(Model::Strings(VecModel::from(items))),
                Value::RecordList(records) => Ok(Model::Records(records)),
                Value::TouchPoints(points) => Ok(Model::Touches(points)),
                _ => Err(Error::Evaluation("invalid repeated model".into())),
            }?;
            Ok(Rc::new(model))
        })?;
        if let Model::Records(records) = model.as_ref() {
            owner.program().validate_live_record_list(records)?;
        }
        Ok(model)
    }

    pub(crate) fn row_model(&self, owner: &Instance, repeat: &Repeat) -> Result<Rc<Model>> {
        self.model(owner, &repeat.model)
    }

    pub fn render(
        &self,
        owner: &Instance,
        template: &Element,
        range: Range<usize>,
    ) -> Result<Vec<Node>> {
        let repeat = template.repeat.as_ref().expect("repeated template");
        let model = self.row_model(owner, repeat)?;
        self.render_indices(
            owner,
            template,
            &model,
            range,
            &TableColumnWindow::default(),
        )
    }

    fn render_indices(
        &self,
        owner: &Instance,
        template: &Element,
        model: &Rc<Model>,
        indices: impl Iterator<Item = usize>,
        columns: &TableColumnWindow,
    ) -> Result<Vec<Node>> {
        let repeat = template.repeat.as_ref().expect("repeated template");
        let allocate = |context| {
            owner.allocate_row(
                template,
                RowScope {
                    owner: Rc::downgrade(&owner.0),
                    root_key: template.key.clone(),
                    item: repeat.item.clone(),
                    index: repeat.index.clone(),
                    context,
                    sampled: false,
                },
            )
        };
        if !repeat.uses_row_identity {
            let column_window = matches!(template.visual, Visual::Builtin("TableRow"))
                .then(|| self.display_rows.column_window(columns));
            return self.display_rows.try_render_resolved_indices(
                model,
                indices,
                |_, context, instance| {
                    if let Some(columns_changed) = column_window {
                        columns_changed.track();
                    }
                    if instance.get().is_none() {
                        let _ = instance.set(allocate(context.clone())?);
                    }
                    instance
                        .get()
                        .expect("initialized display row")
                        .render_template(template, columns)
                },
            );
        }
        let cache_reads_index = repeat
            .scene_cache
            .map(|plan| plan == RowSceneCache::Indexed);
        if cache_reads_index.is_some() {
            self.scene_columns.set_if_changed(columns.clone());
        }
        self.rows.try_render_resolved_indices(
            model.as_ref(),
            indices,
            cache_reads_index,
            allocate,
            |instance, _, _, _, _| {
                if cache_reads_index.is_some() {
                    let columns = self.scene_columns.get(Default::default);
                    instance.render_template(template, &columns)
                } else {
                    instance.render_template(template, columns)
                }
            },
        )
    }

    pub fn row(
        &self,
        owner: &Instance,
        repeat: &Repeat,
        index: usize,
        key: RowKey,
        follow_row: bool,
    ) -> Result<Option<Instance>> {
        let model = self.model(owner, &repeat.model)?;
        Ok(self.resolved_row(repeat, &model, index, key, follow_row))
    }

    pub(crate) fn resolved_row(
        &self,
        repeat: &Repeat,
        model: &Rc<Model>,
        index: usize,
        key: RowKey,
        follow_row: bool,
    ) -> Option<Instance> {
        if repeat.uses_row_identity {
            self.rows
                .with_resolved_event_row(model.as_ref(), index, key, follow_row, Clone::clone)
        } else {
            self.display_rows
                .with_resolved_row(model, index, key, |slot| slot.get().cloned())
                .flatten()
        }
    }

    fn renderer(&self, owner: &Instance, view: &Element) -> ListRowRenderer {
        self.renderer
            .get_or_init(|| {
                let owner = Rc::downgrade(&owner.0);
                let key = view.key.clone();
                ListRowRenderer::with_columns(move |range, columns| {
                    let Some(owner) = owner.upgrade().map(Instance::from_state) else {
                        return Vec::new();
                    };
                    let Some(view) = owner.definition().root.find_key(&key) else {
                        return Vec::new();
                    };
                    owner
                        .capture_result(owner.view_rows(view, range, columns))
                        .unwrap_or_default()
                })
            })
            .clone()
    }
}

#[derive(Clone)]
pub(crate) struct ViewModel {
    pub(crate) source: Option<Rc<Model>>,
    ordered: Option<TableRowPositions>,
    pub(crate) projected: Option<TableRows<VecModel<RecordValue>>>,
}

impl ViewModel {
    pub(crate) fn positions(&self) -> Option<&TableRowPositions> {
        self.projected
            .as_ref()
            .map(TableRows::positions)
            .or(self.ordered.as_ref())
    }

    pub(crate) fn table_rows(&self) -> Result<TableRows<Model>> {
        let source = self
            .source
            .as_ref()
            .ok_or_else(|| Error::Invalid("table has no source".into()))?;
        let positions = self
            .positions()
            .ok_or_else(|| Error::Invalid("table has no row order".into()))?;
        Ok(TableRows::from_positions(source.clone(), positions.clone()))
    }

    pub(crate) fn row_count(&self) -> usize {
        self.positions().map_or_else(
            || self.source.as_deref().map_or(0, ListModel::row_count),
            TableRowPositions::row_count,
        )
    }

    pub(crate) fn stamp(&self) -> Option<(u64, u64)> {
        self.positions().map_or_else(
            || self.source.as_deref().map(ListModel::structure_stamp),
            |positions| Some(positions.stamp()),
        )
    }

    pub(crate) fn source_index(&self, index: usize) -> Option<usize> {
        if let Some(positions) = self.positions() {
            positions.source_index(index)
        } else {
            (index < self.row_count()).then_some(index)
        }
    }

    pub(crate) fn visual_index(&self, source: usize) -> Option<usize> {
        if let Some(positions) = self.positions() {
            positions.visual_index(source)
        } else {
            (source < self.row_count()).then_some(source)
        }
    }

    fn locator(self) -> Option<ListRowLocator> {
        if let Some(model) = self.projected {
            Some(ListRowLocator::new(Rc::new(model)))
        } else if let Some(positions) = self.ordered {
            let source = self.source?;
            Some(ListRowLocator::new(Rc::new(TableRows::from_positions(
                source, positions,
            ))))
        } else {
            self.source.map(ListRowLocator::new)
        }
    }

    fn standard_rows(&self, label: &[String], range: Range<usize>) -> Result<Vec<Node>> {
        let record_text = |record: RecordValue| match record.field_path(label) {
            Some(Value::String(label)) => label.clone(),
            _ => String::new(),
        };
        Ok(match (self.source.as_deref(), &self.projected) {
            (_, Some(rows)) => xrack_runtime::standard_list_rows_in_view(rows, range, record_text),
            (Some(Model::Records(records)), None) => {
                xrack_runtime::standard_list_rows(records.model(), range, record_text)
            }
            (Some(Model::Strings(rows)), None) => {
                xrack_runtime::standard_list_rows(rows, range, |value| value)
            }
            (None, _) => Vec::new(),
            _ => return Err(Error::Invalid("expected a standard list model".into())),
        })
    }
}

impl Instance {
    pub(crate) fn host_element(
        &self,
        id: &str,
        matches_view: impl Fn(&Element) -> bool,
    ) -> Result<&Element> {
        self.definition()
            .root
            .find_host_element(id, &matches_view)
            .ok_or_else(|| Error::Invalid(format!("unknown view {id}")))
    }

    pub(crate) fn view_source_model(&self, view: &Element) -> Result<Option<Rc<Model>>> {
        view.view_source()
            .map(|(element, expression)| self.repeat_state(element).model(self, expression))
            .transpose()
    }

    pub(crate) fn view_model(&self, view: &Element) -> Result<ViewModel> {
        let source = self.view_source_model(view)?;
        let state = self.repeat_state(view);
        let projected = match (source.as_deref(), &view.visible_field) {
            (Some(Model::Records(records)), _) if view.table.is_some() => {
                Some(self.project_table(view, &state, records.model())?)
            }
            (Some(Model::Records(records)), Some(path)) => {
                let path = path.clone();
                let rows = self.repeat_state(view).row_order.visible_rows(
                    || Rc::new(records.model().clone()),
                    move |record| record.field_path(&path) == Some(&Value::Bool(true)),
                );
                Some(rows)
            }
            (_, Some(_)) => {
                return Err(Error::Invalid("expected a record visibility model".into()));
            }
            _ => None,
        };
        let ordered = if view.table.is_some() && projected.is_none() {
            source
                .as_deref()
                .map(|source| state.row_order.positions(source))
        } else {
            None
        };
        Ok(ViewModel {
            source,
            projected,
            ordered,
        })
    }

    pub(crate) fn element_owner(&self, target: &str) -> Result<(Self, &Element)> {
        let definition = self.definition();
        let view = definition
            .root
            .find_key(target)
            .ok_or_else(|| Error::Invalid(format!("unknown element {target}")))?;
        let mut owner = self.clone();
        while let Some(row) = owner.row_template() {
            if definition
                .root
                .find_key(row)
                .is_some_and(|template| template.scope_contains(target))
            {
                break;
            }
            owner = owner
                .row_owner()
                .ok_or_else(|| Error::Invalid("element owner was released".into()))?;
        }
        Ok((owner, view))
    }

    pub(crate) fn view_index(
        &self,
        target: &str,
        index: usize,
        direction: xrack_runtime::ViewIndex,
    ) -> Result<Option<usize>> {
        let (owner, view) = self.element_owner(target)?;
        if view.tree.is_some() {
            let (tree, _) = owner.tree_projection(view)?;
            return Ok(tree.view_index(index, direction));
        }
        let model = owner.view_model(view)?;
        Ok(match model.positions() {
            Some(positions) => positions.view_index(index, direction),
            None => (index < model.row_count()).then_some(index),
        })
    }

    pub(crate) fn source_key(&self, target: &str, row: usize) -> Result<Option<RowKey>> {
        let (owner, view) = self.element_owner(target)?;
        let Some((element, expression)) = view.view_source() else {
            return Ok(None);
        };
        let model = owner.repeat_state(element).model(&owner, expression)?;
        Ok(model.key_at(row))
    }

    pub(crate) fn render_view(&self, view: &Element, node: &mut Node) -> Result<()> {
        if view.tree.is_some() {
            return self.render_tree_view(view, node);
        }
        let model = self.view_model(view)?;
        node.list = Some(ListMetrics {
            row_count: model.row_count(),
            first_row: 0,
        });
        let property = if view.table.is_some() {
            "current-row"
        } else {
            "current-index"
        };
        node.properties.entry(property).or_insert(Value::Int(-1));
        let state = self.repeat_state(view);
        node.standard_list_stamp = model.stamp();
        if let Some(path) = &view.height_field
            && let Some(Model::Records(records)) = model.source.as_deref()
        {
            let path = path.clone();
            let field = move |record: &RecordValue| match record.field_path(&path) {
                Some(Value::Length(height)) => *height,
                _ => Length::default(),
            };
            let base = ListMetrics::row_height(node);
            node.row_heights = if let Some(positions) = model.positions() {
                state
                    .row_heights
                    .table_snapshot(records.model(), base, positions, field)
            } else {
                state.row_heights.snapshot(records.model(), base, field)
            };
        }
        if view.table.is_some() {
            self.render_table(view, &model, &state, node)?;
        }
        if self.definition().source_scroll_targets.contains(&view.key)
            || node.boolean("preserve-scroll-anchor", false)
            || node.boolean("movable-rows", false)
        {
            node.row_locator = model.locator();
        }
        node.list_row_renderer = Some(state.renderer(self, view));
        Ok(())
    }

    fn render_tree_view(&self, view: &Element, node: &mut Node) -> Result<()> {
        let (tree, model) = self.tree_projection(view)?;
        let state = self.repeat_state(view);
        if let Some(path) = &view.height_field
            && let Model::Records(records) = model.as_ref()
        {
            let path = path.clone();
            node.row_heights = state.row_heights.tree_snapshot(
                records.model(),
                ListMetrics::row_height(node),
                &tree,
                move |record| match record.field_path(&path) {
                    Some(Value::Length(height)) => *height,
                    _ => Length::default(),
                },
            );
        }
        state.tree_state().populate_scene(node, tree);
        node.list_row_renderer = Some(state.renderer(self, view));
        Ok(())
    }

    pub(crate) fn view_rows(
        &self,
        view: &Element,
        range: Range<usize>,
        columns: &TableColumnWindow,
    ) -> Result<Vec<Node>> {
        if view.tree.is_some() {
            let template = view
                .view_template()
                .ok_or_else(|| Error::Invalid("missing tree row template".into()))?;
            let (tree, model) = self.tree_projection(view)?;
            let rows = tree.rows_in_range(range);
            let state = self.repeat_state(template);
            return state.render_indices(
                self,
                template,
                &model,
                rows.iter().map(|row| row.model_index),
                columns,
            );
        }
        let model = self.view_model(view)?;
        if let Some(list) = &view.standard_list {
            return model.standard_rows(&list.label, range);
        }
        if view
            .table
            .as_ref()
            .is_some_and(|table| table.model.is_some())
        {
            return self.standard_table_rows(view, &model, range, columns);
        }
        let template = view
            .view_template()
            .ok_or_else(|| Error::Invalid("missing view row template".into()))?;
        let source = model
            .source
            .as_ref()
            .ok_or_else(|| Error::Invalid("missing view row model".into()))?;
        let indices = range.filter_map(|index| model.source_index(index));
        self.repeat_state(template)
            .render_indices(self, template, source, indices, columns)
    }
}

impl Element {
    pub(crate) fn is_list_view(&self) -> bool {
        matches!(
            self.visual,
            Visual::Builtin(
                "ListView"
                    | "GridView"
                    | "StandardListView"
                    | "TableView"
                    | "StandardTableView"
                    | "TreeView"
            )
        )
    }

    fn view_source(&self) -> Option<(&Element, &Expr)> {
        if let Some(list) = &self.standard_list {
            return list.model.as_ref().map(|model| (self, model));
        }
        if let Some(model) = self.table.as_ref().and_then(|table| table.model.as_ref()) {
            return Some((self, model));
        }
        let template = self.view_template()?;
        Some((template, &template.repeat.as_ref()?.model))
    }

    pub(crate) fn view_template(&self) -> Option<&Element> {
        self.children.iter().find(|child| child.repeat.is_some())
    }

    fn scope_contains(&self, key: &str) -> bool {
        self.key == key
            || self
                .owned_children()
                .filter(|child| child.repeat.is_none())
                .any(|child| child.scope_contains(key))
    }

    pub(crate) fn find_key(&self, key: &str) -> Option<&Self> {
        if self.key == key {
            return Some(self);
        }
        self.owned_children().find_map(|child| child.find_key(key))
    }

    fn find_host_element(&self, id: &str, matches_view: &impl Fn(&Self) -> bool) -> Option<&Self> {
        if xrack_mlir::host_element_id(&self.key, &self.id) == Some(id) && matches_view(self) {
            return Some(self);
        }
        self.owned_children()
            .find_map(|child| child.find_host_element(id, matches_view))
    }
}
