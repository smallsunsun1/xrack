use std::{borrow::Cow, collections::BTreeMap, fmt::Write, sync::Arc};

use crate::{RecordValue, Value};

use super::lighting::linear_color;

#[doc(hidden)]
pub const MAX_SKY_UNIFORM_SLOTS: usize = 1024;

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkyUniformType {
    Bool,
    Int,
    Uint,
    Float,
    Vec2,
    Vec3,
    Vec4,
    Mat3,
}

impl SkyUniformType {
    #[doc(hidden)]
    pub const fn slots(self) -> usize {
        match self {
            Self::Mat3 => 3,
            _ => 1,
        }
    }
}

#[doc(hidden)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkyUniformField {
    pub name: String,
    pub kind: SkyUniformType,
}

/// Shader source plus the uniform schema validated by the asset compiler.
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkyShader {
    pub source: Arc<str>,
    pub fields: Box<[SkyUniformField]>,
    pub uniform_interface: String,
}

impl SkyShader {
    pub fn new(source: Arc<str>, fields: Vec<SkyUniformField>) -> Self {
        let uniform_interface = uniform_source(&fields);
        Self {
            source,
            fields: fields.into_boxed_slice(),
            uniform_interface,
        }
    }

    pub(super) fn pack_uniforms(&self, values: Option<&RecordValue>) -> Arc<[[u32; 4]]> {
        if self.fields.is_empty() {
            return Arc::default();
        }
        let values: BTreeMap<_, _> = values
            .into_iter()
            .flat_map(RecordValue::fields)
            .map(|(name, value)| {
                let name = if name.contains('-') {
                    Cow::Owned(name.replace('-', "_"))
                } else {
                    Cow::Borrowed(name)
                };
                (name, value)
            })
            .collect();
        let mut words =
            Vec::with_capacity(self.fields.iter().map(|field| field.kind.slots()).sum());
        for field in &self.fields {
            let value = values.get(field.name.as_str()).copied();
            if field.kind == SkyUniformType::Mat3 {
                if let Some(Value::Transform2D(transform)) = value {
                    let [a, b, c, d, x, y] = transform.coefficients();
                    words.extend([
                        float_words([a, b, 0.0, 0.0]),
                        float_words([c, d, 0.0, 0.0]),
                        float_words([x, y, 1.0, 0.0]),
                    ]);
                } else {
                    words.extend([[0; 4]; 3]);
                }
            } else {
                words.push(pack_scalar_or_vector(field.kind, value));
            }
        }
        words.into()
    }
}

fn float_words(values: [f64; 4]) -> [u32; 4] {
    values.map(|value| {
        let value = value as f32;
        if value.is_finite() {
            value.to_bits()
        } else {
            0
        }
    })
}

fn pack_scalar_or_vector(kind: SkyUniformType, value: Option<&Value>) -> [u32; 4] {
    use SkyUniformType::*;
    match (kind, value) {
        (Bool, Some(Value::Bool(value))) => [u32::from(*value), 0, 0, 0],
        (Int, Some(Value::Int(value))) => {
            let value = (*value).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
            [value as u32, 0, 0, 0]
        }
        (Uint, Some(Value::Int(value))) => [(*value).clamp(0, i64::from(u32::MAX)) as u32, 0, 0, 0],
        (Float, Some(Value::Float(value))) => float_words([*value, 0.0, 0.0, 0.0]),
        (Float, Some(Value::Int(value))) => float_words([*value as f64, 0.0, 0.0, 0.0]),
        (Float, Some(Value::Length(value))) => float_words([value.0, 0.0, 0.0, 0.0]),
        (Float, Some(Value::Percent(value))) => float_words([value.0, 0.0, 0.0, 0.0]),
        (Vec2, Some(Value::Point2D(value))) => float_words([value.x.0, value.y.0, 0.0, 0.0]),
        (Vec2, Some(Value::Vector2D(value))) => float_words([value.x.0, value.y.0, 0.0, 0.0]),
        (Vec3, Some(Value::Point3D(value))) => float_words([value.x, value.y, value.z, 0.0]),
        (Vec3, Some(Value::Vector3D(value))) => float_words([value.x, value.y, value.z, 0.0]),
        (Vec4, Some(Value::Color(value))) => {
            let [r, g, b] = linear_color(*value);
            let alpha = f32::from(value.0.to_be_bytes()[3]) / 255.0;
            [r, g, b, alpha].map(f32::to_bits)
        }
        (Vec4, Some(Value::Quaternion3D(value))) => float_words(value.components()),
        _ => [0; 4],
    }
}

