//! Development-mode support shared by the Cargo and workspace build paths.

use crate::Program;
#[cfg(unix)]
use crate::live_attach as attach;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs, io,
    io::Write,
    marker::PhantomData,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
    sync::{Arc, Condvar, Mutex, Weak, mpsc},
    thread,
    time::Duration,
};
use xrack_runtime::{ComponentLifetime, WeakComponent};

#[doc(hidden)]
pub use crate::{__xr_forward_component_methods, ReloadableInstance};

#[doc(hidden)]
pub trait LiveUi: Sized + 'static {
    fn from_live(component: LiveComponent<Self>) -> Self;
}

#[doc(hidden)]
pub struct LiveComponent<T>(Rc<State>, PhantomData<fn() -> T>);

struct State {
    instance: ReloadableInstance,
    lifetime: ComponentLifetime,
    _watch_lifetime: Arc<()>,
    #[cfg(unix)]
    _attach: Option<attach::Session>,
}

#[doc(hidden)]
#[derive(Deserialize)]
pub struct PreparedUi {
    pub mlir: String,
    pub inputs: BTreeSet<PathBuf>,
}

#[doc(hidden)]
pub type SavedSources = BTreeMap<PathBuf, Result<String, io::ErrorKind>>;

type Buffers = BTreeMap<PathBuf, String>;

#[derive(Clone, Default)]
pub(crate) struct EditorSources(Arc<EditorUpdates>);

#[derive(Default)]
struct EditorUpdates {
    state: Mutex<EditorState>,
    changed: Condvar,
}

#[derive(Default)]
struct EditorState {
    buffers: Arc<Buffers>,
    diagnostic: Option<String>,
    #[cfg(unix)]
    source_versions: crate::live_inspect::SourceVersions,
}

impl EditorSources {
    pub(crate) fn replace(&self, buffers: Buffers) {
        let mut editor = self.0.state.lock().unwrap();
        if *editor.buffers != buffers {
            editor.buffers = Arc::new(buffers);
            self.0.changed.notify_one();
        }
    }

    fn snapshot(&self) -> Arc<Buffers> {
        self.0.state.lock().unwrap().buffers.clone()
    }

    fn wait(&self, seen: &Arc<Buffers>, interval: Duration) -> Arc<Buffers> {
        let state = self.0.state.lock().unwrap();
        let (state, _) = self
            .0
            .changed
            .wait_timeout_while(state, interval, |state| Arc::ptr_eq(seen, &state.buffers))
            .unwrap();
        state.buffers.clone()
    }

    fn diagnostic(&self, diagnostic: Option<String>) {
        self.0.state.lock().unwrap().diagnostic = diagnostic;
    }

    pub(crate) fn last_diagnostic(&self) -> Option<String> {
        self.0.state.lock().unwrap().diagnostic.clone()
    }

    #[cfg(unix)]
    pub(crate) fn source_versions(&self) -> crate::live_inspect::SourceVersions {
        self.0.state.lock().unwrap().source_versions.clone()
    }

    #[cfg(unix)]
    fn set_source_versions(&self, versions: crate::live_inspect::SourceVersions) {
        self.0.state.lock().unwrap().source_versions = versions;
    }
}

impl<T: LiveUi> LiveComponent<T> {
    pub fn open_bundled(
        filename: &str,
        name: &str,
        compiler: &str,
        project: &str,
    ) -> Result<Self, String> {
        let compiler = PathBuf::from(compiler);
        Self::open_with(
            filename,
            name,
            Some(Path::new(project)),
            move |source, saved| compile_with_command(&compiler, source, saved),
        )
    }

    pub fn open_with(
        filename: &str,
        name: &str,
        project: Option<&Path>,
        compile: impl Fn(&Path, &SavedSources) -> Result<PreparedUi, String> + Send + 'static,
    ) -> Result<Self, String> {
        let source = env::var_os("XRACK_LIVE_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                env::var_os("BUILD_WORKSPACE_DIRECTORY")
                    .map(PathBuf::from)
                    .unwrap_or_default()
                    .join(filename)
            });
        let source = source
            .canonicalize()
            .map_err(|error| format!("{}: {error}", source.display()))?;
        let initial = BTreeMap::from([(source.clone(), read(&source))]);
        let prepared = compile(&source, &initial)?;
        let instance = load(&source, &prepared)?
            .create(name)
            .map_err(|error| error.to_string())?;
        let component = ReloadableInstance::new(instance);
        let weak = component.as_weak();
        let watch_lifetime = Arc::new(());
        let alive = Arc::downgrade(&watch_lifetime);
        let editor = EditorSources::default();
        let saved = watched(&source, prepared.inputs, initial);
        #[cfg(unix)]
        editor.set_source_versions(crate::live_inspect::source_versions(&saved));
        #[cfg(unix)]
        let attach_session = project.and_then(|project| {
            match attach::Session::open(project, &source, name, weak.clone(), editor.clone()) {
                Ok(session) => Some(session),
                Err(error) => {
                    eprintln!("XRack inspection unavailable: {error}");
                    None
                }
            }
        });
        #[cfg(not(unix))]
        let _ = project;
        eprintln!("XRack live reload watching {}", source.display());
        thread::Builder::new()
            .name("xrack-live-reload".into())
            .spawn(move || watch(source, saved, weak, alive, editor, compile))
            .map_err(|error| error.to_string())?;
        Ok(Self(
            Rc::new(State {
                instance: component,
                lifetime: ComponentLifetime::default(),
                _watch_lifetime: watch_lifetime,
                #[cfg(unix)]
                _attach: attach_session,
            }),
            PhantomData,
        ))
    }

    pub fn component(&self) -> ReloadableInstance {
        self.0.instance.clone()
    }

    pub fn as_weak(&self) -> WeakComponent<T> {
        WeakComponent::new(&self.0, &self.0.lifetime, |state| {
            T::from_live(Self(state, PhantomData))
        })
    }
}

