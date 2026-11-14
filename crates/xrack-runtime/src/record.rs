use crate::{
    ChangeSignal, RowKey, TableHistoryPayload, TableRowPatch, Value, VecModel, model::RowObserver,
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fmt,
    rc::{Rc, Weak},
};

#[derive(Clone, Debug)]
pub struct RecordValue {
    name: Rc<str>,
    fields: Rc<BTreeMap<String, Value>>,
}

impl PartialEq for RecordValue {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.fields.len() == other.fields.len()
            && self.fields.iter().zip(other.fields.iter()).all(
                |((left_name, left), (right_name, right))| {
                    left_name == right_name && left.eq_with_image_assets(right)
                },
            )
    }
}

impl RecordValue {
    pub fn new(name: impl Into<Rc<str>>, fields: BTreeMap<String, Value>) -> Self {
        Self {
            name: name.into(),
            fields: Rc::new(fields),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn field(&self, name: &str) -> Option<&Value> {
        self.fields.get(name)
    }

    pub fn fields(&self) -> impl ExactSizeIterator<Item = (&str, &Value)> {
        self.fields
            .iter()
            .map(|(name, value)| (name.as_str(), value))
    }

    pub fn field_path(&self, path: &[String]) -> Option<&Value> {
        let (name, rest) = path.split_first()?;
        let field = self.field(name)?;
        if rest.is_empty() {
            Some(field)
        } else if let Value::Record(record) = field {
            record.field_path(rest)
        } else {
            None
        }
    }

    pub fn with_field(&self, path: &[String], value: Value) -> Option<Self> {
        let (name, rest) = path.split_first()?;
        let mut record = self.clone();
        let target = Rc::make_mut(&mut record.fields).get_mut(name)?;
        if rest.is_empty() {
            *target = value;
        } else if let Value::Record(nested) = target {
            *nested = nested.with_field(rest, value)?;
        } else {
            return None;
        }
        Some(record)
    }
}

impl TableRowPatch<RecordValue> {
    /// Save changed leaf fields so replay preserves independent changes to the same record.
    pub fn record_changes(&mut self, before: &RecordValue, after: &RecordValue) {
        self.record_record(before, after, &mut Vec::new());
    }

    fn record_record(&mut self, before: &RecordValue, after: &RecordValue, path: &mut Vec<String>) {
        if Rc::ptr_eq(&before.fields, &after.fields) {
            return;
        }
        for (name, before) in before.fields.iter() {
            let Some(after) = after.field(name) else {
                continue;
            };
            path.push(name.clone());
            if let (Value::Record(before), Value::Record(after)) = (before, after) {
                self.record_record(before, after, path);
            } else if !same_field(before, after) {
                let field = path.clone();
                self.record_with(
                    before,
                    after,
                    same_field,
                    field_bytes,
                    path,
                    move |row, expected, value| {
                        if !row
                            .field_path(&field)
                            .is_some_and(|current| same_field(current, expected))
                        {
                            return false;
                        }
                        let Some(next) = row.with_field(&field, value.clone()) else {
                            return false;
                        };
                        *row = next;
                        true
                    },
                );
            }
            path.pop();
        }
    }
}

fn same_field(before: &Value, after: &Value) -> bool {
    before.eq_with_image_assets(after)
        || match (before, after) {
            (Value::Float(before), Value::Float(after)) => before.to_bits() == after.to_bits(),
            (Value::Length(before), Value::Length(after)) => {
                before.0.to_bits() == after.0.to_bits()
            }
            _ => false,
        }
}

fn field_bytes(value: &Value) -> usize {
    match value {
        Value::String(value) => value.history_payload_bytes(),
        Value::Font(value) => value.history_payload_bytes(),
        Value::KeyEvent(value) => value.history_payload_bytes(),
        Value::StyledText(value) => value.history_payload_bytes(),
        Value::Brush(value) => value.history_payload_bytes(),
        _ => 0,
    }
}

#[doc(hidden)]
pub type RecordRowValidator = dyn Fn(&str, &RecordValue) -> Result<(), String>;

#[derive(Default)]
struct ValidationState {
    failures: RefCell<BTreeMap<RowKey, String>>,
    changed: ChangeSignal,
}

struct RecordValidation {
    validator: Weak<RecordRowValidator>,
    state: Rc<ValidationState>,
    _observer: Rc<RowObserver<RecordValue>>,
}

impl RecordValidation {
    fn new(list: &RecordList, validator: &Rc<RecordRowValidator>) -> Self {
        let state = Rc::new(ValidationState::default());
        let mut failures = state.failures.borrow_mut();
        list.model.visit_rows_untracked(usize::MAX, |_, key, row| {
            if let Err(error) = validator(&list.name, row) {
                failures.insert(key, error);
            }
        });
        drop(failures);

        let validator = Rc::downgrade(validator);
        let observer: Rc<RowObserver<RecordValue>> = Rc::new({
            let state = state.clone();
            let validator = validator.clone();
            let name = list.name.clone();
            move |key, row| {
                let Some(validator) = validator.upgrade() else {
                    return;
                };
                let error = row.and_then(|row| validator(&name, row).err());
                let mut failures = state.failures.borrow_mut();
                if failures.get(&key) != error.as_ref() {
                    if let Some(error) = error {
                        failures.insert(key, error);
                    } else {
                        failures.remove(&key);
                    }
                    state.changed.notify();
                }
            }
        });
        list.model.observe_rows(&observer);
        Self {
            validator,
            state,
            _observer: observer,
        }
    }
}

#[derive(Clone)]
pub struct RecordList {
    name: Rc<str>,
    model: VecModel<RecordValue>,
    validations: Rc<RefCell<Vec<RecordValidation>>>,
}

impl PartialEq for RecordList {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.model == other.model
    }
}

impl RecordList {
    pub fn new(name: impl Into<Rc<str>>, rows: Vec<RecordValue>) -> Self {
        Self::from_model(name, VecModel::from(rows))
    }

