use android_activity::AndroidApp;
use jni::{
    Env, JavaVM, jni_sig, jni_str,
    objects::{JClass, JObject, JString, JValue},
    refs::Global,
};

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
            let directory = parse_uri(env, &self.uri)?;
            let contract = env.find_class(jni_str!("android/provider/DocumentsContract"))?;
            let cursor = query_children(env, activity, &contract, &directory)?;
            if cursor.is_null() {
                return Err(jni::errors::Error::NullPtr("ContentResolver.query"));
            }
            let entries = self.read_entries(env, &contract, &directory, &cursor);
            let (entries, closed) = close_resource(env, &cursor, entries);
            let documents = entries?;
            closed?;
            Ok(documents)
        })
    }

    fn read_entries(
        &self,
        env: &mut Env<'_>,
        contract: &JClass<'_>,
        directory: &JObject<'_>,
        cursor: &JObject<'_>,
    ) -> jni::errors::Result<Vec<AndroidDocument>> {
        let mut entries = Vec::new();
        while env
            .call_method(cursor, jni_str!("moveToNext"), jni_sig!("()Z"), &[])?
            .z()?
        {
            let entry = env.with_local_frame(8, |env| -> jni::errors::Result<_> {
                let id = cursor_string(env, cursor, 0)?;
                let name = cursor_string(env, cursor, 1)?.try_to_string(env)?;
                let mime = cursor_string(env, cursor, 2)?.try_to_string(env)?;
                let uri = env
                    .call_static_method(
                        contract,
                        jni_str!("buildDocumentUriUsingTree"),
                        jni_sig!("(Landroid/net/Uri;Ljava/lang/String;)Landroid/net/Uri;"),
                        &[JValue::Object(directory), JValue::Object(id.as_ref())],
                    )?
                    .l()?;
                let uri = env
                    .call_method(
                        &uri,
                        jni_str!("toString"),
                        jni_sig!("()Ljava/lang/String;"),
                        &[],
                    )?
                    .l()?;
                Ok(AndroidDocument {
                    app: self.app.clone(),
                    uri: env.cast_local::<JString>(uri)?.try_to_string(env)?,
                    name,
                    directory: mime == "vnd.android.document/directory",
                })
            })?;
            entries.push(entry);
        }
        Ok(entries)
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
        i32::try_from(max_bytes)
            .map_err(|_| "The document byte limit exceeds Android's supported range.")?;
        let bytes = with_activity(&self.app, |env, activity| {
            let uri = parse_uri(env, &self.uri)?;
            let resolver = content_resolver(env, activity)?;
            let stream = env
                .call_method(
                    &resolver,
                    jni_str!("openInputStream"),
                    jni_sig!("(Landroid/net/Uri;)Ljava/io/InputStream;"),
                    &[JValue::Object(&uri)],
                )?
                .l()?;
            if stream.is_null() {
                return Err(jni::errors::Error::NullPtr(
                    "ContentResolver.openInputStream",
                ));
            }
            let read = read_stream(env, &stream, max_bytes);
            let (read, closed) = close_resource(env, &stream, read);
            let bytes = read?;
            if bytes.is_some() {
                closed?;
            }
            Ok(bytes)
        })?
        .ok_or("The document exceeds the requested byte limit.")?;
        String::from_utf8(bytes).map_err(|error| error.to_string())
    }
}

