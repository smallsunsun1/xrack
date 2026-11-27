use crate::{
    ChangeSignal, Node, Property, RowKey, TableRowPositions, Value, VecModel, list::MAX_ROWS,
};
use std::{
    cell::{Cell, RefCell},
    fmt,
    rc::Rc,
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Eq)]
#[doc(hidden)]
pub struct ListSection {
    pub index: usize,
    pub key: RowKey,
    pub label: Arc<str>,
    pub heading_count: usize,
}

#[derive(Clone)]
#[doc(hidden)]
pub struct ListSectionInput {
    pub label: Arc<str>,
    pub first_index: i64,
    pub count: i64,
    pub pinned: bool,
}

#[derive(Clone, Copy)]
#[doc(hidden)]
pub enum ListSectionParameter {
    Label,
    FirstIndex,
    Count,
    Pinned,
}

impl ListSectionParameter {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "section-label" => Self::Label,
            "section-first-index" => Self::FirstIndex,
            "section-count" => Self::Count,
            "section-pinned" => Self::Pinned,
            _ => return None,
        })
    }

    pub fn read(self, input: &ListSectionInput) -> Value {
        match self {
            Self::Label => input.label.to_string().into(),
            Self::FirstIndex => input.first_index.into(),
            Self::Count => input.count.into(),
            Self::Pinned => input.pinned.into(),
        }
    }
}

type RenderSection = dyn Fn(ListSectionInput) -> Option<Node>;

/// Produces the visual subtree for a heading in the prepared viewport.
#[derive(Clone)]
#[doc(hidden)]
pub struct ListSectionRenderer(Rc<RenderSection>);

impl ListSectionRenderer {
    pub fn new(render: impl Fn(ListSectionInput) -> Option<Node> + 'static) -> Self {
        Self(Rc::new(render))
    }

    pub fn render(&self, input: ListSectionInput) -> Option<Node> {
        (self.0)(input)
    }
}

impl PartialEq for ListSectionRenderer {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for ListSectionRenderer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ListSectionRenderer")
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_snapshot_tracks_only_group_fields_and_model_order() {
        let model = VecModel::from(vec![
            ("A", "first"),
            ("A", "second"),
            ("", "note"),
            ("B", "last"),
        ]);
        let projection = ListSectionProjection::default();
        let snapshot = || projection.snapshot(&model, None, |row| row.0);
        let before = snapshot();
        assert_eq!(
            before
                .iter()
                .map(|run| (run.index, run.label.as_ref(), run.heading_count))
                .collect::<Vec<_>>(),
            [(0, "A", 1), (2, "", 1), (3, "B", 2)]
        );
        assert!(model.set_row(1, ("A", "renamed")));
        assert!(Arc::ptr_eq(&before, &snapshot()));
        assert!(model.set_row(1, ("B", "renamed")));
        let after = snapshot();
        assert_eq!(
            after
                .iter()
                .map(|run| (run.index, run.label.as_ref(), run.heading_count))
                .collect::<Vec<_>>(),
            [(0, "A", 1), (1, "B", 2), (2, "", 2), (3, "B", 3)]
        );
        assert!(model.sort_by(|a, b| a.0.cmp(b.0)));
        let after = snapshot();
        assert_eq!(
            after
                .iter()
                .map(|run| (run.index, run.label.as_ref()))
                .collect::<Vec<_>>(),
            [(0, ""), (1, "A"), (2, "B")]
        );
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    model: (u64, u64),
    positions: Option<(u64, u64)>,
}

#[derive(Default)]
#[doc(hidden)]
pub struct ListSectionProjection {
    stamp: Cell<Option<Stamp>>,
    input_changed: ChangeSignal,
    observation: RefCell<Option<(u64, Rc<ChangeSignal>)>>,
    sections: Property<Arc<[ListSection]>>,
}

impl ListSectionProjection {
    pub fn snapshot<T: 'static>(
        &self,
        model: &VecModel<T>,
        positions: Option<&TableRowPositions>,
        field: impl Fn(&T) -> &str + Clone + 'static,
    ) -> Arc<[ListSection]> {
        let stamp = Stamp {
            model: model.structure_stamp(),
            positions: positions.map(TableRowPositions::stamp),
        };
        if self.stamp.replace(Some(stamp)) != Some(stamp) {
            self.input_changed.notify();
        }
        self.sections.get(|| {
            self.input_changed.track();
            let source_id = stamp.model.0;
            let mut observation = self.observation.borrow_mut();
            if observation.as_ref().is_none_or(|(id, _)| *id != source_id) {
                let select = field.clone();
                let changed = model
                    .observe_projection_changes(MAX_ROWS, move |_, before, after| {
                        select(before) != select(after)
                    });
                *observation = Some((source_id, changed));
            }
            if let Some((_, changed)) = observation.as_ref() {
                changed.track();
            }
            drop(observation);

            let count = positions
                .map_or_else(|| model.len(), TableRowPositions::row_count)
                .min(MAX_ROWS);
            let mut sections: Vec<ListSection> = Vec::new();
            let mut append = |index, key, label: &str| {
                if sections
                    .last()
                    .is_some_and(|previous| &*previous.label == label)
                {
                    return;
                }
                sections.push(ListSection {
                    index,
                    key,
                    label: label.into(),
                    heading_count: sections.last().map_or(0, |run| run.heading_count)
                        + usize::from(!label.is_empty()),
                });
            };
            if let Some(positions) = positions {
                for index in 0..count {
                    if let Some(source) = positions.source_index(index)
                        && let Some(key) = model.row_key(source)
                    {
                        model.project_row_untracked(source, |row| append(index, key, field(row)));
                    }
                }
            } else {
                model.visit_rows_untracked(count, |index, key, row| append(index, key, field(row)));
            }
            if sections.last().is_some_and(|run| run.heading_count == 0) {
                return Arc::default();
            }
            sections.into()
        })
    }
}
