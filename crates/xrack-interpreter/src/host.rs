use crate::{
    Error, Program, RecordList, RecordRowValidator, RecordSchemas, RecordValue, Result, Value,
    ValueType,
};
use std::{fmt::Display, rc::Rc};

pub(super) fn record_validator(records: Rc<RecordSchemas>) -> Rc<RecordRowValidator> {
    Rc::new(move |name, row| {
        validate_record(&records, name, row, &mut String::new()).map_err(|error| error.to_string())
    })
}

impl Program {
    /// Observe the validity of a model returned to a reactive host reader.
    pub(super) fn observe_host_value(
        &self,
        ty: &ValueType,
        value: &Value,
        context: impl Display,
    ) -> Result<()> {
        if let (ValueType::RecordList(name), Value::RecordList(list)) = (ty, value)
            && name.as_ref() == list.name()
        {
            return list
                .validate_rows(&self.record_validator)
                .map_err(|error| Error::Evaluation(format!("{context}{error}")));
        }
        self.validate_host_value(ty, value, context)
    }

    pub(super) fn validate_host_value(
        &self,
        ty: &ValueType,
        value: &Value,
        context: impl Display,
    ) -> Result<()> {
        match (ty, value) {
            (ValueType::Record(name), Value::Record(record)) => {
                validate_record(&self.records, name, record, &mut context.to_string())
            }
            (ValueType::RecordList(name), Value::RecordList(list))
                if name.as_ref() == list.name() =>
            {
                list.validate_rows_untracked(&self.record_validator)
                    .map_err(|error| Error::Evaluation(format!("{context}{error}")))
            }
            _ if ty.matches(value) => Ok(()),
            _ => Err(Error::Evaluation(format!("{context}: expected {ty:?}"))),
        }
    }

    pub(super) fn validate_live_record_list(&self, list: &RecordList) -> Result<()> {
        list.validate_rows(&self.record_validator)
            .map_err(|error| Error::Evaluation(format!("model {}{error}", list.name())))
    }

    /// A model lookup depends on the selected row, including its shape.
    pub(super) fn record_at(&self, list: &RecordList, index: i64) -> Result<Option<RecordValue>> {
        let Some(row) = list.model().row_at(index) else {
            return Ok(None);
        };
        let name = list.name();
        validate_record(
            &self.records,
            name,
            &row,
            &mut format!("model {name}[{index}]"),
        )?;
        Ok(Some(row))
    }

    pub(super) fn validate_callback_arguments(
        &self,
        name: &str,
        parameters: &[ValueType],
        arguments: &[Value],
    ) -> Result<()> {
        if arguments.len() != parameters.len() {
            return Err(Error::Evaluation(format!(
                "callback {name}: expected {} arguments, found {}",
                parameters.len(),
                arguments.len()
            )));
        }
        for (index, (ty, value)) in parameters.iter().zip(arguments).enumerate() {
            let argument = index + 1;
            self.validate_host_value(
                ty,
                value,
                format_args!("callback {name} argument {argument}"),
            )?;
        }
        Ok(())
    }

    pub(super) fn validate_callback_result(
        &self,
        name: &str,
        ty: Option<&ValueType>,
        value: Option<&Value>,
    ) -> Result<()> {
        match (ty, value) {
            (Some(ty), Some(value)) => {
                self.validate_host_value(ty, value, format_args!("callback {name} return value"))
            }
            (None, None) => Ok(()),
            _ => Err(Error::Evaluation(format!(
                "callback {name}: return value does not match its declaration"
            ))),
        }
    }
}

fn validate_record(
    records: &RecordSchemas,
    name: &str,
    record: &RecordValue,
    path: &mut String,
) -> Result<()> {
    if record.name() != name {
        return Err(Error::Evaluation(format!(
            "{path}: expected record {name}, found {}",
            record.name()
        )));
    }
    let fields = records
        .get(name)
        .ok_or_else(|| Error::Evaluation(format!("{path}: unknown record {name}")))?;
    for field in fields {
        let length = path.len();
        path.push('.');
        path.push_str(&field.name);
        let value = record
            .field(&field.name)
            .ok_or_else(|| Error::Evaluation(format!("{path}: missing field")))?;
        match (&field.ty, value) {
            (ValueType::Record(name), Value::Record(record)) => {
                validate_record(records, name, record, path)?;
            }
            _ if !field.ty.matches(value) => {
                return Err(Error::Evaluation(format!(
                    "{path}: expected {:?}",
                    field.ty
                )));
            }
            _ => {}
        }
        path.truncate(length);
    }
    if record.fields().len() != fields.len()
        && let Some((extra, _)) = record
            .fields()
            .find(|(name, _)| fields.iter().all(|field| field.name != *name))
    {
        return Err(Error::Evaluation(format!("{path}.{extra}: unknown field")));
    }
    Ok(())
}
