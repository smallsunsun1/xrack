use crate::{
    ChangeSignal, Length, ListMetrics, ListModel, Property, RowKey, TreeSnapshot, VecModel,
    list::MAX_ROWS,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq)]
struct RowHeight {
    index: usize,
    height: f32,
    delta: f64,
}

/// Sparse prefix sums for a virtual view's row heights.
#[derive(Debug, PartialEq)]
#[doc(hidden)]
pub struct RowHeights {
    base: f32,
    projected: Option<Arc<RowHeights>>,
    overrides: Vec<RowHeight>,
}

impl RowHeights {
    fn new(
        base: f32,
        sizes: impl IntoIterator<Item = (usize, f32)>,
        projected: Option<Arc<Self>>,
    ) -> Self {
        let mut overrides: Vec<_> = sizes
            .into_iter()
            .map(|(index, height)| RowHeight {
                index,
                height,
                delta: 0.0,
            })
            .collect();
        overrides.sort_by_key(|row| row.index);
        let mut prefix = 0.0;
        for row in &mut overrides {
            let previous = projected
                .as_ref()
                .map_or(base, |heights| heights.height(row.index));
            prefix += f64::from(row.height) - f64::from(previous);
            row.delta = prefix;
        }
        Self {
            base,
            projected,
            overrides,
        }
    }

    pub fn height(&self, index: usize) -> f32 {
        self.overrides
            .binary_search_by_key(&index, |row| row.index)
            .map_or_else(
                |_| {
                    self.projected
                        .as_ref()
                        .map_or(self.base, |heights| heights.height(index))
                },
                |slot| self.overrides[slot].height,
            )
    }

    pub fn top(&self, index: usize) -> f64 {
        let slot = self.overrides.partition_point(|row| row.index < index);
        let delta = slot
            .checked_sub(1)
            .map_or(0.0, |slot| self.overrides[slot].delta);
        self.projected.as_ref().map_or_else(
            || index as f64 * f64::from(self.base),
            |heights| heights.top(index),
        ) + delta
    }
}

struct ProjectionInput {
    stamp: (u64, u64),
    base: f32,
    tree: Option<Arc<TreeSnapshot>>,
}

type FieldUpdates = Rc<RefCell<BTreeMap<usize, Option<f32>>>>;

struct FieldObservation {
    model: u64,
    changed: Rc<ChangeSignal>,
    updates: FieldUpdates,
}

#[derive(Default)]
struct FieldHeights {
    stamp: Option<(u64, u64)>,
    values: BTreeMap<usize, f32>,
}

/// Tracks only the record field that controls layout.
#[derive(Default)]
#[doc(hidden)]
pub struct RowHeightProjection {
    input: RefCell<Option<ProjectionInput>>,
    input_changed: ChangeSignal,
    observation: RefCell<Option<FieldObservation>>,
    fields: RefCell<FieldHeights>,
    geometry: Property<Option<Arc<RowHeights>>>,
}

fn projected_height(value: Length) -> Option<f32> {
    (value.0.is_finite() && value.0 > 0.0).then(|| ListMetrics::clamp_row_height(value.0 as f32))
}

