mod documents;
mod intent;
mod message;
mod share;

use android_activity::AndroidApp;
pub(crate) use documents::ImportedFiles;
use documents::Picker;
use jni::{
    EnvUnowned, NativeMethod, jni_sig, jni_str,
    objects::{JObject, JString, JValue},
};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicI64, Ordering},
    },
};
use tokio::sync::oneshot;
use xrack_runtime::{
    FileDialogMode, MessageButtons, NativeDialogOptions, NativeDialogResult, StandardButtonKind,
};

static NEXT_TOKEN: AtomicI64 = AtomicI64::new(1);
static PENDING: LazyLock<Mutex<HashMap<i64, PendingReply>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static BUSY_ACTIVITIES: LazyLock<Mutex<HashSet<usize>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

struct Pending(i64);

struct PendingReply {
    send: oneshot::Sender<NativeDialogResult>,
    kind: DialogKind,
    share: Option<share::ShareRequest>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DialogKind {
    Document(Picker),
    Message(MessageButtons),
    ShareText,
}

struct ActivePanel(usize);

impl ActivePanel {
    fn begin(app: &AndroidApp) -> Result<Self, String> {
        let activity = app.activity_as_ptr() as usize;
        if !BUSY_ACTIVITIES
            .lock()
            .map_err(|error| error.to_string())?
            .insert(activity)
        {
            return Err("The Android activity cannot open another dialog.".into());
        }
        Ok(Self(activity))
    }
}

impl Drop for ActivePanel {
    fn drop(&mut self) {
        if let Ok(mut activities) = BUSY_ACTIVITIES.lock() {
            activities.remove(&self.0);
        }
    }
}

struct Panel {
    result: NativeDialogResult,
    active: ActivePanel,
}

pub(crate) enum AndroidDialogResult {
    Ready(NativeDialogResult),
    Imported(ImportedFiles),
}

pub(crate) enum SystemRequest {
    ShareText {
        text: String,
        reply: oneshot::Sender<Result<bool, String>>,
    },
    ExportFile {
        path: std::path::PathBuf,
        reply: oneshot::Sender<Result<bool, String>>,
    },
    SelectDirectory {
        reply: oneshot::Sender<Result<Option<crate::AndroidDirectory>, String>>,
    },
}

impl SystemRequest {
    pub(crate) async fn present(self, app: AndroidApp) {
        match self {
            Self::ShareText { text, reply } => {
                let result = match open(&app, DialogKind::ShareText, "", &text, &[]).await {
                    Ok(Panel {
                        result: NativeDialogResult::Canceled,
                        ..
                    }) => Ok(false),
                    Ok(Panel {
                        result: NativeDialogResult::Button(StandardButtonKind::Ok),
                        ..
                    }) => Ok(true),
                    Ok(Panel {
                        result: NativeDialogResult::Failed(error),
                        ..
                    })
                    | Err(error) => Err(error),
                    _ => Err("The share sheet returned an unexpected result.".into()),
                };
                let _ = reply.send(result);
            }
            Self::ExportFile { path, reply } => {
                let _ = reply.send(export_file(app, &path).await);
            }
            Self::SelectDirectory { reply } => {
                let result =
                    match open(&app, DialogKind::Document(Picker::Directory), "", "", &[]).await {
                        Ok(Panel {
                            result: NativeDialogResult::Canceled,
                            ..
                        }) => Ok(None),
                        Ok(Panel {
                            result: NativeDialogResult::Files(paths),
                            ..
                        }) => paths
                            .into_iter()
                            .next()
                            .map(|uri| Some(crate::AndroidDirectory::new(app, uri)))
                            .ok_or_else(|| "The directory picker returned no directory.".into()),
                        Ok(Panel {
                            result: NativeDialogResult::Failed(error),
                            ..
                        })
                        | Err(error) => Err(error),
                        _ => Err("The directory picker returned an unexpected result.".into()),
                    };
                let _ = reply.send(result);
            }
        }
    }
}

async fn export_file(app: AndroidApp, path: &std::path::Path) -> Result<bool, String> {
    if !path.is_absolute() || !path.is_file() {
        return Err("The export source must be an existing file with an absolute path.".into());
    }
    let source = path
        .to_str()
        .ok_or("The export source cannot be represented as UTF-8.")?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("The export source has no file name.")?;
    let panel = open(&app, DialogKind::Document(Picker::Create), name, "", &[]).await?;
    match panel.result {
        NativeDialogResult::Canceled => Ok(false),
        NativeDialogResult::Files(uris) => {
            let destination = uris
                .into_iter()
                .next()
                .ok_or("The export returned no document.")?;
            let source = source.to_owned();
            file_worker(panel.active, move || {
                documents::export_file(&app, &source, &destination)
            })
            .await?;
            Ok(true)
        }
        NativeDialogResult::Failed(error) => Err(error),
        _ => Err("The export returned an unexpected result.".into()),
    }
}

async fn file_worker<T: Send + 'static>(
    active: ActivePanel,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (send, receive) = oneshot::channel();
    std::thread::Builder::new()
        .name("xrack-android-documents".into())
        .spawn(move || {
            let result = work();
            drop(active);
            let _ = send.send(result);
        })
        .map_err(|error| error.to_string())?;
    receive
        .await
        .map_err(|_| "The Android document worker stopped.")?
}

