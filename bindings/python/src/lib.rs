mod values;

use pyo3::{
    exceptions::{PyAttributeError, PyRuntimeError, PyTypeError, PyValueError},
    inspect::PyStaticExpr,
    prelude::*,
    type_hint_identifier, type_hint_subscript,
    types::PyTuple,
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    convert::Infallible,
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};
use values::{PythonValue, RecordModel, StringModel};
use xrack_interpreter::{
    HostValue, Program, ReloadableInstance, ValueType,
    live::{LiveComponent, LiveUi, PreparedUi, imported_sources, source_text},
};
use xrack_platform_winit::DesktopWindow;

thread_local! {
    // The handoff stays on the UI thread while Python releases the GIL.
    static PENDING_RUN: RefCell<Option<ReloadableInstance>> = const { RefCell::new(None) };
    static WINDOW: RefCell<Option<(u64, DesktopWindow)>> = const { RefCell::new(None) };
}

static NEXT_COMPONENT_ID: AtomicU64 = AtomicU64::new(0);

struct PythonCallback(Py<PyAny>);

impl<'py> FromPyObject<'_, 'py> for PythonCallback {
    type Error = PyErr;
    const INPUT_TYPE: PyStaticExpr = type_hint_subscript!(
        type_hint_identifier!("collections.abc", "Callable"),
        PyStaticExpr::List { elts: &[] },
        type_hint_identifier!("builtins", "object")
    );

    fn extract(value: Borrowed<'_, 'py, PyAny>) -> PyResult<Self> {
        if !value.is_callable() {
            return Err(PyTypeError::new_err("expected a Python callable"));
        }
        Ok(Self(value.to_owned().unbind()))
    }
}

struct PythonFuture<'py>(Bound<'py, PyAny>);

impl<'py> IntoPyObject<'py> for PythonFuture<'py> {
    type Target = PyAny;
    type Output = Bound<'py, PyAny>;
    type Error = Infallible;
    const OUTPUT_TYPE: PyStaticExpr = type_hint_subscript!(
        type_hint_identifier!("collections.abc", "Awaitable"),
        type_hint_identifier!("typing", "Any")
    );

    fn into_pyobject(self, _py: Python<'py>) -> Result<Self::Output, Self::Error> {
        Ok(self.0)
    }
}

#[pyclass(name = "Program", unsendable)]
struct PyProgram(Program, Option<(PathBuf, PathBuf)>);

struct LiveBinding;
impl LiveUi for LiveBinding {
    fn from_live(_component: LiveComponent<Self>) -> Self {
        Self
    }
}

#[pymethods]
impl PyProgram {
    fn components(&self) -> Vec<String> {
        self.0.exported_components().map(str::to_owned).collect()
    }

    fn create(&self, name: &str) -> PyResult<PyComponent> {
        if !self.0.exported_components().any(|export| export == name) {
            return Err(PyValueError::new_err(format!(
                "unknown exported XRack component {name:?}"
            )));
        }
        let (instance, live) = if let Some((source, project)) = &self.1 {
            let filename = source
                .to_str()
                .ok_or_else(|| PyValueError::new_err("XRack paths must be UTF-8"))?;
            let live = LiveComponent::open_with(filename, name, Some(project), |source, saved| {
                let filename = source.to_str().ok_or("XRack paths must be UTF-8")?;
                let prepared = xrack_compiler::prepare_preview(
                    source_text(source, saved)?,
                    filename,
                    imported_sources(source, saved),
                )
                .map_err(|error| error.to_string())?;
                Ok(PreparedUi {
                    mlir: prepared.module.to_mlir(),
                    inputs: prepared.inputs,
                })
            })
            .map_err(runtime_error)?;
            (live.component(), Some(live))
        } else {
            (
                ReloadableInstance::new(self.0.create(name).map_err(runtime_error)?),
                None,
            )
        };
        Ok(PyComponent {
            instance,
            _live: live,
            id: NEXT_COMPONENT_ID.fetch_add(1, Ordering::Relaxed),
        })
    }
}

#[pyclass(name = "Component", unsendable)]
struct PyComponent {
    instance: ReloadableInstance,
    _live: Option<LiveComponent<LiveBinding>>,
    id: u64,
}

