use jni::{
    EnvUnowned, JavaVM, NativeMethod, jni_sig, jni_str,
    objects::{JObject, JString, JValue},
    refs::Global,
};
use std::{
    collections::HashMap,
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicI64, Ordering},
    },
};
use tokio::sync::oneshot;
use winit::platform::android::activity::AndroidApp;
use xrack_runtime::{
    FileDialogMode, MessageButtons, NativeDialogOptions, NativeDialogResult, StandardButtonKind,
};

static NEXT_TOKEN: AtomicI64 = AtomicI64::new(1);
static PENDING: LazyLock<Mutex<HashMap<i64, oneshot::Sender<NativeDialogResult>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

struct Pending(i64);

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
                let result = match open(&app, 8, "", &text, &[]).await {
                    Ok(NativeDialogResult::Canceled) => Ok(false),
                    Ok(NativeDialogResult::Button(StandardButtonKind::Ok)) => Ok(true),
                    Ok(NativeDialogResult::Failed(error)) | Err(error) => Err(error),
                    _ => Err("The share sheet returned an unexpected result.".into()),
                };
                let _ = reply.send(result);
            }
            Self::ExportFile { path, reply } => {
                let _ = reply.send(export_file(&app, &path).await);
            }
            Self::SelectDirectory { reply } => {
                let result = match open(&app, 6, "", "", &[]).await {
                    Ok(NativeDialogResult::Canceled) => Ok(None),
                    Ok(NativeDialogResult::Files(paths)) => paths
                        .into_iter()
                        .next()
                        .map(|uri| Some(crate::AndroidDirectory::new(app, uri)))
                        .ok_or_else(|| "The directory picker returned no directory.".into()),
                    Ok(NativeDialogResult::Failed(error)) | Err(error) => Err(error),
                    _ => Err("The directory picker returned an unexpected result.".into()),
                };
                let _ = reply.send(result);
            }
        }
    }
}

async fn export_file(app: &AndroidApp, path: &std::path::Path) -> Result<bool, String> {
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
    match open(app, 7, name, source, &[]).await? {
        NativeDialogResult::Canceled => Ok(false),
        NativeDialogResult::Button(StandardButtonKind::Ok) => Ok(true),
        NativeDialogResult::Failed(error) => Err(error),
        _ => Err("The export returned an unexpected result.".into()),
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if let Ok(mut pending) = PENDING.lock() {
            pending.remove(&self.0);
        }
    }
}

pub(crate) async fn present(app: &AndroidApp, options: &NativeDialogOptions) -> NativeDialogResult {
    match show(app, options).await {
        Ok(result) => result,
        Err(error) => NativeDialogResult::Failed(error),
    }
}

async fn show(
    app: &AndroidApp,
    options: &NativeDialogOptions,
) -> Result<NativeDialogResult, String> {
    let (kind, title, text, extensions) = match options {
        NativeDialogOptions::File(options) => {
            let kind = match options.mode {
                FileDialogMode::OpenFile => 0,
                FileDialogMode::OpenFiles => 1,
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
            (kind, options.title.as_str(), String::new(), extensions)
        }
        NativeDialogOptions::Message(options) => {
            let kind = match options.buttons {
                MessageButtons::Ok => 2,
                MessageButtons::OkCancel => 3,
                MessageButtons::YesNo => 4,
                MessageButtons::YesNoCancel => 5,
            };
            let text = [options.text.as_str(), options.informative_text.as_str()]
                .into_iter()
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            (kind, options.title.as_str(), text, Vec::new())
        }
    };
    open(app, kind, title, &text, &extensions).await
}

async fn open(
    app: &AndroidApp,
    kind: i32,
    title: &str,
    text: &str,
    extensions: &[String],
) -> Result<NativeDialogResult, String> {
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    let (send, receive) = oneshot::channel();
    PENDING
        .lock()
        .map_err(|error| error.to_string())?
        .insert(token, send);
    let _pending = Pending(token);
    // SAFETY: AndroidApp owns the VM association and the global Activity reference.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let raw_activity = app.activity_as_ptr().cast();
    vm.attach_current_thread_for_scope(|env| -> jni::errors::Result<()> {
        // SAFETY: The reference is borrowed for this JNI frame and is not released here.
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity)? };
        let class = env.get_object_class(&activity)?;
        // SAFETY: finish_dialog uses the instance JNI ABI matching this descriptor.
        let finish = unsafe {
            NativeMethod::from_raw_parts(
                jni_str!("finishDialog"),
                jni_str!("(JILjava/lang/String;)V"),
                finish_dialog as *mut std::ffi::c_void,
            )
        };
        // SAFETY: The private native method on XRackActivity has the descriptor above.
        unsafe { env.register_native_methods(&class, &[finish])? };
        let title = env.new_string(title)?;
        let text = env.new_string(text)?;
        let count = i32::try_from(extensions.len())
            .map_err(|_| jni::errors::Error::JniCall(jni::errors::JniError::InvalidArguments))?;
        let array = env.new_object_array(count, jni_str!("java/lang/String"), JObject::null())?;
        for (index, extension) in extensions.iter().enumerate() {
            let value = env.new_string(extension)?;
            array.set_element(env, index, &value)?;
            env.delete_local_ref(value);
        }
        env.call_method(
            activity,
            jni_str!("showDialog"),
            jni_sig!("(JILjava/lang/String;Ljava/lang/String;[Ljava/lang/String;)V"),
            &[
                JValue::Long(token),
                JValue::Int(kind),
                JValue::Object(title.as_ref()),
                JValue::Object(text.as_ref()),
                JValue::Object(array.as_ref()),
            ],
        )?;
        Ok(())
    })
    .map_err(|error| format!("The Android dialog bridge could not open the dialog: {error}"))?;
    receive
        .await
        .map_err(|_| "The Android dialog was closed.".into())
}

extern "system" fn finish_dialog<'local>(
    mut env: EnvUnowned<'local>,
    _: JObject<'local>,
    token: i64,
    result: i32,
    payload: JString<'local>,
) {
    env.with_env(|env| -> jni::errors::Result<()> {
        let send = PENDING
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(&token));
        if let Some(send) = send {
            let value = match result {
                0 => NativeDialogResult::Canceled,
                1 => match payload.try_to_string(env) {
                    Ok(paths) => {
                        NativeDialogResult::Files(paths.split('\0').map(str::to_owned).collect())
                    }
                    Err(error) => NativeDialogResult::Failed(error.to_string()),
                },
                2 => NativeDialogResult::Button(StandardButtonKind::Ok),
                3 => NativeDialogResult::Button(StandardButtonKind::Yes),
                4 => NativeDialogResult::Button(StandardButtonKind::No),
                5 => NativeDialogResult::Button(StandardButtonKind::Cancel),
                _ => NativeDialogResult::Failed(
                    payload
                        .try_to_string(env)
                        .unwrap_or_else(|error| error.to_string()),
                ),
            };
            let _ = send.send(value);
        }
        Ok(())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>();
}
