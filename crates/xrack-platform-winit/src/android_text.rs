use crate::{UserEvent, desktop::WindowEvents, text_indices::byte_offset};
use android_activity::AndroidApp;
use jni::{
    EnvUnowned, JavaVM, NativeMethod, jni_sig, jni_str,
    objects::{JObject, JString, JValue},
    refs::Global,
};
use std::{
    collections::HashMap,
    ops::Range,
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicI64, Ordering},
    },
};
use xrack_layout::Target;
use xrack_runtime::{InputType, Node, ReturnKey, is_multiline_text_input};

static NEXT_TOKEN: AtomicI64 = AtomicI64::new(1);
static EDITORS: LazyLock<Mutex<HashMap<i64, WindowEvents>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) struct Edit {
    token: i64,
    epoch: i64,
    serial: i64,
    pub kind: i32,
    pub value: String,
    pub preedit: String,
    pub selection: Range<usize>,
    pub anchor: Range<usize>,
}

impl Edit {
    pub(crate) fn align_preedit(&mut self) {
        let range = crate::text_indices::cover_graphemes(&self.value, self.anchor.clone());
        if range == self.anchor {
            return;
        }
        let (Some(prefix), Some(suffix)) = (
            self.value.get(range.start..self.anchor.start),
            self.value.get(self.anchor.end..range.end),
        ) else {
            return;
        };
        // Android can compose one scalar of an existing grapheme. Keep its
        // untouched neighbors while replacing the complete grapheme in XRack.
        let mut preedit = String::with_capacity(prefix.len() + self.preedit.len() + suffix.len());
        preedit.push_str(prefix);
        preedit.push_str(&self.preedit);
        preedit.push_str(suffix);
        self.selection = self.selection.start + prefix.len()..self.selection.end + prefix.len();
        self.anchor = range;
        self.preedit = preedit;
    }
}

pub(crate) struct TextInput {
    app: AndroidApp,
    token: i64,
    epoch: i64,
    serial: i64,
    focused: Option<Target>,
    observed: String,
    mode: (i32, i32),
    composing: bool,
    applying: bool,
    keyboard_requested: bool,
}

impl TextInput {
    pub fn new(app: AndroidApp, events: WindowEvents) -> Result<Self, String> {
        // SAFETY: AndroidApp retains its VM and global Activity reference.
        let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
        let raw = app.activity_as_ptr().cast();
        vm.attach_current_thread_for_scope(|env| -> jni::errors::Result<()> {
            // SAFETY: Borrow the reference for this JNI frame without taking ownership.
            let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw)? };
            let class = env.get_object_class(&activity)?;
            // SAFETY: edit_text has the instance JNI ABI and parameters declared here.
            let method = unsafe {
                NativeMethod::from_raw_parts(
                    jni_str!("editText"),
                    jni_str!("(JJJILjava/lang/String;Ljava/lang/String;IIII)V"),
                    edit_text as *mut std::ffi::c_void,
                )
            };
            // SAFETY: ime_insets matches XRackActivity's instance JNI descriptor.
            let geometry = unsafe {
                NativeMethod::from_raw_parts(
                    jni_str!("imeInsets"),
                    jni_str!("(JI)V"),
                    ime_insets as *mut std::ffi::c_void,
                )
            };
            // SAFETY: XRackActivity declares these native methods with the descriptors above.
            unsafe {
                env.register_native_methods(&class, &[method, geometry])?;
            }
            Ok(())
        })
        .map_err(|error| error.to_string())?;
        let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
        EDITORS
            .lock()
            .map_err(|error| error.to_string())?
            .insert(token, events);
        Ok(Self {
            app,
            token,
            epoch: 0,
            serial: 0,
            focused: None,
            observed: String::new(),
            mode: (0, 0),
            composing: false,
            applying: false,
            keyboard_requested: false,
        })
    }

    pub fn focus(
        &mut self,
        target: &Target,
        node: &Node,
        return_key: ReturnKey,
        value: &str,
        selection: Range<usize>,
        composing: bool,
    ) -> bool {
        let mode = input_mode(node, return_key);
        let changed = self.focused.as_ref() != Some(target)
            || self.keyboard_requested
            || self.mode != mode
            || self.observed != value
            || (self.composing && !composing && !self.applying);
        if changed {
            self.keyboard_requested = false;
            self.epoch += 1;
            self.serial = 0;
            self.composing = false;
            self.focused = Some(target.clone());
            self.observed = value.into();
            self.mode = mode;
        }
        if self.applying || self.composing {
            return true;
        }
        self.sync(value, selection, true).is_ok()
    }

    pub fn clear_focus(&mut self) {
        self.keyboard_requested = false;
        if self.focused.take().is_some() {
            self.epoch += 1;
            self.serial = 0;
            self.composing = false;
            let _ = self.sync("", 0..0, false);
        }
    }

    pub fn request_keyboard(&mut self) {
        self.keyboard_requested = true;
    }

    pub fn owns_token(&self, token: i64) -> bool {
        self.token == token
    }

    pub fn accept(&mut self, target: &Target, edit: &Edit) -> bool {
        if self.focused.as_ref() != Some(target)
            || edit.token != self.token
            || edit.epoch != self.epoch
            || edit.serial <= self.serial
        {
            return false;
        }
        self.serial = edit.serial;
        self.observed.clone_from(&edit.value);
        self.composing = edit.kind == 1;
        self.applying = true;
        true
    }

    pub fn finish_event(&mut self) {
        self.applying = false;
    }

    fn sync(&self, value: &str, selection: Range<usize>, active: bool) -> Result<(), String> {
        let start = value[..selection.start].encode_utf16().count() as i32;
        let end = value[..selection.end].encode_utf16().count() as i32;
        // SAFETY: AndroidApp retains the VM and Activity for this call.
        let vm = unsafe { JavaVM::from_raw(self.app.vm_as_ptr().cast()) };
        let raw = self.app.activity_as_ptr().cast();
        vm.attach_current_thread_for_scope(|env| -> jni::errors::Result<()> {
            // SAFETY: The Activity is borrowed for the duration of this JNI frame.
            let activity = unsafe { env.as_cast_raw::<Global<JObject>>(&raw)? };
            let value = env.new_string(value)?;
            env.call_method(
                activity,
                jni_str!("syncTextInput"),
                jni_sig!("(JJJLjava/lang/String;IIIIZ)V"),
                &[
                    JValue::Long(self.token),
                    JValue::Long(self.epoch),
                    JValue::Long(self.serial),
                    JValue::Object(value.as_ref()),
                    JValue::Int(start),
                    JValue::Int(end),
                    JValue::Int(self.mode.0),
                    JValue::Int(self.mode.1),
                    JValue::Bool(active),
                ],
            )?;
            Ok(())
        })
        .map_err(|error| error.to_string())
    }
}