#[pymethods]
impl PyComponent {
    fn properties(&self) -> Vec<(String, String, bool)> {
        self.instance
            .current()
            .properties()
            .map(|(name, ty, writable)| (name.to_owned(), type_name(ty), writable))
            .collect()
    }

    fn callbacks(&self) -> Vec<(String, Vec<String>, Option<String>)> {
        self.instance
            .current()
            .callbacks()
            .map(|(name, values, result)| {
                (
                    name.to_owned(),
                    values.iter().cloned().map(type_name).collect(),
                    result.map(type_name),
                )
            })
            .collect()
    }

    fn record_model(&self, name: &str) -> PyResult<RecordModel> {
        self.writable_type(name)?;
        let model = self
            .instance
            .current()
            .record_model(name)
            .map_err(runtime_error)?;
        let instance = self.instance.as_weak();
        let property = name.to_owned();
        Ok(RecordModel::new(model, move |row| {
            let instance = instance
                .upgrade()
                .ok_or_else(|| PyRuntimeError::new_err("XRack component was closed"))?;
            instance
                .current()
                .validate_record_row(&property, row)
                .map_err(runtime_error)
        }))
    }

    fn get(&self, name: &str) -> PyResult<PythonValue> {
        self.instance
            .current()
            .get_snapshot(name)
            .map(PythonValue)
            .map_err(runtime_error)
    }

    fn string_model(&self, name: &str) -> PyResult<StringModel> {
        self.writable_type(name)?;
        self.instance
            .current()
            .string_model(name)
            .map(StringModel)
            .map_err(runtime_error)
    }

    fn set(&self, name: &str, value: PythonValue) -> PyResult<()> {
        let ty = self.writable_type(name)?;
        self.instance
            .current()
            .set(name, value.into_for(&ty)?)
            .map_err(runtime_error)
    }

    fn reset(&self, name: &str) -> PyResult<()> {
        self.writable_type(name)?;
        self.instance.current().reset(name).map_err(runtime_error)
    }

    fn callback_returns_value(&self, name: &str) -> PyResult<bool> {
        Ok(self.signature(name)?.1.is_some())
    }
    fn on(&self, name: &str, handler_ref: PythonCallback) -> PyResult<()> {
        let result = self.signature(name)?.1;
        self.instance
            .on(
                name,
                Rc::new(move |arguments| {
                    Python::attach(|py| -> PyResult<Option<HostValue>> {
                        let handler = handler_ref.0.bind(py).call0()?;
                        if handler.is_none() {
                            return Err(PyRuntimeError::new_err(
                                "the Python callback was released",
                            ));
                        }
                        let args = PyTuple::new(py, arguments.iter().cloned().map(PythonValue))?;
                        let value = handler.call1(args)?;
                        match &result {
                            Some(ty) => Ok(Some(value.extract::<PythonValue>()?.into_for(ty)?)),
                            None if value.is_none() => Ok(None),
                            None => Err(PyTypeError::new_err(
                                "a void XRack callback must return None",
                            )),
                        }
                    })
                    .map_err(|error| error.to_string())
                }),
            )
            .map_err(runtime_error)
    }

    fn invoke(&self, name: &str, arguments: Vec<PythonValue>) -> PyResult<Option<PythonValue>> {
        let (types, _) = self.signature(name)?;
        if arguments.len() != types.len() {
            return Err(PyTypeError::new_err(format!(
                "{name} expects {} argument(s), received {}",
                types.len(),
                arguments.len(),
            )));
        }
        let arguments = arguments
            .into_iter()
            .zip(types)
            .map(|(value, ty)| value.into_for(&ty))
            .collect::<PyResult<Vec<_>>>()?;
        self.instance
            .current()
            .invoke(name, &arguments)
            .map(|value| value.map(PythonValue))
            .map_err(runtime_error)
    }

    fn run(&self, py: Python<'_>, ready: PythonCallback) -> PyResult<()> {
        if WINDOW.with_borrow(Option::is_some) {
            return Err(PyRuntimeError::new_err(
                "an XRack window is already running",
            ));
        }
        PENDING_RUN.set(Some(self.instance.clone()));
        let id = self.id;
        let result = py.detach(move || {
            let component = PENDING_RUN.take().expect("prepared XRack component");
            let result = xrack_platform_winit::run_desktop(|desktop| {
                WINDOW.set(Some((id, desktop.open(component)?)));
                Python::attach(|py| ready.0.call0(py).map_err(|error| error.to_string()))?;
                Ok(())
            });
            WINDOW.take();
            result
        });
        result.map_err(runtime_error)
    }

