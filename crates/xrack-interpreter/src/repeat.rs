use crate::*;
use std::{cell::OnceCell, ops::Range, rc::Weak};
use xrack_runtime::{
    ComponentRepeater, Length, ListMetrics, ListModel, ListRowLocator, ListRowRenderer,
    ListSectionProjection, Property, RecordValue, Repeater, RowContext, RowHeightProjection,
    RowKey, RowLifetime, TableColumnWindow, TableRowOrder, TableRowPositions, TableRows, VecModel,
};

#[derive(Clone)]
pub(crate) enum Model {
    Count(i64),
    Integers(VecModel<i64>),
    Dates(VecModel<xrack_runtime::Date>),
    Strings(VecModel<String>),
    Records(xrack_runtime::RecordList),
    Touches(xrack_runtime::TouchPoints),
}

impl Model {
    pub(crate) fn move_row(&self, from: usize, to: usize) -> bool {
        match self {
            Self::Integers(rows) => rows.move_row(from, to),
            Self::Dates(rows) => rows.move_row(from, to),
            Self::Strings(rows) => rows.move_row(from, to),
            Self::Records(rows) => rows.model().move_row(from, to),
            Self::Count(_) | Self::Touches(_) => false,
        }
    }
}

pub(crate) fn edit_model(
    model: Value,
    method: xrack_syntax::builtins::ModelEditKind,
    args: &[Value],
) -> Result<()> {
    use xrack_runtime::ModelEdit;
    use xrack_syntax::builtins::ModelEditKind::*;
    let invalid = || Error::Invalid("model edit argument type mismatch".into());
    let edit = match (method, args) {
        (Append, [value]) => ModelEdit::Append(value.clone()),
        (Insert, [Value::Int(index), value]) => ModelEdit::Insert(*index, value.clone()),
        (Set, [Value::Int(index), value]) => ModelEdit::Set(*index, value.clone()),
        (Remove, [Value::Int(index)]) => ModelEdit::Remove(*index),
        (Move, [Value::Int(from), Value::Int(to)]) => ModelEdit::Move(*from, *to),
        (Clear, []) => ModelEdit::Clear,
        _ => return Err(invalid()),
    };
    match model {
        Value::IntModel(rows) => edit
            .try_map(|value| match value {
                Value::Int(value) => Ok(value),
                _ => Err(invalid()),
            })?
            .apply(&rows),
        Value::DateModel(rows) => edit
            .try_map(|value| match value {
                Value::Date(value) => Ok(value),
                _ => Err(invalid()),
            })?
            .apply(&rows),
        Value::StringModel(rows) => edit
            .try_map(|value| match value {
                Value::String(value) => Ok(value),
                _ => Err(invalid()),
            })?
            .apply(&rows),
        Value::RecordList(rows) => edit
            .try_map(|value| match value {
                Value::Record(value) if value.name() == rows.name() => Ok(value),
                _ => Err(invalid()),
            })?
            .apply(rows.model()),
        _ => return Err(invalid()),
    }
    Ok(())
}

impl ListModel for Model {
    type Item = Value;

    fn row_count(&self) -> usize {
        match self {
            Self::Count(count) => count.row_count(),
            Self::Integers(rows) => rows.row_count(),
            Self::Dates(rows) => rows.row_count(),
            Self::Strings(rows) => rows.row_count(),
            Self::Records(records) => records.model().row_count(),
            Self::Touches(points) => points.row_count(),
        }
    }

    fn item(&self, row: usize) -> Value {
        match self {
            Self::Count(count) => Value::Int(count.item(row)),
            Self::Integers(rows) => rows.item(row).into(),
            Self::Dates(rows) => rows.item(row).into(),
            Self::Strings(rows) => rows.item(row).into(),
            Self::Records(records) => Value::Record(records.model().item(row)),
            Self::Touches(points) => Value::TouchPoint(points.item(row)),
        }
    }

    fn track_item(&self, row: usize) {
        match self {
            Self::Count(count) => count.track_item(row),
            Self::Integers(rows) => rows.track_item(row),
            Self::Dates(rows) => rows.track_item(row),
            Self::Strings(rows) => rows.track_item(row),
            Self::Records(records) => records.model().track_item(row),
            Self::Touches(points) => points.track_item(row),
        }
    }

    fn row_key(&self, row: usize) -> RowKey {
        match self {
            Self::Count(count) => count.row_key(row),
            Self::Integers(rows) => ListModel::row_key(rows, row),
            Self::Dates(rows) => ListModel::row_key(rows, row),
            Self::Strings(rows) => ListModel::row_key(rows, row),
            Self::Records(records) => ListModel::row_key(records.model(), row),
            Self::Touches(points) => points.row_key(row),
        }
    }

