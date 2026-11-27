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
        let owner = input
            .xr_type()
            .map_err(|error| format!("unknown {error} for built-in field operand"))?;
        let name = op.attr("value")?;
        let (operation, ty) = owner
            .builtin_field(name)
            .ok_or_else(|| format!("{} has no field `{name}`", owner.name()))?;
        let result = op.result(0).ok_or("missing built-in field result")?;
        if op.name().strip_prefix("xr.") != Some(operation) || result.xr_type()? != ty {
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

macro_rules! read_member {
    (Bounds3D, $input:ident, $member:ident) => {
        $input.$member()
    };
    (MeshNode3D, $input:ident, valid) => {
        $input.valid()
    };
    (MeshAsset3D, $input:ident, $member:ident) => {
        $input.$member()
    };
    (RotationTrack3D, $input:ident, $member:ident) => {
        $input.$member()
    };
    (Quaternion3D, $input:ident, $member:ident) => {
        $input.$member()
    };
    (WalkResult3D, $input:ident, contacts) => {
        $input.contacts()
    };
    (TextureData2D, $input:ident, $member:ident) => {
        $input.$member()
    };
    (InstanceTable3D, $input:ident, count) => {
        $input.count()
    };
    (InstanceTable3D, $input:ident, loaded) => {
        $input.is_loaded()
    };
    (Instance3D, $input:ident, object_id) => {
        $input.object_id.clone()
    };
    (RenderStats3D, $input:ident, work) => {
        $input.work.clone()
    };
    (Hit3D, $input:ident, object_id) => {
        $input.object_id.clone()
    };
    (Hit3D, $input:ident, item_source) => {
        $input.item_source().to_owned()
    };
    (Hit3D, $input:ident, item_position) => {
        $input.item_position()
    };
    ($owner:ident, $input:ident, $member:ident) => {
        $input.$member
    };
}

macro_rules! field_access {
    ($($owner:ident, $op:literal { $($name:literal => $member:ident: $result:ident,)* })*) => {
        fn builtin_field(owner: &Type, name: &str) -> BuiltinField {
            match (owner, name) {
                $($((Type::$owner, $name) => BuiltinField {
                    member: stringify!($member),
                    read: |value| match value {
                        Value::$owner(input) => Some(Value::$result(read_member!($owner, input, $member))),
                        _ => None,
                    },
                },)*)*
                _ => unreachable!("validated built-in field"),
            }
        }
    };
}

xrack_syntax::builtin_value_fields!(field_access);