fn uniform_source(fields: &[SkyUniformField]) -> String {
    if fields.is_empty() {
        return String::new();
    }
    let size: usize = fields.iter().map(|field| field.kind.slots()).sum();
    let mut source = format!(
        "@group(0) @binding(2) var<uniform> xr_sky_uniforms: array<vec4<u32>, {size}>;\n\
        fn sky_uniforms() -> SkyUniforms {{ return SkyUniforms(\n"
    );
    let mut slot = 0;
    for field in fields {
        use SkyUniformType::*;
        let field_source = match field.kind {
            Bool => format!("xr_sky_uniforms[{slot}].x != 0u"),
            Int => format!("bitcast<i32>(xr_sky_uniforms[{slot}].x)"),
            Uint => format!("xr_sky_uniforms[{slot}].x"),
            Float => format!("bitcast<f32>(xr_sky_uniforms[{slot}].x)"),
            Vec2 => format!("bitcast<vec2<f32>>(xr_sky_uniforms[{slot}].xy)"),
            Vec3 => format!("bitcast<vec3<f32>>(xr_sky_uniforms[{slot}].xyz)"),
            Vec4 => format!("bitcast<vec4<f32>>(xr_sky_uniforms[{slot}])"),
            Mat3 => {
                let columns = (slot..slot + 3)
                    .map(|index| format!("bitcast<vec3<f32>>(xr_sky_uniforms[{index}].xyz)"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("mat3x3<f32>({columns})")
            }
        };
        let _ = writeln!(source, "    {field_source},");
        slot += field.kind.slots();
    }
    source.push_str("); }\n");
    source
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Length, Point2D, Quaternion3D, Transform2D, Vector3D};

    #[test]
    fn named_uniforms_pack_booleans_colors_and_padded_matrix_columns() {
        use SkyUniformType::*;
        let shader = SkyShader::new(
            "".into(),
            [
                ("enabled", Bool),
                ("signed", Int),
                ("unsigned", Uint),
                ("sun_gain", Float),
                ("offset", Vec2),
                ("direction", Vec3),
                ("tint", Vec4),
                ("rotation", Vec4),
                ("transform", Mat3),
                ("absent", Float),
            ]
            .into_iter()
            .map(|(name, kind)| SkyUniformField {
                name: name.into(),
                kind,
            })
            .collect(),
        );
        let inputs = RecordValue::new(
            "SkySettings",
            [
                ("enabled", Value::Bool(true)),
                ("signed", Value::Int(-29)),
                ("unsigned", Value::Int(i64::MAX)),
                ("sun-gain", Value::Float(1.25)),
                (
                    "offset",
                    Value::Point2D(Point2D::new(Length(2.0), Length(4.0))),
                ),
                ("direction", Value::Vector3D(Vector3D::new(1.0, -2.0, 3.0))),
                ("tint", Value::Color(Color(0x80808080))),
                ("rotation", Value::Quaternion3D(Quaternion3D::default())),
                (
                    "transform",
                    Value::Transform2D(Transform2D::from_coefficients([
                        2.0, 3.0, 4.0, 5.0, 6.0, 7.0,
                    ])),
                ),
            ]
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect(),
        );
        let words = shader.pack_uniforms(Some(&inputs));
        let floats = |index: usize| words[index].map(f32::from_bits);
        assert_eq!(words.len(), 12);
        assert_eq!(words[0], [1, 0, 0, 0]);
        assert_eq!(words[1][0] as i32, -29);
        assert_eq!(words[2][0], u32::MAX);
        assert_eq!(floats(3), [1.25, 0.0, 0.0, 0.0]);
        assert_eq!(floats(4), [2.0, 4.0, 0.0, 0.0]);
        assert_eq!(floats(5), [1.0, -2.0, 3.0, 0.0]);
        assert!((floats(6)[0] - 0.21586).abs() < 0.0001);
        assert_eq!(floats(6)[3], 128.0 / 255.0);
        assert_eq!(floats(7), [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(floats(8), [2.0, 3.0, 0.0, 0.0]);
        assert_eq!(floats(9), [4.0, 5.0, 0.0, 0.0]);
        assert_eq!(floats(10), [6.0, 7.0, 1.0, 0.0]);
        assert_eq!(words[11], [0; 4]);
        assert!(shader.uniform_interface.contains("array<vec4<u32>, 12>"));
    }
}
