use crate::{ClipboardShortcut, UserEvent, desktop::WindowEvents};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use xrack_layout::{Interaction, TextEditContext};
use xrack_runtime::{Node, TextCommand};

pub(crate) enum Operation {
    Read,
    Write(String),
}

pub(crate) enum ClipboardValue {
    Text(String),
    Written,
}

struct Request {
    id: u64,
    operation: Operation,
    events: WindowEvents,
}

pub(crate) struct Reply {
    pub id: u64,
    pub result: Result<ClipboardValue, String>,
}

struct Worker {
    send: Option<SyncSender<Request>>,
    stopped: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    finished: Receiver<()>,
}

impl Worker {
    fn new() -> std::io::Result<Self> {
        let (send, receive) = sync_channel::<Request>(8);
        let (done, finished) = sync_channel(1);
        let stopped = Arc::new(AtomicBool::new(false));
        let stop = stopped.clone();
        let join = thread::Builder::new()
            .name("xrack-clipboard".into())
            .spawn(move || {
                // Retain one clipboard owner for X11 and Wayland selections.
                let mut clipboard = None;
                while let Ok(request) = receive.recv() {
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    let result = perform(&mut clipboard, request.operation)
                        .map_err(|error| error.to_string());
                    if !request.events.post(UserEvent::Clipboard(Reply {
                        id: request.id,
                        result,
                    })) {
                        break;
                    }
                }
                drop(clipboard);
                let _ = done.send(());
            })?;
        Ok(Self {
            send: Some(send),
            stopped,
            join: Some(join),
            finished,
        })
    }
}

fn perform(
    clipboard: &mut Option<arboard::Clipboard>,
    operation: Operation,
) -> Result<ClipboardValue, arboard::Error> {
    let clipboard = match clipboard {
        Some(clipboard) => clipboard,
        None => clipboard.insert(arboard::Clipboard::new()?),
    };
    match operation {
        Operation::Read => match clipboard.get_text() {
            Ok(text) => Ok(ClipboardValue::Text(text)),
            Err(arboard::Error::ContentNotAvailable) => Ok(ClipboardValue::Text(String::new())),
            Err(error) => Err(error),
        },
        Operation::Write(text) => clipboard.set_text(text).map(|()| ClipboardValue::Written),
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        drop(self.send.take());
        // A remote clipboard owner can delay a read. Give normal shutdown a
        // chance to hand off the selection while keeping exit bounded.
        if self.finished.recv_timeout(Duration::from_secs(1)) != Err(RecvTimeoutError::Timeout)
            && let Some(thread) = self.join.take()
        {
            let _ = thread.join();
        }
    }
}

pub(crate) struct ClipboardPort {
    worker: Option<Worker>,
    next: u64,
}

impl ClipboardPort {
    pub(crate) fn new() -> Self {
        Self {
            worker: None,
            next: 0,
        }
    }

    pub(crate) fn request(
        &mut self,
        events: &WindowEvents,
        operation: Operation,
    ) -> Result<u64, String> {
        let worker = match &mut self.worker {
            Some(worker) => worker,
            None => self
                .worker
                .insert(Worker::new().map_err(|error| error.to_string())?),
        };
        let id = self.next;
        self.next = id.checked_add(1).expect("clipboard request ID exhausted");
        worker
            .send
            .as_ref()
            .unwrap()
            .try_send(Request {
                id,
                operation,
                events: events.clone(),
            })
            .map_err(|error| error.to_string())?;
        Ok(id)
    }
}

pub(super) fn edit_request(
    input: &Interaction,
    scene: &Node,
    shortcut: &ClipboardShortcut,
) -> Option<(Operation, TextEditContext)> {
    let command = match shortcut {
        ClipboardShortcut::Cut => TextCommand::Cut,
        ClipboardShortcut::Copy => TextCommand::Copy,
        ClipboardShortcut::Paste => TextCommand::Paste,
    };
    let context = TextEditContext::for_clipboard(input, scene, command)?;
    let operation = if command == TextCommand::Paste {
        Operation::Read
    } else {
        Operation::Write(context.selection().to_owned())
    };
    Some((operation, context))
}
