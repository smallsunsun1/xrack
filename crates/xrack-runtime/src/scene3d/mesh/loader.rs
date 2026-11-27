use crate::{MeshAsset3D, Property, scene3d::MeshImportOptions};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    rc::{Rc, Weak},
    sync::{Arc, OnceLock, Weak as WorkerToken, mpsc},
    time::{Duration, Instant},
};

type Import = fn(&Path, MeshImportOptions) -> Result<MeshAsset3D, String>;
static IMPORT: OnceLock<Import> = OnceLock::new();

/// Install the Rust scene importer for declarative loaders on this host.
#[doc(hidden)]
pub fn register_mesh_importer(import: Import) {
    let _ = IMPORT.set(import);
}

#[derive(Clone, PartialEq)]
struct Input {
    path: PathBuf,
    revision: i64,
    options: MeshImportOptions,
}

#[derive(Clone, Default, PartialEq)]
enum Output {
    #[default]
    Empty,
    Loading,
    Ready(MeshAsset3D),
    Failed(String),
}

#[derive(Default)]
struct State {
    input: RefCell<Option<Input>>,
    job: RefCell<Option<Arc<()>>>,
    output: Property<Output>,
}

#[derive(Clone, Default)]
#[doc(hidden)]
pub struct MeshLoader3D(Rc<State>);

struct Pending {
    target: Weak<State>,
    token: WorkerToken<()>,
    result: mpsc::Receiver<Result<MeshAsset3D, String>>,
}

thread_local! {
    static PENDING: RefCell<Vec<Pending>> = const { RefCell::new(Vec::new()) };
}

impl MeshLoader3D {
    pub fn configure(
        &self,
        declaration: &str,
        source: &str,
        active: bool,
        revision: i64,
        options: MeshImportOptions,
    ) {
        let input = (active && !source.is_empty()).then(|| Input {
            path: Path::new(declaration)
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(source),
            revision,
            options,
        });
        if *self.0.input.borrow() == input {
            return;
        }
        self.0.job.take();
        self.0.input.replace(input.clone());
        let output = match input {
            None => Output::Empty,
            Some(input) => match self.submit(input) {
                Ok(()) => Output::Loading,
                Err(error) => Output::Failed(error),
            },
        };
        self.0.output.set_if_changed(output);
    }

    pub fn asset(&self) -> MeshAsset3D {
        match self.0.output.get(Output::default) {
            Output::Ready(asset) => asset,
            _ => MeshAsset3D::default(),
        }
    }

    pub fn bounds(&self) -> crate::Bounds3D {
        self.asset().bounds()
    }

    pub fn status(&self) -> String {
        match self.0.output.get(Output::default) {
            Output::Empty => "empty",
            Output::Loading => "loading",
            Output::Ready(_) => "ready",
            Output::Failed(_) => "error",
        }
        .into()
    }

    pub fn error(&self) -> String {
        match self.0.output.get(Output::default) {
            Output::Failed(error) => error,
            _ => String::new(),
        }
    }

    /// Keep the prepared asset across a layout reload. The next configuration
    /// still decides whether the source and options match.
    pub fn copy_completed(&self) -> Option<Self> {
        let input = self.0.input.borrow().clone()?;
        let output @ Output::Ready(_) = self.0.output.cached_value()? else {
            return None;
        };
        let copy = Self::default();
        copy.0.input.replace(Some(input));
        copy.0.output.set(output);
        Some(copy)
    }

    fn submit(&self, input: Input) -> Result<(), String> {
        let import = *IMPORT
            .get()
            .ok_or("3D file importing is unavailable on this host")?;
        let token = Arc::new(());
        let weak = Arc::downgrade(&token);
        let worker_token = weak.clone();
        let (send, result) = mpsc::channel();
        super::worker::submit(move || {
            if worker_token.strong_count() != 0 {
                let value = import(&input.path, input.options);
                if worker_token.strong_count() != 0 {
                    let _ = send.send(value);
                }
            }
        })
        .map_err(|error| {
            match error {
                super::worker::SubmitError::Busy => "3D importer is busy",
                super::worker::SubmitError::Stopped => "3D importer has stopped",
            }
            .to_owned()
        })?;
        self.0.job.replace(Some(token));
        PENDING.with_borrow_mut(|pending| {
            pending.retain(|job| job.token.strong_count() != 0);
            pending.push(Pending {
                target: Rc::downgrade(&self.0),
                token: weak,
                result,
            });
        });
        Ok(())
    }
}

/// Deliver completed mesh imports and generated geometry on the UI thread.
#[doc(hidden)]
pub fn poll_mesh_loaders() {
    let completed = PENDING.with_borrow_mut(|pending| {
        let mut completed = Vec::new();
        pending.retain(|request| {
            let (Some(target), Some(token)) = (request.target.upgrade(), request.token.upgrade())
            else {
                return false;
            };
            let output = match request.result.try_recv() {
                Ok(Ok(asset)) => Output::Ready(asset),
                Ok(Err(error)) => Output::Failed(error),
                Err(mpsc::TryRecvError::Empty) => return true,
                Err(mpsc::TryRecvError::Disconnected) => Output::Failed("3D import stopped".into()),
            };
            completed.push((target, token, output));
            false
        });
        completed
    });
    for (target, token, output) in completed {
        if target
            .job
            .borrow()
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &token))
        {
            target.job.take();
            target.output.set_if_changed(output);
        }
    }
    super::text::poll_text_jobs();
}

#[doc(hidden)]
pub fn mesh_loader_deadline(now: Instant) -> Option<Instant> {
    let importing =
        PENDING.with_borrow(|pending| pending.iter().any(|job| job.token.strong_count() != 0));
    (importing || super::text::text_jobs_pending()).then_some(now + Duration::from_millis(25))
}