#[doc(hidden)]
pub fn source_text<'a>(source: &Path, saved: &'a SavedSources) -> Result<&'a str, String> {
    saved[source]
        .as_deref()
        .map_err(|error| format!("{}: {error}", source.display()))
}

#[doc(hidden)]
pub fn imported_sources(source: &Path, saved: &SavedSources) -> BTreeMap<PathBuf, String> {
    saved
        .iter()
        .filter(|(path, _)| path.as_path() != source)
        .filter_map(|(path, content)| Some((path.clone(), content.as_ref().ok()?.clone())))
        .collect()
}

fn compile_with_command(
    compiler: &Path,
    source: &Path,
    saved: &SavedSources,
) -> Result<PreparedUi, String> {
    let request = serde_json::to_vec(&serde_json::json!({
        "text": source_text(source, saved)?,
        "buffers": imported_sources(source, saved),
    }))
    .map_err(|error| error.to_string())?;
    let mut child = Command::new(compiler)
        .args(["--emit", "live-module"])
        .arg(source)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("{}: {error}", compiler.display()))?;
    let sent = child.stdin.take().expect("piped input").write_all(&request);
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    sent.map_err(|error| error.to_string())?;
    serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())
}

fn load(source: &Path, prepared: &PreparedUi) -> Result<Program, String> {
    let module = xrack_mlir::Module::parse(&prepared.mlir)?;
    let translations =
        xrack_resources::resolve_translations(&module, source, None, &BTreeMap::new())?;
    let (program, _) =
        Program::from_module_with_file_resources(&module, source, translations.catalog)
            .map_err(|error| error.to_string())?;
    Ok(program)
}

fn read(path: &Path) -> Result<String, io::ErrorKind> {
    fs::read_to_string(path).map_err(|error| error.kind())
}

fn watched(source: &Path, inputs: BTreeSet<PathBuf>, mut saved: SavedSources) -> SavedSources {
    inputs
        .into_iter()
        .filter(|path| path == source || path.extension().is_some_and(|ext| ext == "xr"))
        .map(|path| {
            let contents = saved.remove(&path).unwrap_or_else(|| read(&path));
            (path, contents)
        })
        .collect()
}

fn watch(
    source: PathBuf,
    mut saved: SavedSources,
    ui: WeakComponent<ReloadableInstance>,
    alive: Weak<()>,
    editor: EditorSources,
    compile: impl Fn(&Path, &SavedSources) -> Result<PreparedUi, String>,
) {
    let interval = Duration::from_millis(60);
    let mut attempted = saved.clone();
    let mut seen = editor.snapshot();
    while alive.strong_count() != 0 {
        let buffers = editor.wait(&seen, interval);
        seen = buffers.clone();
        let disk: SavedSources = saved
            .keys()
            .map(|path| (path.clone(), read(path)))
            .collect();
        let effective = overlay(&disk, &buffers);
        if effective == attempted {
            continue;
        }
        let prepared = match compile(&source, &effective) {
            Ok(prepared) => prepared,
            Err(error) => {
                if Arc::ptr_eq(&buffers, &editor.snapshot()) {
                    eprintln!("XRack live reload skipped: {error}");
                    editor.diagnostic(Some(error));
                    attempted = effective;
                }
                continue;
            }
        };
        if alive.strong_count() == 0 {
            return;
        }
        if !Arc::ptr_eq(&buffers, &editor.snapshot()) {
            continue;
        }
        let filename = source.clone();
        let next_saved = watched(&source, prepared.inputs.clone(), disk.clone());
        #[cfg(unix)]
        let versions = crate::live_inspect::source_versions(&effective);
        let editor_for_ui = editor.clone();
        let (done, received) = mpsc::channel();
        if ui
            .upgrade_in_event_loop(move |ui| {
                if !Arc::ptr_eq(&buffers, &editor_for_ui.snapshot()) {
                    let _ = done.send(false);
                    return;
                }
                let result = load(&filename, &prepared).and_then(|program| {
                    ui.reload_if_current(&program, || {
                        Arc::ptr_eq(&buffers, &editor_for_ui.snapshot())
                    })
                    .map_err(|error| error.to_string())
                });
                match result {
                    Ok(false) => {
                        let _ = done.send(false);
                        return;
                    }
                    Ok(true) => {
                        #[cfg(unix)]
                        editor_for_ui.set_source_versions(versions);
                        eprintln!("XRack live reload applied: {}", filename.display());
                        editor_for_ui.diagnostic(None);
                    }
                    Err(error) => {
                        eprintln!("XRack live reload skipped: {error}");
                        editor_for_ui.diagnostic(Some(error));
                    }
                }
                let _ = done.send(true);
            })
            .is_err()
        {
            // The application may still be starting its event loop.
            continue;
        }
        loop {
            match received.recv_timeout(interval) {
                Ok(applied) => {
                    if applied {
                        // Keep the exact inputs just rendered. Adding a newly
                        // imported file to the watch set can require one more pass.
                        attempted = effective;
                        saved = next_saved;
                    }
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) if alive.strong_count() != 0 => {}
                Err(_) => return,
            }
        }
    }
}

fn overlay(disk: &SavedSources, buffers: &Buffers) -> SavedSources {
    disk.iter()
        .map(|(path, value)| {
            let value = buffers
                .get(path)
                .map_or_else(|| value.clone(), |text| Ok(text.clone()));
            (path.clone(), value)
        })
        .collect()
}
