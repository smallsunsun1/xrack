use crate::{ClipboardShortcut, UserEvent, desktop::WindowEvents};
#[cfg(target_os = "android")]
use android_activity::AndroidApp;
#[cfg(not(any(target_os = "ios", target_os = "android")))]
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

#[cfg(not(any(target_os = "ios", target_os = "android")))]
struct Request {
    id: u64,
    operation: Operation,
    events: WindowEvents,
}

pub(crate) struct Reply {
    pub id: u64,
    pub result: Result<ClipboardValue, String>,
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
struct Worker {
    send: Option<SyncSender<Request>>,
    stopped: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    finished: Receiver<()>,
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
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

#[cfg(not(any(target_os = "ios", target_os = "android")))]
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

#[cfg(not(any(target_os = "ios", target_os = "android")))]
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
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    worker: Option<Worker>,
    #[cfg(target_os = "android")]
    app: AndroidApp,
    next: u64,
}

impl ClipboardPort {
    pub(crate) fn new(#[cfg(target_os = "android")] app: AndroidApp) -> Self {
        Self {
            #[cfg(not(any(target_os = "ios", target_os = "android")))]
            worker: None,
            #[cfg(target_os = "android")]
            app,
            next: 0,
        }
    }

    pub(crate) fn request(
        &mut self,
        events: &WindowEvents,
        operation: Operation,
    ) -> Result<u64, String> {
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        let worker = match &mut self.worker {
            Some(worker) => worker,
            None => self
                .worker
                .insert(Worker::new().map_err(|error| error.to_string())?),
        };
        let id = self.next;
        self.next = id.checked_add(1).ok_or("clipboard request ID exhausted")?;
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        worker
            .send
            .as_ref()
            .ok_or("clipboard worker stopped")?
            .try_send(Request {
                id,
                operation,
                events: events.clone(),
            })
            .map_err(|error| error.to_string())?;
        #[cfg(target_os = "android")]
        let result = perform_android(&self.app, operation);
        #[cfg(target_os = "ios")]
        let result = match operation {
            // Record the target before UIKit opens the authorization alert.
            Operation::Read => {
                return events
                    .post(UserEvent::IosReadClipboard(id))
                    .then_some(id)
                    .ok_or_else(|| "clipboard event loop is closed".into());
            }
            operation => perform_ios(operation),
        };
        #[cfg(any(target_os = "ios", target_os = "android"))]
        if !events.post(UserEvent::Clipboard(Reply { id, result })) {
            return Err("clipboard event loop is closed".into());
        }
        Ok(id)
    }
}

#[cfg(target_os = "android")]
fn perform_android(app: &AndroidApp, operation: Operation) -> Result<ClipboardValue, String> {
    use jni::{
        JavaVM, jni_sig, jni_str,
        objects::{JObject, JValue},
        refs::Global,
    };

    // SAFETY: AndroidApp supplies the VM and retains the Activity global reference.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let raw_activity = app.activity_as_ptr().cast();
    vm.attach_current_thread_for_scope(|env| -> jni::errors::Result<_> {
        // SAFETY: Borrow the Activity reference for this JNI call without taking ownership.
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity)? };
        let service = env.new_string("clipboard")?;
        let clipboard = env
            .call_method(
                activity,
                jni_str!("getSystemService"),
                jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                &[JValue::Object(service.as_ref())],
            )?
            .l()?;
        match operation {
            Operation::Read => read_android_clip(env, &clipboard).map(ClipboardValue::Text),
            Operation::Write(text) => {
                let label = env.new_string("")?;
                let text = env.new_string(text)?;
                let clip = env
                    .call_static_method(
                        jni_str!("android/content/ClipData"),
                        jni_str!("newPlainText"),
                        jni_sig!("(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;"),
                        &[JValue::Object(label.as_ref()), JValue::Object(text.as_ref())],
                    )?
                    .l()?;
                env.call_method(
                    &clipboard,
                    jni_str!("setPrimaryClip"),
                    jni_sig!("(Landroid/content/ClipData;)V"),
                    &[JValue::Object(&clip)],
                )?;
                Ok(ClipboardValue::Written)
            }
        }
    })
    .map_err(|error| error.to_string())
}

#[cfg(target_os = "android")]
fn read_android_clip(
    env: &mut jni::Env<'_>,
    clipboard: &jni::objects::JObject<'_>,
) -> jni::errors::Result<String> {
    use jni::{
        jni_sig, jni_str,
        objects::{JString, JValue},
        refs::Reference,
    };

    let clip = env
        .call_method(
            clipboard,
            jni_str!("getPrimaryClip"),
            jni_sig!("()Landroid/content/ClipData;"),
            &[],
        )?
        .l()?;
    if clip.is_null() {
        return Ok(String::new());
    }
    let item = env
        .call_method(
            &clip,
            jni_str!("getItemAt"),
            jni_sig!("(I)Landroid/content/ClipData$Item;"),
            &[JValue::Int(0)],
        )?
        .l()?;
    let text = env
        .call_method(
            &item,
            jni_str!("getText"),
            jni_sig!("()Ljava/lang/CharSequence;"),
            &[],
        )?
        .l()?;
    if text.is_null() {
        return Ok(String::new());
    }
    let string = env
        .call_method(
            &text,
            jni_str!("toString"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .l()?;
    env.cast_local::<JString>(string)?.try_to_string(env)
}

#[cfg(target_os = "ios")]
pub(crate) fn perform_ios(operation: Operation) -> Result<ClipboardValue, String> {
    use objc2_foundation::{MainThreadMarker, NSString};
    use objc2_ui_kit::UIPasteboard;

    let _main_thread = MainThreadMarker::new().ok_or("UIKit clipboard requires the main thread")?;
    // SAFETY: The pasteboard is obtained and accessed on the UIKit main thread.
    unsafe {
        let clipboard = UIPasteboard::generalPasteboard();
        Ok(match operation {
            Operation::Read => ClipboardValue::Text(
                clipboard
                    .string()
                    .map_or_else(String::new, |text| text.to_string()),
            ),
            Operation::Write(text) => {
                clipboard.setString(Some(&NSString::from_str(&text)));
                ClipboardValue::Written
            }
        })
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