    fn close(&self) -> PyResult<()> {
        WINDOW
            .with_borrow(|window| {
                window
                    .as_ref()
                    .filter(|(id, _)| *id == self.id)
                    .map(|(_, window)| window)
                    .ok_or_else(|| "the XRack window is not running".to_owned())?
                    .close()
            })
            .map_err(runtime_error)
    }
}

impl PyComponent {
    fn writable_type(&self, name: &str) -> PyResult<ValueType> {
        let (_, ty, writable) = self
            .instance
            .current()
            .properties()
            .find(|(property, _, _)| *property == name)
            .ok_or_else(|| {
                PyAttributeError::new_err(format!("unknown public property {name:?}"))
            })?;
        if !writable {
            return Err(PyAttributeError::new_err(format!(
                "property {name:?} is read-only"
            )));
        }
        Ok(ty)
    }

    fn signature(&self, name: &str) -> PyResult<(Vec<ValueType>, Option<ValueType>)> {
        self.instance
            .current()
            .callbacks()
            .find(|(callback, _, _)| *callback == name)
            .map(|(_, arguments, result)| (arguments.to_vec(), result))
            .ok_or_else(|| PyAttributeError::new_err(format!("unknown callback {name:?}")))
    }
}

fn runtime_error(error: impl std::fmt::Display) -> PyErr {
    PyRuntimeError::new_err(error.to_string())
}

fn type_name(ty: ValueType) -> String {
    match ty {
        ValueType::Bool => "bool".into(),
        ValueType::Int => "int".into(),
        ValueType::Float => "float".into(),
        ValueType::String => "str".into(),
        ValueType::StringList => "list[str]".into(),
        ValueType::Record(_) => "xrack.Record".into(),
        ValueType::RecordList(_) => "xrack.RecordList".into(),
        _ => format!("xrack.{ty:?}"),
    }
}

#[pyfunction]
#[pyo3(signature = (path, live = false, project = None))]
fn load_file(path: PathBuf, live: bool, project: Option<PathBuf>) -> PyResult<PyProgram> {
    let path = path.canonicalize()?;
    let source = std::fs::read_to_string(&path)?;
    let mut prepared =
        xrack_compiler::prepare_preview(&source, &path.to_string_lossy(), BTreeMap::new())
            .map_err(|error| PyValueError::new_err(error.to_string()))?;
    let translations = prepared.take_translation_catalog();
    let (program, _) =
        Program::from_module_with_file_resources(&prepared.module, &path, translations)
            .map_err(runtime_error)?;
    let live = if live {
        let project = project
            .unwrap_or_else(|| path.parent().expect("canonical file").to_owned())
            .canonicalize()?;
        Some((path, project))
    } else {
        None
    };
    Ok(PyProgram(program, live))
}

#[pyfunction]
fn load_mlir_file(path: PathBuf) -> PyResult<PyProgram> {
    xrack_interpreter::load_mlir_file(path)
        .map(|program| PyProgram(program, None))
        .map_err(runtime_error)
}

#[pyfunction]
fn post(callback: PythonCallback) -> PyResult<()> {
    xrack_runtime::invoke_from_event_loop(move || {
        Python::attach(|py| {
            if let Err(error) = callback.0.call0(py) {
                error.write_unraisable(py, Some(callback.0.bind(py)));
            }
        });
    })
    .map_err(runtime_error)
}

#[pyfunction]
fn call_on_ui<'py>(py: Python<'py>, callback: PythonCallback) -> PyResult<PythonFuture<'py>> {
    use pyo3_async_runtimes::tokio::{future_into_py_with_locals, get_current_locals};

    let locals = get_current_locals(py)?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    xrack_runtime::invoke_from_event_loop(move || {
        if !sender.is_closed() {
            let _ = sender.send(Python::attach(|py| callback.0.call0(py)));
        }
    })
    .map_err(runtime_error)?;
    future_into_py_with_locals(py, locals, async move {
        receiver.await.map_err(|_| {
            PyRuntimeError::new_err("the XRack window closed before the callback ran")
        })?
    })
    .map(PythonFuture)
}