impl RowHeightProjection {
    pub fn snapshot<T: 'static>(
        &self,
        model: &VecModel<T>,
        base: f32,
        project: fn(&T) -> Length,
    ) -> Option<Arc<RowHeights>> {
        self.project(model, base, None, project)
    }

    pub fn tree_snapshot<T: 'static>(
        &self,
        model: &VecModel<T>,
        base: f32,
        tree: &Arc<TreeSnapshot>,
        project: fn(&T) -> Length,
    ) -> Option<Arc<RowHeights>> {
        self.project(model, base, Some(tree), project)
    }

    fn project<T: 'static>(
        &self,
        model: &VecModel<T>,
        base: f32,
        tree: Option<&Arc<TreeSnapshot>>,
        field: fn(&T) -> Length,
    ) -> Option<Arc<RowHeights>> {
        let stamp = model.structure_stamp();
        {
            let mut input = self.input.borrow_mut();
            if input.as_ref().is_none_or(|input| {
                input.stamp != stamp || input.base != base || !same_arc(input.tree.as_ref(), tree)
            }) {
                *input = Some(ProjectionInput {
                    stamp,
                    base,
                    tree: tree.cloned(),
                });
                self.input_changed.notify();
            }
        }
        self.geometry.get(|| {
            self.input_changed.track();
            let project = move |row: &T| projected_height(field(row));
            let updates = {
                let mut observation = self.observation.borrow_mut();
                if observation
                    .as_ref()
                    .is_none_or(|entry| entry.model != stamp.0)
                {
                    let updates = FieldUpdates::default();
                    let pending = Rc::downgrade(&updates);
                    let changed =
                        model.observe_projection_changes(MAX_ROWS, move |index, old, new| {
                            let before = project(old);
                            let after = project(new);
                            if before == after {
                                return false;
                            }
                            let Some(pending) = pending.upgrade() else {
                                return false;
                            };
                            pending.borrow_mut().insert(index, after);
                            true
                        });
                    *observation = Some(FieldObservation {
                        model: stamp.0,
                        changed,
                        updates,
                    });
                }
                let observation = observation.as_ref().expect("installed field observation");
                observation.changed.track();
                observation.updates.take()
            };
            let mut fields = self.fields.borrow_mut();
            if fields.stamp == Some(stamp) {
                for (index, height) in updates {
                    if let Some(height) = height {
                        fields.values.insert(index, height);
                    } else {
                        fields.values.remove(&index);
                    }
                }
            } else {
                // Inserts, removals, and sorting invalidate old model indices.
                fields.stamp = Some(stamp);
                fields.values.clear();
                model.visit_rows_untracked(MAX_ROWS, |index, _, row| {
                    if let Some(height) = project(row) {
                        fields.values.insert(index, height);
                    }
                });
            }
            let sizes: Vec<_> = if let Some(tree) = tree {
                tree.rows()
                    .iter()
                    .enumerate()
                    .filter_map(|(index, row)| {
                        let &height = fields.values.get(&row.model_index)?;
                        (height != base).then_some((index, height))
                    })
                    .collect()
            } else {
                fields
                    .values
                    .iter()
                    .filter_map(|(&index, &height)| (height != base).then_some((index, height)))
                    .collect()
            };
            (!sizes.is_empty()).then(|| Arc::new(RowHeights::new(base, sizes, None)))
        })
    }
}

fn same_arc<T>(left: Option<&Arc<T>>, right: Option<&Arc<T>>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}

#[derive(Default)]
struct Overrides {
    stamp: Option<(u64, u64)>,
    rows: BTreeMap<RowKey, (usize, f32)>,
    geometry: Option<Arc<RowHeights>>,
}

impl Overrides {
    fn reconcile(&mut self, model: &impl ListModel) {
        let stamp = model.structure_stamp();
        if self.stamp == Some(stamp) {
            return;
        }
        self.stamp = Some(stamp);
        self.geometry = None;
        self.rows.retain(|key, _| key.model == stamp.0);
        let count = model.row_count();
        let mut displaced: BTreeSet<_> = self
            .rows
            .iter()
            .filter_map(|(&key, &(index, _))| {
                (index >= count || model.row_key(index) != key).then_some(key)
            })
            .collect();
        if displaced.is_empty() {
            return;
        }
        let mut index = 0;
        model.visit_row_keys(0..count, |key| {
            if displaced.remove(&key) {
                self.rows.get_mut(&key).expect("tracked row").0 = index;
            }
            index += 1;
        });
        for key in displaced {
            self.rows.remove(&key);
        }
    }
}

/// Per-view overrides follow stable record identities, even when rows are reordered.
#[derive(Default)]
#[doc(hidden)]
pub struct RowHeightOverrides {
    state: RefCell<Overrides>,
    changed: ChangeSignal,
}

impl RowHeightOverrides {
    pub fn set(
        &self,
        model: &impl ListModel,
        index: usize,
        expected: Option<RowKey>,
        height: f64,
    ) -> bool {
        if index >= model.row_count() || !height.is_finite() {
            return false;
        }
        let key = model.row_key(index);
        if expected.is_some_and(|expected| expected != key) {
            return false;
        }
        let mut state = self.state.borrow_mut();
        let changed = if height < 0.0 {
            state.rows.remove(&key).is_some()
        } else {
            let height = ListMetrics::clamp_row_height(height as f32);
            if state.rows.get(&key).is_some_and(|&(_, old)| old == height) {
                return false;
            }
            state.rows.insert(key, (index, height));
            true
        };
        if changed {
            state.geometry = None;
            drop(state);
            self.changed.notify();
        }
        changed
    }

