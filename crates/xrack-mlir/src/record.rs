use crate::{OperationRef, ValueRef};
use xrack_syntax::ast::Parameter;

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
