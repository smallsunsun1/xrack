use super::{DialogKind, PENDING, intent::Context};
use jni::{
    EnvUnowned, NativeMethod, bind_java_type, jni_sig, jni_str,
    objects::{JObject, JString},
    sys::jboolean,
};
use xrack_runtime::{MessageButtons, NativeDialogResult, StandardButtonKind};

bind_java_type! { OnClickListener => "android.content.DialogInterface$OnClickListener" }
bind_java_type! { OnCancelListener => "android.content.DialogInterface$OnCancelListener" }
bind_java_type! { AlertDialog => android.app.AlertDialog }

bind_java_type! {
    AlertDialogBuilder => "android.app.AlertDialog$Builder",
    type_map {
        Context => android.content.Context,
        OnClickListener => "android.content.DialogInterface$OnClickListener",
        OnCancelListener => "android.content.DialogInterface$OnCancelListener",
        AlertDialog => android.app.AlertDialog,
    },
    constructors { fn new(context: Context) },
    methods {
        fn set_title(title: JCharSequence) -> AlertDialogBuilder,
        fn set_message(text: JCharSequence) -> AlertDialogBuilder,
        fn set_positive_button(label: jint, listener: OnClickListener) -> AlertDialogBuilder,
        fn set_negative_button(label: jint, listener: OnClickListener) -> AlertDialogBuilder,
        fn set_neutral_button(label: jint, listener: OnClickListener) -> AlertDialogBuilder,
        fn set_on_cancel_listener(listener: OnCancelListener) -> AlertDialogBuilder,
        fn show() -> AlertDialog,
    }
}

pub(super) fn native_methods() -> [NativeMethod<'static>; 2] {
    // SAFETY: Both methods use the instance JNI ABI declared by XRackActivity.
    unsafe {
        [
            NativeMethod::from_raw_parts(
                jni_str!("launchMessage"),
                jni_str!(
                    "(JLjava/lang/String;Ljava/lang/String;Ldev/xrack/platform/XRackActivity$MessageListener;II)Z"
                ),
                launch as *mut std::ffi::c_void,
            ),
            NativeMethod::from_raw_parts(
                jni_str!("finishMessage"),
                jni_str!("(JI)V"),
                finish as *mut std::ffi::c_void,
            ),
        ]
    }
}

fn is_question(buttons: MessageButtons) -> bool {
    matches!(buttons, MessageButtons::YesNo | MessageButtons::YesNoCancel)
}

fn has_cancel_button(buttons: MessageButtons) -> bool {
    matches!(
        buttons,
        MessageButtons::OkCancel | MessageButtons::YesNoCancel
    )
}

extern "system" fn launch<'local>(
    mut env: EnvUnowned<'local>,
    activity: JObject<'local>,
    token: i64,
    title: JString<'local>,
    text: JString<'local>,
    listener: JObject<'local>,
    yes_label: i32,
    no_label: i32,
) -> jboolean {
    env.with_env(|env| -> jni::errors::Result<bool> {
        let buttons = PENDING.lock().ok().and_then(|pending| {
            let reply = pending
                .get(&token)
                .filter(|reply| !reply.send.is_closed())?;
            match reply.kind {
                DialogKind::Message(buttons) => Some(buttons),
                _ => None,
            }
        });
        let Some(buttons) = buttons else {
            return Ok(false);
        };
        let context = env.as_cast::<Context>(&activity)?;
        let click = env.as_cast::<OnClickListener>(&listener)?;
        let cancel = env.as_cast::<OnCancelListener>(&listener)?;
        let builder = AlertDialogBuilder::new(env, &*context)?;
        builder.set_title(env, &title)?;
        builder.set_message(env, &text)?;
        let question = is_question(buttons);
        let positive_label = if question {
            yes_label
        } else {
            env.get_static_field(jni_str!("android/R$string"), jni_str!("ok"), jni_sig!("I"))?
                .i()?
        };
        builder.set_positive_button(env, positive_label, &*click)?;
        if question {
            builder.set_negative_button(env, no_label, &*click)?;
        }
        if has_cancel_button(buttons) {
            let label = env
                .get_static_field(
                    jni_str!("android/R$string"),
                    jni_str!("cancel"),
                    jni_sig!("I"),
                )?
                .i()?;
            builder.set_neutral_button(env, label, &*click)?;
        }
        builder.set_on_cancel_listener(env, &*cancel)?;
        builder.show(env)?;
        Ok(true)
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

extern "system" fn finish(mut env: EnvUnowned<'_>, _: JObject<'_>, token: i64, button: i32) {
    const POSITIVE: i32 = -1;
    const NEGATIVE: i32 = -2;
    const NEUTRAL: i32 = -3;
    const CANCELED: i32 = 0;

    env.with_env(|_| -> jni::errors::Result<()> {
        let reply = PENDING.lock().ok().and_then(|mut pending| {
            let DialogKind::Message(buttons) = pending.get(&token)?.kind else {
                return None;
            };
            pending.remove(&token).map(|reply| (buttons, reply.send))
        });
        if let Some((buttons, send)) = reply {
            let result = match button {
                POSITIVE if is_question(buttons) => StandardButtonKind::Yes,
                POSITIVE => StandardButtonKind::Ok,
                NEGATIVE if is_question(buttons) => StandardButtonKind::No,
                NEUTRAL if has_cancel_button(buttons) => StandardButtonKind::Cancel,
                CANCELED => StandardButtonKind::Cancel,
                _ => {
                    let _ = send.send(NativeDialogResult::Failed(
                        "The Android message dialog returned an unknown button.".into(),
                    ));
                    return Ok(());
                }
            };
            let _ = send.send(NativeDialogResult::Button(result));
        }
        Ok(())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>();
}
