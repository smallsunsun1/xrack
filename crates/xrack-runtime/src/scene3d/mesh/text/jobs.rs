use std::{
    cell::RefCell,
    collections::VecDeque,
    rc::Rc,
    sync::{Arc, mpsc},
};

use crate::{ChangeSignal, scene3d::Mesh3D};

use super::{GeometryStatus, PreparedText, TextRecipe};
use crate::scene3d::mesh::{
    authored::SceneMeshInputs,
    worker::{self, SubmitError},
};

const REQUEST_LIMIT: usize = 64;
const ACTIVE_LIMIT: usize = 2;

enum Work {
    Waiting,
    Building {
        result: mpsc::Receiver<Option<Mesh3D>>,
        _token: Arc<()>,
    },
    Done(PreparedText),
}

pub(in crate::scene3d::mesh) struct TextRequest {
    recipe: TextRecipe,
    work: RefCell<Work>,
    changes: ChangeSignal,
}

thread_local! {
    static REQUESTS: RefCell<VecDeque<Rc<TextRequest>>> = const { RefCell::new(VecDeque::new()) };
}

impl TextRequest {
    pub(in crate::scene3d::mesh) fn read(&self) -> PreparedText {
        self.changes.track();
        match &*self.work.borrow() {
            Work::Done(result) => result.clone(),
            _ => PreparedText::loading(),
        }
    }

    fn submit(&self) -> Result<(), SubmitError> {
        let token = Arc::new(());
        let weak = Arc::downgrade(&token);
        let (send, result) = mpsc::channel();
        let recipe = self.recipe.clone();
        worker::submit(move || {
            if weak.strong_count() != 0 {
                let mesh = recipe.mesh();
                if weak.strong_count() != 0 {
                    let _ = send.send(mesh);
                }
            }
        })?;
        self.work.replace(Work::Building {
            result,
            _token: token,
        });
        Ok(())
    }

    fn poll(&self) -> bool {
        let outcome = match &*self.work.borrow() {
            Work::Building { result, .. } => match result.try_recv() {
                Ok(mesh) => PreparedText::from_mesh(mesh),
                Err(mpsc::TryRecvError::Empty) => return false,
                Err(mpsc::TryRecvError::Disconnected) => {
                    PreparedText::error("3D text worker stopped before finishing")
                }
            },
            _ => return false,
        };
        self.work.replace(Work::Done(outcome));
        true
    }
}

pub(super) fn prepare(recipe: TextRecipe, inputs: &SceneMeshInputs) -> PreparedText {
    let existing = REQUESTS.with_borrow_mut(|requests| {
        let index = requests.iter().position(|job| job.recipe == recipe)?;
        let job = requests.remove(index)?;
        requests.push_front(job.clone());
        Some(job)
    });
    if let Some(job) = existing {
        return inputs.observe_text(job);
    }
    if let Some(mesh) = recipe.cached() {
        return PreparedText::from_mesh(Some(mesh));
    }
    let request = Rc::new(TextRequest {
        recipe,
        work: RefCell::new(Work::Waiting),
        changes: ChangeSignal::default(),
    });
    let evicted = REQUESTS.with_borrow_mut(|requests| {
        let evicted = if requests.len() >= REQUEST_LIMIT {
            requests.pop_back()
        } else {
            None
        };
        requests.push_front(request.clone());
        evicted
    });
    if let Some(job) = evicted {
        let pending = !matches!(*job.work.borrow(), Work::Done(_));
        if pending {
            job.work.replace(Work::Done(PreparedText::error(
                "3D text request limit reached",
            )));
            job.changes.notify();
        }
    }
    start_waiting();
    inputs.observe_text(request)
}

fn start_waiting() {
    let failed = REQUESTS.with_borrow(|requests| {
        let mut active = requests
            .iter()
            .filter(|job| matches!(*job.work.borrow(), Work::Building { .. }))
            .count();
        let mut failed = Vec::new();
        for job in requests {
            if active >= ACTIVE_LIMIT {
                break;
            }
            if matches!(*job.work.borrow(), Work::Waiting) {
                match job.submit() {
                    Ok(()) => active += 1,
                    Err(SubmitError::Busy) => break,
                    Err(error) => {
                        job.work
                            .replace(Work::Done(PreparedText::error(error.message())));
                        failed.push(job.clone());
                    }
                }
            }
        }
        failed
    });
    for job in failed {
        job.changes.notify();
    }
}

pub(in crate::scene3d::mesh) fn poll_text_jobs() {
    let completed = REQUESTS.with_borrow_mut(|requests| {
        let mut completed = Vec::new();
        requests.retain(|job| {
            if job.poll() {
                completed.push(job.clone());
            }
            // Successful meshes already have a memory-bounded geometry cache.
            !matches!(&*job.work.borrow(), Work::Done(text) if text.status == GeometryStatus::Ready)
        });
        completed
    });
    for job in completed {
        job.changes.notify();
    }
    start_waiting();
}

pub(in crate::scene3d::mesh) fn text_jobs_pending() -> bool {
    REQUESTS.with_borrow(|requests| {
        requests
            .iter()
            .any(|job| !matches!(*job.work.borrow(), Work::Done(_)))
    })
}
