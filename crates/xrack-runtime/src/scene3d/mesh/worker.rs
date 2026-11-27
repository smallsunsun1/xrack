//! A small, shared queue for mesh work that must stay off the UI thread.

use std::sync::{Arc, Mutex, OnceLock, mpsc};

type Work = Box<dyn FnOnce() + Send>;
static WORKERS: OnceLock<Result<mpsc::SyncSender<Work>, String>> = OnceLock::new();

pub(super) enum SubmitError {
    Busy,
    Stopped,
}

impl SubmitError {
    pub(super) fn message(self) -> &'static str {
        match self {
            Self::Busy => "3D mesh worker queue is busy",
            Self::Stopped => "3D mesh workers are unavailable",
        }
    }
}

pub(super) fn submit(work: impl FnOnce() + Send + 'static) -> Result<(), SubmitError> {
    WORKERS
        .get_or_init(start)
        .as_ref()
        .map_err(|_| SubmitError::Stopped)?
        .try_send(Box::new(work))
        .map_err(|error| match error {
            mpsc::TrySendError::Full(_) => SubmitError::Busy,
            mpsc::TrySendError::Disconnected(_) => SubmitError::Stopped,
        })
}

fn start() -> Result<mpsc::SyncSender<Work>, String> {
    let (send, receive) = mpsc::sync_channel::<Work>(32);
    let receive = Arc::new(Mutex::new(receive));
    for index in 0..2 {
        let receive = receive.clone();
        let worker = std::thread::Builder::new()
            .name(format!("xrack-mesh-worker-{index}"))
            .spawn(move || {
                loop {
                    let work = receive
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .recv();
                    let Ok(work) = work else { break };
                    work();
                }
            });
        if index == 0 {
            worker.map_err(|error| error.to_string())?;
        }
    }
    Ok(send)
}
