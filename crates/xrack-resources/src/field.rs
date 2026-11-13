use xrack_mlir::OperationRef;
use xrack_runtime::Value;
use xrack_syntax::ast::Type;

#[derive(Clone, Copy, Debug)]
pub struct BuiltinField {
    member: &'static str,
    read: fn(Value) -> Option<Value>,
}

impl BuiltinField {
    pub fn from_operation(op: OperationRef<'_>) -> Result<Self, String> {
        let input = op.operand(0).ok_or("missing built-in field operand")?;
        let input_type = input.type_name();
        let owner = input_type
            .strip_prefix("!xr.")
            .and_then(Type::parse)
            .ok_or_else(|| format!("unknown built-in field operand type {input_type}"))?;
        let name = op.attr("value")?;
        let (operation, ty) = owner
            .builtin_field(name)
            .ok_or_else(|| format!("{} has no field `{name}`", owner.name()))?;
        let result = op.result(0).ok_or("missing built-in field result")?;
        if op.name().strip_prefix("xr.") != Some(operation)
            || result.type_name().strip_prefix("!xr.") != Some(ty.name().as_ref())
        {
            return Err(format!(
                "incorrect operation or result type for field `{name}`"
            ));
        }
        Ok(builtin_field(&owner, name))
    }

    pub fn member(self) -> &'static str {
        self.member
    }

    pub fn read(self, value: Value) -> Option<Value> {
        (self.read)(value)
    }
}

macro_rules! field_access {
    ($($owner:ident, $op:literal { $($name:literal => $member:ident: $result:ident,)* })*) => {
        fn builtin_field(owner: &Type, name: &str) -> BuiltinField {
            match (owner, name) {
                $($((Type::$owner, $name) => BuiltinField {
                    member: stringify!($member),
                    read: |value| match value {
                        Value::$owner(input) => Some(Value::$result(input.$member)),
                        _ => None,
                    },
                },)*)*
                _ => unreachable!("validated built-in field"),
            }
        }
    };
}

xrack_syntax::builtin_value_fields!(field_access);
