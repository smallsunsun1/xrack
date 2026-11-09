use crate::{ClipboardShortcut, UserEvent, desktop::WindowEvents};
use std::{
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use xrack_layout::{Interaction, Target, TextPosition};
use xrack_runtime::{Node, spin_editable};

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

pub(crate) struct EditContext {
    target: Target,
    text: String,
    selection: Range<usize>,
    caret: TextPosition,
    password: bool,
}

impl EditContext {
    pub(super) fn request(
        input: &Interaction,
        scene: &Node,
        shortcut: &ClipboardShortcut,
    ) -> Option<(Operation, Self)> {
        let node = scene.interactive_at_path(&input.focused.as_ref()?.path)?;
        if node.is_password() && !matches!(shortcut, ClipboardShortcut::Paste)
            || node.boolean("read-only", false) && !matches!(shortcut, ClipboardShortcut::Copy)
        {
            return None;
        }
        let context = Self::capture(input, scene)?;
        let operation = match shortcut {
            ClipboardShortcut::Paste => Operation::Read,
            ClipboardShortcut::Copy | ClipboardShortcut::Cut if !context.selection().is_empty() => {
                Operation::Write(context.selection().into())
            }
            _ => return None,
        };
        Some((operation, context))
    }

    pub(crate) fn capture(input: &Interaction, scene: &Node) -> Option<Self> {
        let (target, text, selection, caret) = current(input, scene)?;
        Some(Self {
            target: target.clone(),
            text: text.into_owned(),
            selection,
            caret,
            password: scene.at_path(&target.path)?.is_password(),
        })
    }

    pub(crate) fn selection(&self) -> &str {
        &self.text[self.selection.clone()]
    }

    pub(crate) fn is_current(&self, input: &Interaction, scene: &Node) -> bool {
        current(input, scene).is_some_and(|(target, text, selection, caret)| {
            self.target == *target
                && scene
                    .at_path(&target.path)
                    .is_some_and(|node| node.is_password() == self.password)
                && self.text == text
                && self.selection == selection
                && self.caret == caret
        })
    }
}

fn current<'a>(
    input: &'a Interaction,
    scene: &'a Node,
) -> Option<(
    &'a Target,
    std::borrow::Cow<'a, str>,
    Range<usize>,
    TextPosition,
)> {
    let target = input.focused.as_ref()?;
    let node = scene
        .interactive_at_path(&target.path)
        .filter(|node| input.is_text_editor(node, target) || spin_editable(node))?;
    if scene.row_key_at(&target.path) != target.row_key {
        return None;
    }
    let editor = input
        .editor
        .as_ref()
        .filter(|editor| editor.preedit.is_empty())?;
    let text = input.editor_value(node);
    let selection = editor.selection(&text);
    let caret = editor.position(&text);
    Some((target, text, selection, caret))
}
