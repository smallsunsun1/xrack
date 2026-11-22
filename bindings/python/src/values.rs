//! Python representations of public interpreter values.

use pyo3::{
    PyTypeInfo,
    exceptions::{PyIndexError, PyKeyError, PyTypeError, PyValueError},
    inspect::PyStaticExpr,
    prelude::*,
    type_hint_identifier, type_hint_subscript,
    types::{PyBool, PyFloat, PyInt, PyList, PyString},
};
use std::{collections::BTreeMap, path::PathBuf};
use xrack_interpreter::{HostValue, ValueType};
use xrack_runtime as rt;

pub(super) struct PythonValue(pub(super) HostValue);

impl PythonValue {
    pub(super) fn into_for(self, ty: &ValueType) -> PyResult<HostValue> {
        let value = match (ty, self.0) {
            (ValueType::Float, HostValue::Int(value)) => HostValue::Float(value as f64),
            (ValueType::IntList, HostValue::StringList(value)) if value.is_empty() => {
                HostValue::IntList(Vec::new())
            }
            (ValueType::DateList, HostValue::StringList(value)) if value.is_empty() => {
                HostValue::DateList(Vec::new())
            }
            (_, value) => value,
        };
        if ty.matches(&value) {
            Ok(value)
        } else {
            Err(PyTypeError::new_err(format!("expected XRack {ty:?}")))
        }
    }
}

impl<'py> FromPyObject<'_, 'py> for PythonValue {
    type Error = PyErr;
    const INPUT_TYPE: PyStaticExpr = VALUE_TYPE;

    fn extract(value: Borrowed<'_, 'py, PyAny>) -> PyResult<Self> {
        decode(&value).map(Self)
    }
}

impl<'py> IntoPyObject<'py> for PythonValue {
    type Target = PyAny;
    type Output = Bound<'py, PyAny>;
    type Error = PyErr;
    const OUTPUT_TYPE: PyStaticExpr = VALUE_TYPE;

    fn into_pyobject(self, py: Python<'py>) -> PyResult<Self::Output> {
        encode(py, self.0)
    }
}

