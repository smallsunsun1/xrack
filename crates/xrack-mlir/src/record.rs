use crate::{OperationRef, RepeatRef, ValueRef, scene_children};
use xrack_syntax::ast::{Parameter, Type};

pub struct RecordDeclRef<'m> {
    pub name: &'m str,
    pub fields: Vec<Parameter>,
}

impl<'m> RecordDeclRef<'m> {
    pub fn from_operation(op: OperationRef<'m>) -> Result<Self, String> {
        if op.name() != "xr.record_decl" {
            return Err(format!(
                "expected a record declaration, found {}",
                op.name()
            ));
        }
        Ok(Self {
            name: op.attr("sym_name")?,
            fields: serde_json::from_str(op.attr("fields")?).map_err(|error| error.to_string())?,
        })
    }
}

pub fn record_field_path(mut value: ValueRef<'_>) -> Result<(ValueRef<'_>, Vec<&str>), String> {
    let mut path = Vec::new();
    while let Some(operation) = value
        .defining_operation()
        .filter(|op| op.name() == "xr.get_field")
    {
        path.push(operation.attr("value")?);
        value = operation
            .operand(0)
            .ok_or("record field missing its input")?;
    }
    path.reverse();
    Ok((value, path))
}

pub fn record_role(element: OperationRef<'_>, name: &str) -> Result<Option<String>, String> {
    element
        .children()
        .find(|binding| binding.name() == "xr.bind" && binding.attribute("name") == Some(name))
        .map(|binding| {
            let literal = binding
                .operand(0)
                .and_then(|value| value.defining_operation())
                .ok_or("record role missing its literal")?;
            serde_json::from_str(literal.attr("value")?).map_err(|error| error.to_string())
        })
        .transpose()
}

/// Model binding and record roles of a verified record-backed ComboBox.
pub struct RecordComboRef<'m> {
    pub model: OperationRef<'m>,
    pub record: Box<str>,
    pub label: String,
    pub value: String,
    pub enabled: Option<String>,
}

impl<'m> RecordComboRef<'m> {
    pub fn from_operation(element: OperationRef<'m>) -> Result<Option<Self>, String> {
        if element.attribute("kind") != Some("ComboBox") {
            return Ok(None);
        }
        let Some(model) = element
            .children()
            .find(|op| op.name() == "xr.bind" && op.attribute("name") == Some("model"))
        else {
            return Ok(None);
        };
        let Type::RecordList(record) = model
            .operand(0)
            .ok_or("ComboBox model missing operand")?
            .xr_type()?
        else {
            return Ok(None);
        };
        let label = record_role(element, "text-role")?.unwrap_or_else(|| "text".into());
        let value = record_role(element, "value-role")?.unwrap_or_else(|| label.clone());
        Ok(Some(Self {
            model,
            record,
            label,
            value,
            enabled: record_role(element, "enabled-role")?,
        }))
    }
}

/// Record fields and their bindings in a verified StandardTableView.
pub struct StandardTableRef<'m> {
    pub model: OperationRef<'m>,
    pub record: Box<str>,
    pub columns: Vec<StandardTableColumnRef<'m>>,
    pub row_header: Option<String>,
}

pub struct StandardTableColumnRef<'m> {
    pub field: String,
    pub editable: Option<OperationRef<'m>>,
}

impl<'m> StandardTableRef<'m> {
    pub fn from_operation(table: OperationRef<'m>) -> Result<Self, String> {
        let model = table
            .children()
            .find(|op| op.name() == "xr.bind" && op.attribute("name") == Some("model"))
            .ok_or("standard table requires a model")?;
        let Type::RecordList(record) = model
            .operand(0)
            .ok_or("standard table model missing operand")?
            .xr_type()?
        else {
            return Err("standard table requires a record model".into());
        };
        let columns = scene_children(table)
            .filter(|column| column.attribute("kind") == Some("TableColumn"))
            .map(|column| {
                Ok(StandardTableColumnRef {
                    field: record_role(column, "field")?
                        .ok_or("standard table column requires a field")?,
                    editable: column.children().find(|op| {
                        op.name() == "xr.bind" && op.attribute("name") == Some("editable")
                    }),
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(Self {
            model,
            record,
            columns,
            row_header: record_role(table, "row-header-field")?,
        })
    }
}

/// Row model and record projections of a verified TreeView.
pub struct TreeViewRef<'m> {
    pub row: OperationRef<'m>,
    pub repeat: RepeatRef<'m>,
    pub record: Box<str>,
    pub key: Vec<&'m str>,
    pub parent: Vec<&'m str>,
    pub visible_field: Option<String>,
    pub filter_field: Option<String>,
}

impl<'m> TreeViewRef<'m> {
    pub fn from_operation(tree: OperationRef<'m>) -> Result<Self, String> {
        let row = scene_children(tree)
            .find(|op| op.attribute("kind") == Some("TreeRow"))
            .ok_or("TreeView is missing a row template")?;
        let repeat = RepeatRef::from_operation(
            row.children()
                .find(|op| op.name() == "xr.repeat")
                .ok_or("TreeRow is missing a repeat")?,
        )?;
        let Type::RecordList(record) = repeat.model.xr_type()? else {
            return Err("TreeRow requires a record model".into());
        };
        let role = |name| {
            let input = row
                .children()
                .find(|op| op.name() == "xr.bind" && op.attribute("name") == Some(name))
                .and_then(|op| op.operand(0))
                .ok_or_else(|| format!("TreeRow is missing `{name}`"))?;
            record_field_path(input).map(|(_, path)| path)
        };
        Ok(Self {
            row,
            repeat,
            record,
            key: role("key")?,
            parent: role("parent-key")?,
            visible_field: record_role(tree, "row-visible-field")?,
            filter_field: record_role(tree, "filter-field")?,
        })
    }
}
