use super::intent::Intent;
use crate::android_directory::{close_resource, content_resolver, parse_uri, with_activity};
use android_activity::AndroidApp;
use jni::{
    Env, bind_java_type, jni_sig, jni_str,
    objects::{JObject, JObjectArray, JString, JValue},
};
use std::collections::{BTreeSet, HashSet};
use tempfile::TempDir;

bind_java_type! {
    MimeTypeMap => android.webkit.MimeTypeMap,
    methods {
        static fn get_singleton() -> MimeTypeMap,
        fn get_mime_type_from_extension(extension: JString) -> JString,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Picker {
    Open { multiple: bool },
    Directory,
    Create,
}

impl Picker {
    pub(super) fn intent<'local>(
        self,
        env: &mut Env<'local>,
        title: &str,
        extensions: &[String],
    ) -> jni::errors::Result<JObject<'local>> {
        const GRANT_READ_URI_PERMISSION: i32 = 0x01;
        const GRANT_WRITE_URI_PERMISSION: i32 = 0x02;
        let (action, flags) = match self {
            Self::Open { .. } => (
                "android.intent.action.OPEN_DOCUMENT",
                GRANT_READ_URI_PERMISSION,
            ),
            Self::Directory => (
                "android.intent.action.OPEN_DOCUMENT_TREE",
                GRANT_READ_URI_PERMISSION,
            ),
            Self::Create => (
                "android.intent.action.CREATE_DOCUMENT",
                GRANT_WRITE_URI_PERMISSION,
            ),
        };
        let action = env.new_string(action)?;
        let intent = Intent::new(env, &action)?;
        intent.add_flags(env, flags)?;
        if self == Self::Directory {
            return Ok(intent.into());
        }

        let openable = env.new_string("android.intent.category.OPENABLE")?;
        intent.add_category(env, &openable)?;
        if !title.is_empty() {
            let key = env.new_string("android.intent.extra.TITLE")?;
            let title = env.new_string(title)?;
            intent.put_string(env, &key, &title)?;
        }
        if let Self::Open { multiple } = self {
            let key = env.new_string("android.intent.extra.ALLOW_MULTIPLE")?;
            intent.put_boolean(env, &key, multiple)?;
            let types = mime_types(env, extensions)?;
            let mime = match types.as_slice() {
                [only] => only.as_str(),
                _ => "*/*",
            };
            let mime = env.new_string(mime)?;
            intent.set_type(env, &mime)?;
            if types.len() > 1 {
                let key = env.new_string("android.intent.extra.MIME_TYPES")?;
                let array = JObjectArray::<JString>::new(env, types.len(), JString::null())?;
                for (index, value) in types.iter().enumerate() {
                    let value = env.new_string(value)?;
                    array.set_element(env, index, &value)?;
                    env.delete_local_ref(value);
                }
                intent.put_strings(env, &key, &array)?;
            }
        } else {
            let mime = env.new_string("application/octet-stream")?;
            intent.set_type(env, &mime)?;
        }
        Ok(intent.into())
    }
}

fn mime_types(env: &mut Env<'_>, extensions: &[String]) -> jni::errors::Result<Vec<String>> {
    if extensions.is_empty() {
        return Ok(Vec::new());
    }
    let map = MimeTypeMap::get_singleton(env)?;
    let mut types = BTreeSet::new();
    for extension in extensions {
        let mime = env.with_local_frame(8, |env| {
            let extension = env.new_string(extension)?;
            let mime = map.get_mime_type_from_extension(env, &extension)?;
            if mime.is_null() {
                return Ok(None);
            }
            mime.try_to_string(env).map(Some)
        })?;
        let Some(mime) = mime else {
            // Keep unregistered extensions selectable. Import still checks the file name.
            return Ok(Vec::new());
        };
        types.insert(mime);
    }
    Ok(types.into_iter().collect())
}

pub(crate) struct ImportedFiles {
    directory: TempDir,
    pub(crate) paths: Vec<String>,
}

impl ImportedFiles {
    pub(crate) fn keep(self) {
        let _ = self.directory.keep();
    }
}