impl Drop for TextInput {
    fn drop(&mut self) {
        self.clear_focus();
        if let Ok(mut editors) = EDITORS.lock() {
            editors.remove(&self.token);
        }
    }
}

fn input_mode(node: &Node, return_key: ReturnKey) -> (i32, i32) {
    let (input_type, default_action) = match (node.kind, node.input_type()) {
        (_, InputType::Password) => (1 | 0x80, 6),
        ("SpinBox", _) | (_, InputType::Number) => (2 | 0x1000, 6),
        ("DoubleSpinBox", _) | (_, InputType::Decimal) => (2 | 0x1000 | 0x2000, 6),
        ("SearchField", _) | (_, InputType::Search) => (1, 3),
        (_, InputType::Email) => (1 | 0x20, 6),
        (_, InputType::Url) => (1 | 0x10, 6),
        (_, InputType::Tel) => (3, 6),
        (kind, _) if is_multiline_text_input(kind) => (1 | 0x20000, 1),
        _ => (1, 6),
    };
    let action = match return_key {
        ReturnKey::Auto => default_action,
        ReturnKey::Next => 5, // EditorInfo.IME_ACTION_NEXT
        ReturnKey::Done => 6, // EditorInfo.IME_ACTION_DONE
    };
    (input_type, action)
}

extern "system" fn ime_insets<'local>(
    _env: EnvUnowned<'local>,
    _activity: JObject<'local>,
    token: i64,
    top: i32,
) {
    let events = EDITORS
        .lock()
        .ok()
        .and_then(|editors| editors.get(&token).cloned());
    if let Some(events) = events {
        events.post(UserEvent::AndroidImeGeometry {
            token,
            top: u32::try_from(top).ok(),
        });
    }
}

extern "system" fn edit_text<'local>(
    mut env: EnvUnowned<'local>,
    _: JObject<'local>,
    token: i64,
    epoch: i64,
    serial: i64,
    kind: i32,
    value: JString<'local>,
    preedit: JString<'local>,
    start: i32,
    end: i32,
    anchor_start: i32,
    anchor_end: i32,
) {
    env.with_env(|env| -> jni::errors::Result<()> {
        let events = EDITORS
            .lock()
            .ok()
            .and_then(|editors| editors.get(&token).cloned());
        if let Some(events) = events {
            let value = value.try_to_string(env)?;
            let preedit = preedit.try_to_string(env)?;
            let selected = if kind == 1 { &preedit } else { &value };
            let selection = byte_offset(selected, start.max(0) as usize)
                ..byte_offset(selected, end.max(0) as usize);
            let anchor = byte_offset(&value, anchor_start.max(0) as usize)
                ..byte_offset(&value, anchor_end.max(0) as usize);
            events.post(UserEvent::AndroidText(Edit {
                token,
                epoch,
                serial,
                kind,
                value,
                preedit,
                selection,
                anchor,
            }));
        }
        Ok(())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>();
}