macro_rules! owned_value {
    ($name:ident, $runtime:ty) => {
        #[pyclass(frozen, eq, from_py_object, module = "xrack._native")]
        #[derive(Clone, PartialEq)]
        struct $name($runtime);

        impl From<$name> for $runtime {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl From<$runtime> for $name {
            fn from(value: $runtime) -> Self {
                Self(value)
            }
        }
    };
}

macro_rules! struct_value {
    ($name:ident { $($field:ident: $ty:ty),+ $(,)? }) => {
        owned_value!($name, rt::$name);

        #[pymethods]
        impl $name {
            #[new]
            fn new($($field: $ty),+) -> Self {
                Self(rt::$name { $($field: $field.into()),+ })
            }

            $(
                #[getter]
                fn $field(&self) -> $ty {
                    self.0.$field.to_owned().into()
                }
            )+
        }
    };
}

struct_value!(Font {
    family: String,
    size: Length,
    weight: i64,
    italic: bool
});
struct_value!(Point2D {
    x: Length,
    y: Length
});
struct_value!(Vector2D {
    x: Length,
    y: Length
});
owned_value!(Transform2D, rt::Transform2D);

#[pymethods]
impl Transform2D {
    #[new]
    fn new() -> Self {
        Self(rt::Transform2D::IDENTITY)
    }

    #[staticmethod]
    fn translation(x: Length, y: Length) -> Self {
        Self(rt::Transform2D::translation(x.0, y.0))
    }

    #[staticmethod]
    fn rotation(degrees: f64) -> Self {
        Self(rt::Transform2D::rotation(degrees))
    }

    #[staticmethod]
    fn scale(x: f64, y: f64) -> Self {
        Self(rt::Transform2D::scale(x, y))
    }

    fn then(&self, next: Self) -> Self {
        Self(self.0.then(next.0))
    }

    fn around(&self, point: Point2D) -> Self {
        Self(self.0.around(point.0))
    }

    fn map_point(&self, point: Point2D) -> Point2D {
        Point2D(self.0.map_point(point.0))
    }

    fn map_vector(&self, vector: Vector2D) -> Vector2D {
        Vector2D(self.0.map_vector(vector.0))
    }
}

struct_value!(KeyModifiers {
    control: bool,
    shift: bool,
    alt: bool,
    meta: bool,
    primary: bool
});
struct_value!(KeyEvent {
    key: String,
    text: String,
    modifiers: KeyModifiers,
    repeat: bool
});
struct_value!(WheelEvent {
    x: Length,
    y: Length,
    delta_x: Length,
    delta_y: Length,
    lines_x: f64,
    lines_y: f64,
    is_line: bool,
    modifiers: KeyModifiers,
});
struct_value!(PointerEvent {
    kind: PointerEventKind,
    button: PointerButton,
    x: Length,
    y: Length,
    inside: bool,
    is_touch: bool,
    touch_finger_id: i64,
    modifiers: KeyModifiers,
});
struct_value!(TouchPoint {
    id: i64,
    x: Length,
    y: Length,
    start_x: Length,
    start_y: Length,
    inside: bool,
});

owned_value!(Length, rt::Length);

#[pymethods]
impl Length {
    #[new]
    fn new(px: f64) -> Self {
        Self(rt::Length(px))
    }

    #[getter]
    fn px(&self) -> f64 {
        self.0.0
    }
}

owned_value!(Percent, rt::Percent);

#[pymethods]
impl Percent {
    #[new]
    fn new(percent: f64) -> Self {
        Self(rt::Percent(percent / 100.0))
    }

    #[getter]
    fn percent(&self) -> f64 {
        self.0.0 * 100.0
    }
}

owned_value!(Color, rt::Color);

#[pymethods]
impl Color {
    #[new]
    #[pyo3(signature = (red, green, blue, alpha = 255))]
    fn new(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self(rt::Color(u32::from_be_bytes([red, green, blue, alpha])))
    }

    #[staticmethod]
    fn from_hex(value: &str) -> PyResult<Self> {
        rt::Color::parse_hex_literal(value)
            .map(Self)
            .ok_or_else(|| PyValueError::new_err("expected an XRack hex color"))
    }

    #[getter]
    fn rgba(&self) -> (u8, u8, u8, u8) {
        let [red, green, blue, alpha] = self.0.0.to_be_bytes();
        (red, green, blue, alpha)
    }

    fn __str__(&self) -> String {
        format!("#{:08x}", self.0.0)
    }
}

struct_value!(GradientStop {
    color: Color,
    offset: f64
});
owned_value!(Brush, rt::Brush);

#[pymethods]
impl Brush {
    #[new]
    fn new(color: Color) -> Self {
        Self(rt::Brush::Solid(color.0))
    }

    #[staticmethod]
    fn linear(angle: f64, stops: Vec<PyRef<'_, GradientStop>>) -> PyResult<Self> {
        if !angle.is_finite() {
            return Err(PyValueError::new_err("gradient angle must be finite"));
        }
        Ok(Self(rt::Brush::Linear {
            angle_degrees: angle,
            stops: gradient_stops(stops)?,
        }))
    }

    #[staticmethod]
    fn radial(stops: Vec<PyRef<'_, GradientStop>>) -> PyResult<Self> {
        Ok(Self(rt::Brush::Radial {
            stops: gradient_stops(stops)?,
        }))
    }

    #[getter]
    fn kind(&self) -> &'static str {
        match self.0 {
            rt::Brush::Solid(_) => "solid",
            rt::Brush::Linear { .. } => "linear",
            rt::Brush::Radial { .. } => "radial",
        }
    }

    #[getter]
    fn color(&self) -> Option<Color> {
        match self.0 {
            rt::Brush::Solid(color) => Some(Color(color)),
            _ => None,
        }
    }

    #[getter]
    fn angle(&self) -> Option<f64> {
        match self.0 {
            rt::Brush::Linear { angle_degrees, .. } => Some(angle_degrees),
            _ => None,
        }
    }

    #[getter]
    fn stops(&self) -> Vec<GradientStop> {
        match &self.0 {
            rt::Brush::Solid(_) => Vec::new(),
            rt::Brush::Linear { stops, .. } | rt::Brush::Radial { stops } => {
                stops.iter().copied().map(GradientStop).collect()
            }
        }
    }
}