    /// Use an existing model, preserving the row identities of the supplied handle.
    pub fn from_model(name: impl Into<Rc<str>>, model: VecModel<RecordValue>) -> Self {
        Self {
            name: name.into(),
            model,
            validations: Rc::default(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn model(&self) -> &VecModel<RecordValue> {
        &self.model
    }

    #[doc(hidden)]
    pub fn into_snapshot(self) -> (Rc<str>, Vec<RecordValue>) {
        (self.name, self.model.into_snapshot())
    }

    #[doc(hidden)]
    pub fn values_eq_owned(
        self,
        other: Self,
        validator: &Rc<RecordRowValidator>,
    ) -> Result<bool, String> {
        for list in [&self, &other] {
            list.check_rows(validator, list.model.is_shared())
                .map_err(|error| format!("model {}{error}", list.name))?;
        }
        Ok(self.name == other.name && self.model.values_eq_owned(other.model))
    }

    /// Check the current row shapes and track changes to their validity.
    #[doc(hidden)]
    pub fn validate_rows(&self, validator: &Rc<RecordRowValidator>) -> Result<(), String> {
        self.check_rows(validator, true)
    }

    #[doc(hidden)]
    pub fn validate_rows_untracked(
        &self,
        validator: &Rc<RecordRowValidator>,
    ) -> Result<(), String> {
        self.check_rows(validator, false)
    }

    fn check_rows(&self, validator: &Rc<RecordRowValidator>, tracked: bool) -> Result<(), String> {
        let mut validations = self.validations.borrow_mut();
        validations.retain(|entry| entry.validator.strong_count() != 0);
        let key = Rc::downgrade(validator);
        let index = validations
            .iter()
            .position(|entry| entry.validator.ptr_eq(&key))
            .unwrap_or_else(|| {
                validations.push(RecordValidation::new(self, validator));
                validations.len() - 1
            });
        let state = &validations[index].state;
        if tracked {
            state.changed.track();
        }
        let failures = state.failures.borrow();
        let Some((&row, error)) = failures.first_key_value() else {
            return Ok(());
        };
        let index = if tracked {
            self.model.row_index(row)
        } else {
            self.model.row_index_untracked(row)
        };
        Err(index.map_or_else(|| error.clone(), |index| format!("[{index}]{error}")))
    }
}

impl fmt::Debug for RecordList {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output
            .debug_struct("RecordList")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}