    fn visit_row_keys(&self, range: Range<usize>, visit: impl FnMut(RowKey)) {
        match self {
            Self::Count(count) => count.visit_row_keys(range, visit),
            Self::Integers(rows) => ListModel::visit_row_keys(rows, range, visit),
            Self::Dates(rows) => ListModel::visit_row_keys(rows, range, visit),
            Self::Strings(rows) => ListModel::visit_row_keys(rows, range, visit),
            Self::Records(records) => ListModel::visit_row_keys(records.model(), range, visit),
            Self::Touches(points) => points.visit_row_keys(range, visit),
        }
    }

    fn index_of_key(&self, key: RowKey) -> Option<usize> {
        match self {
            Self::Count(count) => count.index_of_key(key),
            Self::Integers(rows) => ListModel::index_of_key(rows, key),
            Self::Dates(rows) => ListModel::index_of_key(rows, key),
            Self::Strings(rows) => ListModel::index_of_key(rows, key),
            Self::Records(records) => ListModel::index_of_key(records.model(), key),
            Self::Touches(points) => points.index_of_key(key),
        }
    }

    fn row_lifetime(&self, key: RowKey) -> RowLifetime {
        match self {
            Self::Count(count) => count.row_lifetime(key),
            Self::Integers(rows) => ListModel::row_lifetime(rows, key),
            Self::Dates(rows) => ListModel::row_lifetime(rows, key),
            Self::Strings(rows) => ListModel::row_lifetime(rows, key),
            Self::Records(records) => ListModel::row_lifetime(records.model(), key),
            Self::Touches(points) => points.row_lifetime(key),
        }
    }

    fn structure_stamp(&self) -> (u64, u64) {
        match self {
            Self::Count(count) => count.structure_stamp(),
            Self::Integers(rows) => ListModel::structure_stamp(rows),
            Self::Dates(rows) => ListModel::structure_stamp(rows),
            Self::Strings(rows) => ListModel::structure_stamp(rows),
            Self::Records(records) => ListModel::structure_stamp(records.model()),
            Self::Touches(points) => points.structure_stamp(),
        }
    }