fn gradient_stops(
    stops: Vec<PyRef<'_, GradientStop>>,
) -> PyResult<std::sync::Arc<[rt::GradientStop]>> {
    let stops: Vec<_> = stops.iter().map(|stop| stop.0).collect();
    if stops.len() < 2
        || stops.iter().any(|stop| !(0.0..=1.0).contains(&stop.offset))
        || stops.windows(2).any(|pair| pair[0].offset > pair[1].offset)
    {
        return Err(PyValueError::new_err(
            "gradient requires two or more ordered stops in [0, 1]",
        ));
    }
    Ok(stops.into())
}

owned_value!(Date, rt::Date);
owned_value!(Time, rt::Time);

#[pymethods]
impl Date {
    #[new]
    fn new(year: i64, month: i64, day: i64) -> PyResult<Self> {
        rt::Date::new(year, month, day)
            .map(Self)
            .ok_or_else(|| PyValueError::new_err("invalid date"))
    }

    #[getter]
    fn year(&self) -> i64 {
        self.0.year
    }
    #[getter]
    fn month(&self) -> i64 {
        self.0.month
    }
    #[getter]
    fn day(&self) -> i64 {
        self.0.day
    }
    fn __str__(&self) -> String {
        self.0.to_string()
    }
}

#[pymethods]
impl Time {
    #[new]
    #[pyo3(signature = (hour, minute, second = 0))]
    fn new(hour: i64, minute: i64, second: i64) -> PyResult<Self> {
        rt::Time::new(hour, minute, second)
            .map(Self)
            .ok_or_else(|| PyValueError::new_err("invalid time"))
    }

    #[getter]
    fn hour(&self) -> i64 {
        self.0.hour
    }
    #[getter]
    fn minute(&self) -> i64 {
        self.0.minute
    }
    #[getter]
    fn second(&self) -> i64 {
        self.0.second
    }
    fn __str__(&self) -> String {
        self.0.to_string()
    }
}

owned_value!(StyledText, rt::StyledText);

#[pymethods]
impl StyledText {
    #[new]
    fn new(text: String) -> Self {
        Self(rt::StyledText::from_plain_text(text))
    }

    #[staticmethod]
    fn from_markdown(source: &str) -> PyResult<Self> {
        rt::StyledText::from_markdown(source)
            .map(Self)
            .map_err(as_value_error)
    }

    #[getter]
    fn text(&self) -> &str {
        self.0.text()
    }

    #[getter]
    fn links(&self) -> Vec<(usize, usize, String)> {
        self.0
            .links()
            .iter()
            .map(|link| (link.start, link.end, link.href.to_string()))
            .collect()
    }
}

#[pyclass(frozen, eq, unsendable, skip_from_py_object, module = "xrack._native")]
#[derive(Clone, PartialEq)]
struct Image(rt::ImageAsset);

#[pymethods]
impl Image {
    #[new]
    fn new() -> Self {
        Self(rt::ImageAsset::default())
    }

    #[staticmethod]
    fn from_file(path: PathBuf) -> PyResult<Self> {
        let format = match path.extension().and_then(|ext| ext.to_str()) {
            Some(ext) if ext.eq_ignore_ascii_case("png") => rt::ImageFormat::Png,
            Some(ext) if ext.eq_ignore_ascii_case("svg") => rt::ImageFormat::Svg,
            _ => return Err(PyValueError::new_err("expected a PNG or SVG file")),
        };
        let bytes = std::fs::read(path)?;
        let size = xrack_assets::image_size(format, &bytes).map_err(as_value_error)?;
        Ok(Self(rt::ImageAsset::Loaded(rt::LoadedImage::new(
            format, bytes, size,
        ))))
    }