pub(super) fn selected_uris(
    env: &mut Env<'_>,
    intent: &JObject<'_>,
    accepts_clip_data: bool,
) -> jni::errors::Result<Vec<String>> {
    if accepts_clip_data {
        let clip = env
            .call_method(
                intent,
                jni_str!("getClipData"),
                jni_sig!("()Landroid/content/ClipData;"),
                &[],
            )?
            .l()?;
        if !clip.is_null() {
            let count = env
                .call_method(&clip, jni_str!("getItemCount"), jni_sig!("()I"), &[])?
                .i()?;
            let mut uris = Vec::new();
            for index in 0..count {
                let uri = env.with_local_frame(8, |env| {
                    let item = env
                        .call_method(
                            &clip,
                            jni_str!("getItemAt"),
                            jni_sig!("(I)Landroid/content/ClipData$Item;"),
                            &[JValue::Int(index)],
                        )?
                        .l()?;
                    let uri = env
                        .call_method(
                            &item,
                            jni_str!("getUri"),
                            jni_sig!("()Landroid/net/Uri;"),
                            &[],
                        )?
                        .l()?;
                    uri_string(env, &uri)
                })?;
                uris.extend(uri);
            }
            return Ok(uris);
        }
    }
    let uri = env
        .call_method(
            intent,
            jni_str!("getData"),
            jni_sig!("()Landroid/net/Uri;"),
            &[],
        )?
        .l()?;
    Ok(uri_string(env, &uri)?.into_iter().collect())
}