    pub fn snapshot(&self, model: &impl ListModel, base: f32) -> Option<Arc<RowHeights>> {
        self.snapshot_over(model, base, None)
    }

    pub fn snapshot_over(
        &self,
        model: &impl ListModel,
        base: f32,
        projected: Option<Arc<RowHeights>>,
    ) -> Option<Arc<RowHeights>> {
        self.changed.track();
        let mut state = self.state.borrow_mut();
        if state.rows.is_empty() {
            return projected;
        }
        state.reconcile(model);
        if state.rows.is_empty() {
            return projected;
        }
        if state.geometry.as_ref().is_none_or(|geometry| {
            geometry.base != base || !same_arc(geometry.projected.as_ref(), projected.as_ref())
        }) {
            state.geometry = Some(Arc::new(RowHeights::new(
                base,
                state.rows.values().copied(),
                projected,
            )));
        }
        state.geometry.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VecModel;

    #[test]
    fn field_edits_update_only_changed_rows_and_manual_overrides_remain_separate() {
        use std::cell::Cell;

        thread_local! { static READS: Cell<usize> = const { Cell::new(0) }; }
        fn read(row: &(i64, Length)) -> Length {
            READS.with(|reads| reads.set(reads.get() + 1));
            row.1
        }
        let model = VecModel::from(vec![(0, Length(32.0)); 10_000]);
        assert!(model.set_row(8_000, (0, Length(56.0))));
        let projection = RowHeightProjection::default();
        let scene = Property::default();
        let geometry = || {
            scene
                .get(|| projection.snapshot(&model, 32.0, read))
                .unwrap()
        };
        let initial = geometry();
        assert_eq!(initial.top(8_001), 8_000.0 * 32.0 + 56.0);
        READS.with(|reads| reads.set(0));
        assert!(model.set_row(8_000, (1, Length(56.0))));
        assert!(!scene.is_dirty());
        assert!(Arc::ptr_eq(&initial, &geometry()));
        assert!(model.set_row(8_000, (1, Length(96.0))));
        assert!(scene.is_dirty());
        let projected = geometry();
        assert_eq!(projected.height(8_000), 96.0);
        assert_eq!(READS.with(Cell::get), 4);

        let overrides = RowHeightOverrides::default();
        assert!(overrides.set(&model, 8_000, None, 80.0));
        let combined = overrides
            .snapshot_over(&model, 32.0, Some(projected))
            .unwrap();
        assert_eq!(combined.height(8_000), 80.0);
        assert_eq!(combined.top(8_001), 8_000.0 * 32.0 + 80.0);
        assert!(model.set_row(8_000, (1, Length(120.0))));
        let combined = overrides
            .snapshot_over(&model, 32.0, Some(geometry()))
            .unwrap();
        assert_eq!(combined.height(8_000), 80.0);
        assert!(overrides.set(&model, 8_000, None, -1.0));
        let restored = overrides
            .snapshot_over(&model, 32.0, Some(geometry()))
            .unwrap();
        assert_eq!(restored.height(8_000), 120.0);
    }

    #[test]
    fn sparse_row_heights_follow_model_keys_and_drop_orphaned_overrides() {
        let model = VecModel::from((0..100_000).collect::<Vec<_>>());
        let overrides = RowHeightOverrides::default();
        let key = model.row_key(90_000).unwrap();
        assert!(overrides.set(&model, 90_000, Some(key), 80.0));
        let original = overrides.snapshot(&model, 32.0).unwrap();
        assert!(Arc::ptr_eq(
            &original,
            &overrides.snapshot(&model, 32.0).unwrap()
        ));
        assert_eq!(original.top(90_001), 90_000.0 * 32.0 + 80.0);
        assert!(model.insert(0, -1));
        assert!(!overrides.set(&model, 90_000, Some(key), 90.0));
        let shifted = overrides.snapshot(&model, 32.0).unwrap();
        assert_eq!(shifted.height(90_001), 80.0);
        assert!(model.sort_by(|a, b| b.cmp(a)));
        let index = model.row_index(key).unwrap();
        let sorted = overrides.snapshot(&model, 32.0).unwrap();
        assert_eq!(sorted.height(index), 80.0);
        assert_eq!(sorted.height(index + 1), 32.0);
        assert!(model.remove(index).is_some());
        assert!(overrides.snapshot(&model, 32.0).is_none());
    }
}