    #[getter]
    fn size(&self) -> (u32, u32) {
        let [width, height] = self.0.size();
        (width, height)
    }

    #[getter]
    fn frame_count(&self) -> usize {
        self.0.frame_count()
    }
}

fn as_value_error(error: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(error.to_string())
}

owned_value!(PointerButtons, rt::PointerButtons);

#[pymethods]
impl PointerButtons {
    #[new]
    fn new(buttons: Vec<PointerButton>) -> Self {
        Self(buttons.into_iter().map(rt::PointerButton::from).collect())
    }

    fn __contains__(&self, button: PointerButton) -> bool {
        self.0.contains(button.into())
    }
}

#[pyclass(frozen, eq, unsendable, skip_from_py_object, module = "xrack._native")]
#[derive(Clone, PartialEq)]
struct TouchPoints(rt::TouchPoints);

#[pymethods]
impl TouchPoints {
    #[new]
    fn new(points: Vec<PyRef<'_, TouchPoint>>) -> Self {
        Self(
            points
                .iter()
                .map(|point| point.0)
                .collect::<Vec<_>>()
                .into(),
        )
    }

    fn snapshot(&self) -> Vec<TouchPoint> {
        self.0.snapshot().into_iter().map(TouchPoint).collect()
    }
}

#[pyclass(frozen, eq, unsendable, skip_from_py_object, module = "xrack._native")]
#[derive(Clone, PartialEq)]
struct Record(rt::RecordValue);

#[pymethods]
impl Record {
    #[new]
    fn new(name: String, fields: BTreeMap<String, PythonValue>) -> Self {
        Self(rt::RecordValue::new(
            name,
            fields
                .into_iter()
                .map(|(name, value)| (name, value.0))
                .collect(),
        ))
    }

    #[getter]
    fn name(&self) -> &str {
        self.0.name()
    }

    fn __getitem__(&self, field: &str) -> PyResult<PythonValue> {
        self.0
            .field(field)
            .cloned()
            .map(PythonValue)
            .ok_or_else(|| PyKeyError::new_err(field.to_owned()))
    }

    fn fields(&self) -> BTreeMap<String, PythonValue> {
        self.0
            .fields()
            .map(|(name, value)| (name.to_owned(), PythonValue(value.clone())))
            .collect()
    }
}

#[pyclass(frozen, unsendable, skip_from_py_object, module = "xrack._native")]
#[derive(Clone)]
struct RecordList(rt::RecordList);

type ValidateRow = dyn Fn(&rt::RecordValue) -> PyResult<()>;

#[pyclass(frozen, unsendable, module = "xrack._native")]
pub(super) struct RecordModel {
    model: rt::RecordList,
    validate: Box<ValidateRow>,
}

impl RecordModel {
    pub(super) fn new(
        model: rt::RecordList,
        validate: impl Fn(&rt::RecordValue) -> PyResult<()> + 'static,
    ) -> Self {
        Self {
            model,
            validate: Box::new(validate),
        }
    }
}

#[pymethods]
impl RecordModel {
    fn __len__(&self) -> usize {
        self.model.model().len()
    }

    fn __getitem__(&self, index: i64) -> PyResult<Record> {
        let model = self.model.model();
        model
            .row_at(row_index(model, index)? as i64)
            .map(Record)
            .ok_or_else(row_error)
    }

    fn __setitem__(&self, index: i64, row: PyRef<'_, Record>) -> PyResult<()> {
        let model = self.model.model();
        let index = row_index(model, index)?;
        (self.validate)(&row.0)?;
        model.set_row(index, row.0.clone());
        Ok(())
    }

    fn __delitem__(&self, index: i64) -> PyResult<()> {
        let model = self.model.model();
        model.remove(row_index(model, index)?);
        Ok(())
    }