fn uri_string(env: &mut Env<'_>, uri: &JObject<'_>) -> jni::errors::Result<Option<String>> {
    if uri.is_null() {
        return Ok(None);
    }
    let value = env
        .call_method(
            uri,
            jni_str!("toString"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .l()?;
    env.cast_local::<JString>(value)?
        .try_to_string(env)
        .map(Some)
}

pub(super) fn import_files(
    app: &AndroidApp,
    uris: Vec<String>,
    extensions: &[String],
) -> Result<ImportedFiles, String> {
    let cache = with_activity(app, |env, activity| {
        let file = env
            .call_method(
                activity,
                jni_str!("getCacheDir"),
                jni_sig!("()Ljava/io/File;"),
                &[],
            )?
            .l()?;
        let path = env
            .call_method(
                &file,
                jni_str!("getAbsolutePath"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )?
            .l()?;
        env.cast_local::<JString>(path)?.try_to_string(env)
    })?;
    let directory = tempfile::Builder::new()
        .prefix("xrack-file-")
        .tempdir_in(cache)
        .map_err(|error| error.to_string())?;
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    for location in uris {
        if !seen.insert(location.clone()) {
            continue;
        }
        let name = with_activity(app, |env, activity| {
            let uri = parse_uri(env, &location)?;
            let resolver = content_resolver(env, activity)?;
            display_name(env, &resolver, &uri)
        })?;
        let name = name
            .filter(|name| !matches!(name.as_str(), "" | "." | ".."))
            .unwrap_or_else(|| "document".into())
            .replace(['/', '\\', '\0'], "_");
        let lower = name.to_lowercase();
        if !extensions.is_empty()
            && !extensions.iter().any(|extension| {
                lower
                    .strip_suffix(extension)
                    .is_some_and(|prefix| prefix.ends_with('.'))
            })
        {
            return Err("The selected document does not match the requested file types.".into());
        }
        // Each file keeps its display name, even when two providers return the same one.
        let subdirectory = directory.path().join(paths.len().to_string());
        std::fs::create_dir(&subdirectory).map_err(|error| error.to_string())?;
        let path = subdirectory
            .join(name)
            .into_os_string()
            .into_string()
            .map_err(|_| "The document cache path cannot be represented as UTF-8.")?;
        with_activity(app, |env, activity| {
            let uri = parse_uri(env, &location)?;
            let resolver = content_resolver(env, activity)?;
            let target = java_path(env, &path)?;
            let options =
                env.new_object_array(0, jni_str!("java/nio/file/CopyOption"), JObject::null())?;
            let input = env
                .call_method(
                    &resolver,
                    jni_str!("openInputStream"),
                    jni_sig!("(Landroid/net/Uri;)Ljava/io/InputStream;"),
                    &[JValue::Object(&uri)],
                )?
                .l()?;
            if input.is_null() {
                return Err(jni::errors::Error::NullPtr(
                    "ContentResolver.openInputStream",
                ));
            }
            let copy = env.call_static_method(
                jni_str!("java/nio/file/Files"),
                jni_str!("copy"),
                jni_sig!("(Ljava/io/InputStream;Ljava/nio/file/Path;[Ljava/nio/file/CopyOption;)J"),
                &[
                    JValue::Object(&input),
                    JValue::Object(&target),
                    JValue::Object(options.as_ref()),
                ],
            );
            let (copy, close) = close_resource(env, &input, copy);
            copy?;
            close
        })?;
        paths.push(path);
    }
    Ok(ImportedFiles { directory, paths })
}

pub(super) fn export_file(app: &AndroidApp, source: &str, destination: &str) -> Result<(), String> {
    with_activity(app, |env, activity| {
        let uri = parse_uri(env, destination)?;
        let resolver = content_resolver(env, activity)?;
        let source = java_path(env, source)?;
        let mode = env.new_string("wt")?;
        let output = env
            .call_method(
                &resolver,
                jni_str!("openOutputStream"),
                jni_sig!("(Landroid/net/Uri;Ljava/lang/String;)Ljava/io/OutputStream;"),
                &[JValue::Object(&uri), JValue::Object(mode.as_ref())],
            )?
            .l()?;
        if output.is_null() {
            return Err(jni::errors::Error::NullPtr(
                "ContentResolver.openOutputStream",
            ));
        }
        let copy = env.call_static_method(
            jni_str!("java/nio/file/Files"),
            jni_str!("copy"),
            jni_sig!("(Ljava/nio/file/Path;Ljava/io/OutputStream;)J"),
            &[JValue::Object(&source), JValue::Object(&output)],
        );
        let (copy, close) = close_resource(env, &output, copy);
        copy?;
        close
    })
}

fn display_name(
    env: &mut Env<'_>,
    resolver: &JObject<'_>,
    uri: &JObject<'_>,
) -> jni::errors::Result<Option<String>> {
    let name = env.new_string("_display_name")?;
    let projection = env.new_object_array(1, jni_str!("java/lang/String"), &name)?;
    let null = JObject::null();
    let cursor = env.call_method(
        resolver,
        jni_str!("query"),
        jni_sig!("(Landroid/net/Uri;[Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;Ljava/lang/String;)Landroid/database/Cursor;"),
        &[
            JValue::Object(uri),
            JValue::Object(projection.as_ref()),
            JValue::Object(&null),
            JValue::Object(&null),
            JValue::Object(&null),
        ],
    )?.l()?;
    if cursor.is_null() {
        return Ok(None);
    }
    let read = (|| {
        if !env
            .call_method(&cursor, jni_str!("moveToFirst"), jni_sig!("()Z"), &[])?
            .z()?
        {
            return Ok(None);
        }
        let value = env
            .call_method(
                &cursor,
                jni_str!("getString"),
                jni_sig!("(I)Ljava/lang/String;"),
                &[JValue::Int(0)],
            )?
            .l()?;
        if value.is_null() {
            return Ok(None);
        }
        env.cast_local::<JString>(value)?
            .try_to_string(env)
            .map(Some)
    })();
    let (read, close) = close_resource(env, &cursor, read);
    let name = read?;
    close?;
    Ok(name)
}

fn java_path<'local>(env: &mut Env<'local>, path: &str) -> jni::errors::Result<JObject<'local>> {
    let path = env.new_string(path)?;
    let file = env.new_object(
        jni_str!("java/io/File"),
        jni_sig!("(Ljava/lang/String;)V"),
        &[JValue::Object(path.as_ref())],
    )?;
    env.call_method(
        &file,
        jni_str!("toPath"),
        jni_sig!("()Ljava/nio/file/Path;"),
        &[],
    )?
    .l()
}