fn query_children<'local>(
    env: &mut Env<'local>,
    activity: &JObject<'_>,
    contract: &JClass<'_>,
    directory: &JObject<'local>,
) -> jni::errors::Result<JObject<'local>> {
    let document = env
        .call_static_method(
            contract,
            jni_str!("isDocumentUri"),
            jni_sig!("(Landroid/content/Context;Landroid/net/Uri;)Z"),
            &[JValue::Object(activity), JValue::Object(directory)],
        )?
        .z()?;
    let id_method = if document {
        jni_str!("getDocumentId")
    } else {
        jni_str!("getTreeDocumentId")
    };
    let id = env
        .call_static_method(
            contract,
            id_method,
            jni_sig!("(Landroid/net/Uri;)Ljava/lang/String;"),
            &[JValue::Object(directory)],
        )?
        .l()?;
    let uri = env
        .call_static_method(
            contract,
            jni_str!("buildChildDocumentsUriUsingTree"),
            jni_sig!("(Landroid/net/Uri;Ljava/lang/String;)Landroid/net/Uri;"),
            &[JValue::Object(directory), JValue::Object(&id)],
        )?
        .l()?;
    let projection = env.new_object_array(3, jni_str!("java/lang/String"), JObject::null())?;
    // DocumentsContract.Document column names in the order used by read_entries.
    for (index, column) in ["document_id", "_display_name", "mime_type"]
        .into_iter()
        .enumerate()
    {
        let value = env.new_string(column)?;
        projection.set_element(env, index, &value)?;
    }
    let resolver = content_resolver(env, activity)?;
    let null = JObject::null();
    env.call_method(
        &resolver,
        jni_str!("query"),
        jni_sig!("(Landroid/net/Uri;[Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;Ljava/lang/String;)Landroid/database/Cursor;"),
        &[
            JValue::Object(&uri),
            JValue::Object(projection.as_ref()),
            JValue::Object(&null),
            JValue::Object(&null),
            JValue::Object(&null),
        ],
    )?
    .l()
}

fn cursor_string<'local>(
    env: &mut Env<'local>,
    cursor: &JObject<'_>,
    column: i32,
) -> jni::errors::Result<JString<'local>> {
    let value = env
        .call_method(
            cursor,
            jni_str!("getString"),
            jni_sig!("(I)Ljava/lang/String;"),
            &[JValue::Int(column)],
        )?
        .l()?;
    env.cast_local::<JString>(value)
}

pub(crate) fn parse_uri<'local>(
    env: &mut Env<'local>,
    location: &str,
) -> jni::errors::Result<JObject<'local>> {
    let location = env.new_string(location)?;
    env.call_static_method(
        jni_str!("android/net/Uri"),
        jni_str!("parse"),
        jni_sig!("(Ljava/lang/String;)Landroid/net/Uri;"),
        &[JValue::Object(location.as_ref())],
    )?
    .l()
}

pub(crate) fn content_resolver<'local>(
    env: &mut Env<'local>,
    activity: &JObject<'_>,
) -> jni::errors::Result<JObject<'local>> {
    env.call_method(
        activity,
        jni_str!("getContentResolver"),
        jni_sig!("()Landroid/content/ContentResolver;"),
        &[],
    )?
    .l()
}

pub(crate) fn close_resource<T>(
    env: &mut Env<'_>,
    resource: &JObject<'_>,
    operation: jni::errors::Result<T>,
) -> (jni::errors::Result<T>, jni::errors::Result<()>) {
    // Clear a provider exception before making the close call.
    let operation = env.exception_catch().and(operation);
    let closed = env.call_method(resource, jni_str!("close"), jni_sig!("()V"), &[]);
    (operation, env.exception_catch().and(closed).map(|_| ()))
}

fn read_stream<'local>(
    env: &mut Env<'local>,
    stream: &JObject<'local>,
    max_bytes: usize,
) -> jni::errors::Result<Option<Vec<u8>>> {
    let mut chunk = [0_i8; 8192];
    let buffer = env.new_byte_array(chunk.len())?;
    let mut bytes = Vec::new();
    loop {
        let count = env
            .call_method(
                stream,
                jni_str!("read"),
                jni_sig!("([B)I"),
                &[JValue::Object(buffer.as_ref())],
            )?
            .i()?;
        if count == -1 {
            return Ok(Some(bytes));
        }
        let part = chunk
            .get_mut(..count as usize)
            .filter(|part| !part.is_empty())
            .ok_or(jni::errors::Error::JniCall(
                jni::errors::JniError::InvalidArguments,
            ))?;
        if part.len() > max_bytes - bytes.len() {
            return Ok(None);
        }
        buffer.get_region(env, 0, part)?;
        bytes.extend(part.iter().map(|byte| *byte as u8));
    }
}

pub(crate) fn with_activity<T>(
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