    fn append(&self, row: PyRef<'_, Record>) -> PyResult<()> {
        (self.validate)(&row.0)?;
        self.model.model().push(row.0.clone());
        Ok(())
    }
}

#[pymethods]
impl RecordList {
    #[new]
    fn new(name: String, rows: Vec<PyRef<'_, Record>>) -> PyResult<Self> {
        if rows.iter().any(|row| row.0.name() != name) {
            return Err(PyTypeError::new_err(format!(
                "all rows must be records of type {name}"
            )));
        }
        Ok(Self(rt::RecordList::new(
            name,
            rows.iter().map(|row| row.0.clone()).collect(),
        )))
    }

    #[getter]
    fn name(&self) -> &str {
        self.0.name()
    }

    fn __len__(&self) -> usize {
        self.0.model().len()
    }

    fn __getitem__(&self, index: i64) -> PyResult<Record> {
        let index = row_index(self.0.model(), index)?;
        self.0
            .model()
            .row_at(index as i64)
            .map(Record)
            .ok_or_else(row_error)
    }
}

#[pyclass(frozen, unsendable, skip_from_py_object, module = "xrack._native")]
#[derive(Clone)]
pub(super) struct StringModel(pub(super) rt::VecModel<String>);

#[pymethods]
impl StringModel {
    #[new]
    fn new(rows: Vec<String>) -> Self {
        Self(rows.into())
    }

    fn __len__(&self) -> usize {
        self.0.len()
    }

    fn __getitem__(&self, index: i64) -> PyResult<String> {
        let index = row_index(&self.0, index)?;
        self.0.row_at(index as i64).ok_or_else(row_error)
    }

    fn __setitem__(&self, index: i64, value: String) -> PyResult<()> {
        let index = row_index(&self.0, index)?;
        self.0.set_row(index, value);
        Ok(())
    }

    fn __delitem__(&self, index: i64) -> PyResult<()> {
        let index = row_index(&self.0, index)?;
        self.0.remove(index);
        Ok(())
    }

    fn append(&self, value: String) {
        self.0.push(value);
    }

    fn snapshot(&self) -> Vec<String> {
        self.0.snapshot()
    }
}

fn row_index<T: Clone + PartialEq + 'static>(
    model: &rt::VecModel<T>,
    index: i64,
) -> PyResult<usize> {
    let len = model.len() as i64;
    let index = if index < 0 { len + index } else { index };
    if (0..len).contains(&index) {
        Ok(index as usize)
    } else {
        Err(row_error())
    }
}

fn row_error() -> PyErr {
    PyIndexError::new_err("XRack model index out of range")
}