    fn contents_stamp(&self) -> (u64, u64) {
        match self {
            Self::Count(count) => count.contents_stamp(),
            Self::Integers(rows) => rows.contents_stamp(),
            Self::Dates(rows) => rows.contents_stamp(),
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
    list_sections: ListSectionProjection,
    pub(crate) list_selection: xrack_runtime::TableSelection,
    section_renderer: OnceCell<xrack_runtime::ListSectionRenderer>,
    list_keys: OnceCell<Rc<xrack_runtime::ListSelectionKeys>>,
}

impl RepeatState {
    pub(crate) fn selection_keys(&self) -> &Rc<xrack_runtime::ListSelectionKeys> {
        self.list_keys.get_or_init(Default::default)
    }

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
                Value::IntModel(model) => Ok(Model::Integers(model)),
                Value::IntList(items) => Ok(Model::Integers(items.into())),
                Value::DateModel(model) => Ok(Model::Dates(model)),
                Value::DateList(items) => Ok(Model::Dates(items.into())),
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
        let repeat = template
            .repeat
            .as_ref()
            .ok_or_else(|| Error::Invalid("missing repeated template".into()))?;
        xrack_runtime::try_render_rows_from_source(
            || self.row_model(owner, repeat),
            |model| {
                self.render_indices(
                    owner,
                    template,
                    &model,
                    range,
                    &TableColumnWindow::default(),
                )
            },
        )
    }

    pub(crate) fn render_indices(
        &self,
        owner: &Instance,
        template: &Element,
        model: &Rc<Model>,
        indices: impl Iterator<Item = usize>,
        columns: &TableColumnWindow,
    ) -> Result<Vec<Node>> {
        let repeat = template
            .repeat
            .as_ref()
            .ok_or_else(|| Error::Invalid("missing repeated template".into()))?;
        let allocate = |context| Self::allocate_row(owner, template, repeat, context);
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
                        .ok_or_else(|| {
                            Error::Evaluation("failed to initialize display row".into())
                        })?
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

    fn allocate_row(
        owner: &Instance,
        template: &Element,
        repeat: &Repeat,
        context: xrack_runtime::RowContext<Value>,
    ) -> Result<Instance> {
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
    }

    pub(crate) fn prepare_loader(&self, owner: &Instance, template: &Element) -> Result<()> {
        let repeat = template
            .repeat
            .as_ref()
            .ok_or_else(|| Error::Invalid("missing Loader candidate".into()))?;
        let model = self.row_model(owner, repeat)?;
        self.rows.try_render_resolved_indices(
            model.as_ref(),
            0..1,
            None,
            |context| Self::allocate_row(owner, template, repeat, context),
            |_, _, _, _, _| Ok(Node::hidden()),
        )?;
        Ok(())
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
                ListRowRenderer::with_columns_in_scene(move |range, columns, scene| {
                    let Some(owner) = owner.upgrade().map(Instance::from_state) else {
                        return Vec::new();
                    };
                    let Some(view) = owner.definition().root.find_key(&key) else {
                        return Vec::new();
                    };
                    owner
                        .capture_result(owner.view_rows(view, range, columns, scene))
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

    pub(crate) fn selection_rows(&self) -> Option<TableRows<Model>> {
        let source = self.source.clone()?;
        Some(if let Some(positions) = self.positions() {
            TableRows::from_positions(source, positions.clone())
        } else {
            TableRows::from_source(source)
        })
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
        let state = self.repeat_state(view);
        if view
            .table
            .as_ref()
            .is_some_and(|table| table.model.is_some() || view.visible_field.is_some())
        {
            let (projected, source) = self.project_table(view, &state)?;
            return Ok(ViewModel {
                source: Some(source),
                projected: Some(projected),
                ordered: None,
            });
        }
        let source = self.view_source_model(view)?;
        let projected = match (source.as_deref(), &view.visible_field) {
            (Some(Model::Records(records)), _) if view.table.is_some() => {
                Some(state.row_order.rows(Rc::new(records.model().clone())))
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

    pub(crate) fn view_count(&self, target: &str, source: bool) -> Result<usize> {
        let (owner, view) = self.element_owner(target)?;
        if source {
            return Ok(owner
                .view_source_model(view)?
                .as_deref()
                .map_or(0, ListModel::row_count));
        }
        if view.tree.is_some() {
            return owner
                .tree_projection(view)
                .map(|(tree, _)| tree.rows().len());
        }
        owner.view_model(view).map(|model| model.row_count())
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

    pub(crate) fn selected_list_index(&self, target: &str) -> Result<i64> {
        let (owner, view) = self.element_owner(target)?;
        let property = if view.row_key_field.is_some() {
            "current-key"
        } else {
            "current-index"
        };
        let Some(Value::Int(requested)) = owner.control_value(view, property)? else {
            return Ok(-1);
        };
        if requested < 0 {
            return Ok(-1);
        }
        let model = owner.view_model(view)?;
        if view.row_key_field.is_some() {
            owner.index_for_business_key(view, &model, requested)
        } else {
            Ok(ListMetrics::effective_index(requested, model.row_count()))
        }
    }

    pub(crate) fn list_selection_index(&self, view: &Element, model: &ViewModel) -> Result<i64> {
        if view.row_key_field.is_none() {
            return self.current_index(view);
        }
        let Some(Value::Int(key)) = self.control_value(view, "current-key")? else {
            return Err(Error::Invalid("missing list selection key".into()));
        };
        self.index_for_business_key(view, model, key)
    }

    pub(crate) fn index_for_business_key(
        &self,
        view: &Element,
        model: &ViewModel,
        key: i64,
    ) -> Result<i64> {
        if view.row_key_field.is_none() {
            return Err(Error::Invalid("missing list key field".into()));
        }
        Ok(model
            .source
            .as_deref()
            .and_then(|source| self.business_key_source_index(view, source, key))
            .and_then(|source| model.visual_index(source))
            .map_or(-1, |index| index as i64))
    }

    pub(crate) fn business_key_source_index(
        &self,
        view: &Element,
        source: &Model,
        key: i64,
    ) -> Option<usize> {
        let field = view.row_key_field.clone()?;
        let Model::Records(records) = source else {
            return None;
        };
        self.repeat_state(view)
            .selection_keys()
            .find(records.model(), key, move |row| list_key(row, &field))
    }

    pub(crate) fn list_selection_value(
        &self,
        view: &Element,
        model: &ViewModel,
        index: i64,
    ) -> Option<(&'static str, Value)> {
        let Some(field) = view.row_key_field.clone() else {
            return Some(("current-index", Value::Int(index)));
        };
        let Model::Records(records) = model.source.as_deref()? else {
            return None;
        };
        let source = model.source_index(usize::try_from(index).ok()?)?;
        let key = self.repeat_state(view).selection_keys().key_at(
            records.model(),
            source,
            move |row| list_key(row, &field),
        )?;
        Some(("current-key", Value::Int(key)))
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
        if view.table.is_none() && view.row_key_field.is_some() {
            node.properties
                .insert(property, self.list_selection_index(view, &model)?.into());
        }
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
        if let Some(path) = &view.section_field
            && let Some(Model::Records(records)) = model.source.as_deref()
        {
            let path = path.clone();
            node.list_sections = Some(state.list_sections.snapshot(
                records.model(),
                model.positions(),
                move |record| match record.field_path(&path) {
                    Some(Value::String(label)) => label,
                    _ => "",
                },
            ));
        }
        if view.table.is_some() {
            self.render_table(view, &model, &state, node)?;
        } else if view
            .bindings
            .iter()
            .any(|binding| binding.name == "multi-select")
            && let Some(rows) = model.selection_rows()
        {
            state.list_selection.populate_list_scene(node, &rows);
        }
        if node.boolean("typeahead-enabled", view.typeahead_field.is_some()) {
            node.list_text_search = list_text_search(view, &model);
        }
        if self.definition().key_scroll_targets.contains(&view.key) {
            if let (Some(field), Some(Model::Records(records))) =
                (view.row_key_field.clone(), model.source.as_deref())
            {
                node.row_locator = model.clone().locator().map(|locator| {
                    locator.with_business_keys(
                        state.selection_keys().clone(),
                        Rc::new(records.model().clone()),
                        model.positions().cloned(),
                        move |row| list_key(row, &field),
                    )
                });
            }
        } else if self.definition().source_scroll_targets.contains(&view.key)
            || node.boolean("preserve-scroll-anchor", false)
            || node.boolean("drag-auto-scroll", false)
            || node.boolean("movable-rows", false)
        {
            node.row_locator = model.locator();
        }
        node.list_row_renderer = Some(state.renderer(self, view));
        if view.section_header.is_some() {
            node.list_section_renderer = Some(
                state
                    .section_renderer
                    .get_or_init(|| {
                        let owner = Rc::downgrade(&self.0);
                        let key = view.key.clone();
                        xrack_runtime::ListSectionRenderer::new(move |input| {
                            let owner = owner.upgrade().map(Instance::from_state)?;
                            let view = owner.definition().root.find_key(&key)?;
                            let template = view.section_header.as_deref()?;
                            owner.capture_result(owner.render_section_header(template, input))
                        })
                    })
                    .clone(),
            );
        }
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
        scene: Option<&Node>,
    ) -> Result<Vec<Node>> {
        if view.tree.is_some() {
            let template = view
                .view_template()
                .ok_or_else(|| Error::Invalid("missing tree row template".into()))?;
            let (tree, model) = self.tree_projection(view)?;
            if !tree.matches_scene_order(scene) {
                return Ok(Vec::new());
            }
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
        if view
            .table
            .as_ref()
            .is_some_and(|table| table.model.is_some())
        {
            return self.standard_table_rows(
                view,
                range,
                columns,
                scene.and_then(|node| node.standard_list_stamp),
            );
        }
        let model = self.view_model(view)?;
        if !ListMetrics::matches_scene_order(scene, model.stamp()) {
            return Ok(Vec::new());
        }
        if let Some(list) = &view.standard_list {
            return model.standard_rows(&list.label, range);
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

fn list_text_search(view: &Element, model: &ViewModel) -> Option<xrack_runtime::ListTextSearch> {
    let order = model.positions().cloned();
    match model.source.as_deref()? {
        Model::Strings(rows) => Some(xrack_runtime::ListTextSearch::new(
            rows.clone(),
            order,
            String::as_str,
        )),
        Model::Records(records) => {
            let field = view.typeahead_field.clone().or_else(|| {
                view.standard_list
                    .as_ref()
                    .map(|standard| standard.label.clone().into())
            })?;
            Some(xrack_runtime::ListTextSearch::new(
                records.model().clone(),
                order,
                move |row| match row.field_path(&field) {
                    Some(Value::String(label)) => label,
                    _ => "",
                },
            ))
        }
        _ => None,
    }
}

pub(crate) fn list_key(row: &RecordValue, field: &[String]) -> i64 {
    match row.field_path(field) {
        Some(Value::Int(key)) => *key,
        _ => -1,
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

    pub(crate) fn empty_state(&self) -> Option<&Element> {
        self.children
            .iter()
            .find(|child| matches!(child.visual, Visual::Builtin("EmptyState")))
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