impl Drop for Pending {
    fn drop(&mut self) {
        if let Ok(mut pending) = PENDING.lock() {
            pending.remove(&self.0);
        }
    }
}

pub(crate) async fn present(
    app: &AndroidApp,
    options: &NativeDialogOptions,
) -> AndroidDialogResult {
    match show(app, options).await {
        Ok(result) => result,
        Err(error) => AndroidDialogResult::Ready(NativeDialogResult::Failed(error)),
    }
}

async fn show(
    app: &AndroidApp,
    options: &NativeDialogOptions,
) -> Result<AndroidDialogResult, String> {
    let (kind, title, text, extensions) = match options {
        NativeDialogOptions::File(options) => {
            let multiple = match options.mode {
                FileDialogMode::OpenFile => false,
                FileDialogMode::OpenFiles => true,
                _ => return Err("The Android file picker currently supports opening files.".into()),
            };
            let mut extensions = options
                .filters()?
                .into_iter()
                .flat_map(|(_, extensions)| extensions)
                .map(str::to_ascii_lowercase)
                .collect::<Vec<_>>();
            if extensions.iter().any(|extension| extension == "*") {
                extensions.clear();
            }
            (
                DialogKind::Document(Picker::Open { multiple }),
                options.title.as_str(),
                String::new(),
                extensions,
            )
        }
        NativeDialogOptions::Message(options) => {
            let text = [options.text.as_str(), options.informative_text.as_str()]
                .into_iter()
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            (
                DialogKind::Message(options.buttons),
                options.title.as_str(),
                text,
                Vec::new(),
            )
        }
    };
    let panel = open(app, kind, title, &text, &extensions).await?;
    match panel.result {
        NativeDialogResult::Files(uris) => {
            let app = app.clone();
            let files = file_worker(panel.active, move || {
                documents::import_files(&app, uris, &extensions)
            })
            .await?;
            Ok(AndroidDialogResult::Imported(files))
        }
        result => Ok(AndroidDialogResult::Ready(result)),
    }
}

