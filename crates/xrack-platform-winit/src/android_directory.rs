use jni::{
    Env, JavaVM, jni_sig, jni_str,
    objects::{JByteArray, JObject, JObjectArray, JString, JValue},
    refs::Global,
};
use winit::platform::android::activity::AndroidApp;

/// Access to a directory selected with Android's Storage Access Framework.
/// Content remains in its document provider. Run document I/O on a worker thread.
#[derive(Clone)]
pub struct AndroidDirectory {
    app: AndroidApp,
    uri: String,
}

/// A document exposed by an authorized Android directory.
#[derive(Clone)]
pub struct AndroidDocument {
    app: AndroidApp,
    uri: String,
    name: String,
    directory: bool,
}

impl AndroidDirectory {
    pub(crate) fn new(app: AndroidApp, uri: String) -> Self {
        Self { app, uri }
    }

    /// List the immediate children of the selected directory.
    pub fn entries(&self) -> Result<Vec<AndroidDocument>, String> {
        with_activity(&self.app, |env, activity| {
            let uri = env.new_string(&self.uri)?;
            let result = env
                .call_method(
                    activity,
                    jni_str!("listDocuments"),
                    jni_sig!("(Ljava/lang/String;)[Ljava/lang/String;"),
                    &[JValue::Object(uri.as_ref())],
                )?
                .l()?;
            let entries = env.cast_local::<JObjectArray>(result)?;
            let mut documents = Vec::new();
            for index in (0..entries.len(env)?).step_by(3) {
                let mut fields = Vec::with_capacity(3);
                for offset in 0..3 {
                    let value = entries.get_element(env, index + offset)?;
                    let value = env.cast_local::<JString>(value)?;
                    fields.push(value.try_to_string(env)?);
                    env.delete_local_ref(value);
                }
                let mut fields = fields.into_iter();
                if let (Some(name), Some(uri), Some(mime)) =
                    (fields.next(), fields.next(), fields.next())
                {
                    documents.push(AndroidDocument {
                        app: self.app.clone(),
                        uri,
                        name,
                        directory: mime == "vnd.android.document/directory",
                    });
                }
            }
            Ok(documents)
        })
    }
}

impl AndroidDocument {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn is_directory(&self) -> bool {
        self.directory
    }

    /// Access the child directory with the original picker authorization.
    pub fn directory(&self) -> Option<AndroidDirectory> {
        self.directory
            .then(|| AndroidDirectory::new(self.app.clone(), self.uri.clone()))
    }

    /// Read UTF-8 content, failing if it exceeds the application's byte limit.
    pub fn read_to_string(&self, max_bytes: usize) -> Result<String, String> {
        if self.directory {
            return Err("The selected document is a directory.".into());
        }
        let limit = i32::try_from(max_bytes)
            .map_err(|_| "The document byte limit exceeds Android's supported range.")?;
        let bytes = with_activity(&self.app, |env, activity| {
            let uri = env.new_string(&self.uri)?;
            let value = env
                .call_method(
                    activity,
                    jni_str!("readDocument"),
                    jni_sig!("(Ljava/lang/String;I)[B"),
                    &[JValue::Object(uri.as_ref()), JValue::Int(limit)],
                )?
                .l()?;
            let array = env.cast_local::<JByteArray>(value)?;
            env.convert_byte_array(&array)
        })?;
        String::from_utf8(bytes).map_err(|error| error.to_string())
    }
}

fn with_activity<T>(
    app: &AndroidApp,
    action: impl FnOnce(&mut Env<'_>, &JObject<'_>) -> jni::errors::Result<T>,
) -> Result<T, String> {
    // SAFETY: AndroidApp retains its VM and the Activity global reference.
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let raw_activity = app.activity_as_ptr().cast();
    vm.attach_current_thread_for_scope(|env| -> jni::errors::Result<_> {
        // SAFETY: Borrow the Activity reference for this JNI frame.
        let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw_activity)? };
        action(env, activity.as_ref())
    })
    .map_err(|error| match error {
        jni::errors::Error::CaughtJavaException { msg, .. } if !msg.is_empty() => msg,
        error => error.to_string(),
    })
}