macro_rules! bridge_values {
    (
        structs { $($tag:ident => $wrapper:ident),+ $(,)? }
        enums { $($name:ident { $($(#[$meta:meta])* $variant:ident),+ $(,)? }),+ $(,)? }
    ) => {
        $(
            #[pyclass(eq, frozen, from_py_object, module = "xrack._native")]
            #[derive(Clone, Copy, PartialEq)]
            enum $name { $($(#[$meta])* $variant),+ }

            impl From<$name> for rt::$name {
                fn from(value: $name) -> Self {
                    match value { $($name::$variant => Self::$variant),+ }
                }
            }

            impl From<rt::$name> for $name {
                fn from(value: rt::$name) -> Self {
                    match value { $(rt::$name::$variant => Self::$variant),+ }
                }
            }
        )+

        const VALUE_TYPE: PyStaticExpr = type_hint_subscript!(
            type_hint_identifier!("typing", "Union"),
            PyBool::TYPE_HINT,
            PyInt::TYPE_HINT,
            PyFloat::TYPE_HINT,
            PyString::TYPE_HINT,
            type_hint_subscript!(PyList::TYPE_HINT, PyString::TYPE_HINT),
            type_hint_subscript!(PyList::TYPE_HINT, PyInt::TYPE_HINT),
            type_hint_subscript!(PyList::TYPE_HINT, Date::TYPE_HINT),
            $($wrapper::TYPE_HINT,)+
            $($name::TYPE_HINT),+
        );

        fn decode(value: &Bound<'_, PyAny>) -> PyResult<HostValue> {
            if value.is_instance_of::<PyBool>() {
                return Ok(HostValue::Bool(value.extract()?));
            }
            if value.is_instance_of::<PyInt>() {
                return Ok(HostValue::Int(value.extract()?));
            }
            if value.is_instance_of::<PyFloat>() {
                return Ok(HostValue::Float(value.extract()?));
            }
            if value.is_instance_of::<PyString>() {
                return Ok(HostValue::String(value.extract()?));
            }
            if value.is_instance_of::<PyList>() {
                let list = value.cast::<PyList>()?;
                if !list.is_empty() && list.iter().all(|item| {
                    item.is_instance_of::<PyInt>() && !item.is_instance_of::<PyBool>()
                }) {
                    return Ok(HostValue::IntList(value.extract()?));
                }
                if !list.is_empty() && list.iter().all(|item| item.is_instance_of::<Date>()) {
                    let dates = list.iter().map(|item| Ok(item.extract::<PyRef<'_, Date>>()?.0))
                        .collect::<PyResult<Vec<_>>>()?;
                    return Ok(HostValue::DateList(dates));
                }
                return Ok(HostValue::StringList(value.extract()?));
            }
            $(
                if value.is_instance_of::<$wrapper>() {
                    return Ok(HostValue::$tag(value.extract::<PyRef<'_, $wrapper>>()?.0.clone()));
                }
            )+
            $(
                if value.is_instance_of::<$name>() {
                    return Ok(HostValue::$name(value.extract::<$name>()?.into()));
                }
            )+
            Err(PyTypeError::new_err("expected a Python XRack value"))
        }

        fn encode(py: Python<'_>, value: HostValue) -> PyResult<Bound<'_, PyAny>> {
            macro_rules! convert {
                ($value:expr) => {
                    Ok($value.into_pyobject(py)?.into_any())
                };
            }
            match value {
                HostValue::Bool(value) => Ok(value.into_pyobject(py)?.to_owned().into_any()),
                HostValue::Int(value) => convert!(value),
                HostValue::Float(value) => convert!(value),
                HostValue::String(value) => convert!(value),
                HostValue::StringList(value) => convert!(value),
                HostValue::IntList(value) => convert!(value),
                HostValue::IntModel(model) => convert!(model.into_snapshot()),
                HostValue::DateList(dates) => convert!(dates.into_iter().map(Date).collect::<Vec<_>>()),
                HostValue::DateModel(model) => convert!(model.into_snapshot().into_iter().map(Date).collect::<Vec<_>>()),
                HostValue::Image(value) => convert!(Image(value.into())),
                // The combo projection is renderer data and has no public ValueType.
                HostValue::ComboModel(_) => Err(PyTypeError::new_err("invalid public XRack combo projection")),
                $(HostValue::$tag(value) => convert!($wrapper(value)),)+
                $(HostValue::$name(value) => convert!($name::from(value)),)+
            }
        }

        #[pymodule]
        mod _native {
            #[pymodule_export]
            use super::{GradientStop, RecordModel, $($wrapper,)+ $($name),+};
            #[pymodule_export]
            use crate::{PyComponent, PyProgram, call_on_ui, load_file, load_mlir_file, post};
        }
    };
}

bridge_values! {
    structs {
        Brush => Brush,
        Color => Color,
        Date => Date,
        Font => Font,
        ImageAsset => Image,
        KeyEvent => KeyEvent,
        KeyModifiers => KeyModifiers,
        Length => Length,
        Percent => Percent,
        Point2D => Point2D,
        PointerButtons => PointerButtons,
        PointerEvent => PointerEvent,
        Record => Record,
        RecordList => RecordList,
        StringModel => StringModel,
        StyledText => StyledText,
        Time => Time,
        TouchPoint => TouchPoint,
        TouchPoints => TouchPoints,
        Transform2D => Transform2D,
        Vector2D => Vector2D,
        WheelEvent => WheelEvent,
    }
    enums {
        AccessibleLive { Off, Polite, Assertive },
        ButtonDisplay { IconOnly, TextOnly, TextBesideIcon, TextUnderIcon },
        CheckState { Unchecked, PartiallyChecked, Checked },
        ColorScheme { System, Light, Dark },
        CrossAxisAlignment { Auto, Stretch, Start, End, Center },
        CursorShape { Auto, Default, Pointer, Text, Crosshair, Move, Grab, Grabbing, NotAllowed, Wait, Help, ResizeHorizontal, ResizeVertical, ZoomIn, ZoomOut },
        DialInputMode { Circular, Horizontal, Vertical },
        DialogRole { Accept, Reject },
        DragAction { #[pyo3(name = "None_")] None, Copy, Move, Link },
        DrawerEdge { Left, Right, Top, Bottom },
        EventResult { Accept, Reject },
        FileDialogMode { OpenFile, OpenFiles, SaveFile, SelectFolder, SelectFolders },
        FlexboxLayoutDirection { Row, RowReverse, Column, ColumnReverse },
        FlexboxLayoutWrap { Wrap, NoWrap, WrapReverse },
        FlickBoundsBehavior { StopAtBounds, DragOverBounds, OvershootBounds, DragAndOvershootBounds },
        FlickBoundsMovement { FollowBoundsBehavior, StopAtBounds },
        FlickDirection { Both, Horizontal, Vertical },
        FocusReason { Programmatic, TabNavigation, PointerClick, PopupActivation, WindowActivation, Accessibility, LayoutChange, KeyboardNavigation },
        ImageFit { Fill, Contain, Cover, Preserve },
        ImageHorizontalAlignment { Left, Center, Right },
        ImageRendering { Smooth, Pixelated },
        ImageTileMode { Stretch, Repeat, Round },
        ImageVerticalAlignment { Top, Center, Bottom },
        InputType { Text, Password, Number, Decimal, Search, Email, Url, Tel },
        InputPurpose { Auto, Username, CurrentPassword, NewPassword, OneTimeCode },
        KeyboardCapitalization { Auto, #[pyo3(name = "None_")] None, Words, Sentences, Characters },
        ReturnKey { Auto, Next, Done },
        LayoutAlignment { Stretch, Center, Start, End, SpaceBetween, SpaceAround, SpaceEvenly },
        LayoutDirection { LeftToRight, RightToLeft },
        LogLevel { Trace, Debug, Info, Warn, Error },
        MessageButtons { Ok, OkCancel, YesNo, YesNoCancel },
        MessageLevel { Info, Warning, Error },
        MouseSelectionMode { Characters, Words, Caret },
        InputValidity { Invalid, Intermediate, Acceptable },
        Orientation { Horizontal, Vertical },
        PointerButton { Primary, Secondary, Middle },
        PointerEventKind { Down, Move, Up, Cancel },
        PopupPlacement { Below, Above, Left, Right, Offset },
        ScrollAlignment { Beginning, Center, End, Visible, Contain },
        ScrollBarPolicy { AsNeeded, AlwaysOn, AlwaysOff },
        ScrollSnapMode { #[pyo3(name = "None_")] None, Always, OnRelease },
        SelectionTarget { Rows, Cells, Columns },
        SelectionGesture { Auto, Drag, PressAndHold },
        StandardButtonKind { Ok, Open, Save, SaveAll, Cancel, Close, Apply, Reset, RestoreDefaults, Help, Yes, YesToAll, No, NoToAll, Abort, Retry, Ignore, Discard },
        SwipeSide { Closed, Left, Right },
        TextHorizontalAlignment { Start, End, Left, Center, Right },
        TextOverflow { Clip, Elide },
        TextVerticalAlignment { Top, Center, Bottom },
        TextWrap { WordWrap, NoWrap, CharWrap },
    }
}