async fn open(
    app: &AndroidApp,
    kind: DialogKind,
    title: &str,
    text: &str,
    extensions: &[String],
) -> Result<Panel, String> {
    let active = ActivePanel::begin(app)?;
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    let (send, receive) = oneshot::channel();
    PENDING.lock().map_err(|error| error.to_string())?.insert(
        token,
        PendingReply {
            send,
            kind,
            share: None,
        },
    );
    let _pending = Pending(token);
    crate::android::directory::with_activity(app, |env, activity| {
        let class = env.get_object_class(activity)?;
        // SAFETY: Both callbacks use the instance JNI ABI declared by XRackActivity.
        let methods = unsafe {
            [
                NativeMethod::from_raw_parts(
                    jni_str!("finishDialog"),
                    jni_str!("(JILjava/lang/String;)V"),
                    finish_dialog as *mut std::ffi::c_void,
                ),
                NativeMethod::from_raw_parts(
                    jni_str!("finishDocumentPicker"),
                    jni_str!("(JILandroid/content/Intent;)V"),
                    finish_document_picker as *mut std::ffi::c_void,
                ),
            ]
        };
        // SAFETY: XRackActivity declares both native methods with these descriptors.
        unsafe { env.register_native_methods(&class, &methods)? };
        match kind {
            DialogKind::Document(picker) => {
                let intent = picker.intent(env, title, extensions)?;
                env.call_method(
                    activity,
                    jni_str!("showDocumentPicker"),
                    jni_sig!("(JLandroid/content/Intent;)V"),
                    &[JValue::Long(token), JValue::Object(&intent)],
                )?;
            }
            DialogKind::Message(_) => {
                // SAFETY: These callback ABIs match the methods declared by XRackActivity.
                unsafe { env.register_native_methods(&class, &message::native_methods())? };
                let title = env.new_string(title)?;
                let text = env.new_string(text)?;
                env.call_method(
                    activity,
                    jni_str!("showMessageDialog"),
                    jni_sig!("(JLjava/lang/String;Ljava/lang/String;)V"),
                    &[
                        JValue::Long(token),
                        JValue::Object(title.as_ref()),
                        JValue::Object(text.as_ref()),
                    ],
                )?;
            }
            DialogKind::ShareText => {
                // SAFETY: These callback ABIs match the methods declared by XRackActivity.
                unsafe { env.register_native_methods(&class, &share::native_methods())? };
                let text = env.new_string(text)?;
                env.call_method(
                    activity,
                    jni_str!("showShareDialog"),
                    jni_sig!("(JLjava/lang/String;)V"),
                    &[JValue::Long(token), JValue::Object(text.as_ref())],
                )?;
            }
        }
        Ok(())
    })
    .map_err(|error| format!("The Android dialog bridge could not open the dialog: {error}"))?;
    let result = receive
        .await
        .map_err(|_| "The Android dialog was closed.")?;
    Ok(Panel { result, active })
}

extern "system" fn finish_dialog<'local>(
    mut env: EnvUnowned<'local>,
    _: JObject<'local>,
    token: i64,
    result: i32,
    payload: JString<'local>,
) {
    env.with_env(|env| -> jni::errors::Result<()> {
        let pending = PENDING
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(&token));
        if let Some(pending) = pending {
            let value = match result {
                0 => NativeDialogResult::Canceled,
                2 => NativeDialogResult::Button(StandardButtonKind::Ok),
                _ => NativeDialogResult::Failed(
                    payload
                        .try_to_string(env)
                        .unwrap_or_else(|error| error.to_string()),
                ),
            };
            let _ = pending.send.send(value);
        }
        Ok(())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>();
}

extern "system" fn finish_document_picker<'local>(
    mut env: EnvUnowned<'local>,
    _: JObject<'local>,
    token: i64,
    result: i32,
    data: JObject<'local>,
) {
    const RESULT_OK: i32 = -1;
    env.with_env(|env| -> jni::errors::Result<()> {
        let pending = PENDING.lock().ok().and_then(|mut pending| {
            let DialogKind::Document(picker) = pending.get(&token)?.kind else {
                return None;
            };
            pending.remove(&token).map(|reply| (picker, reply.send))
        });
        if let Some((picker, send)) = pending {
            let value = if result != RESULT_OK || data.is_null() {
                NativeDialogResult::Canceled
            } else {
                match documents::selected_uris(env, &data, matches!(picker, Picker::Open { .. })) {
                    Ok(uris) if uris.is_empty() => NativeDialogResult::Canceled,
                    Ok(uris) => NativeDialogResult::Files(uris),
                    Err(error) => NativeDialogResult::Failed(error.to_string()),
                }
            };
            let _ = send.send(value);
        }
        Ok(())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>();
}
