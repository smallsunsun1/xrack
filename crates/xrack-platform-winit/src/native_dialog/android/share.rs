use super::intent::{Context, Intent, IntentSender};
use super::{DialogKind, PENDING};
use android_activity::AndroidApp;
use jni::{
    EnvUnowned, NativeMethod, bind_java_type, jni_sig, jni_str,
    objects::{JCharSequence, JClass, JObject, JString, JValue},
    refs::Global,
    sys::jboolean,
};

bind_java_type! {
    PendingIntent => android.app.PendingIntent,
    type_map {
        Context => android.content.Context,
        Intent => android.content.Intent,
        IntentSender => android.content.IntentSender,
    },
    methods {
        static fn get_broadcast(context: Context, request_code: jint, intent: Intent, flags: jint) -> PendingIntent,
        fn get_intent_sender() -> IntentSender,
        fn cancel(),
    }
}

bind_java_type! {
    Uuid => java.util.UUID,
    methods {
        static fn random_uuid {
            name = "randomUUID",
            sig = () -> Uuid,
        },
        fn as_string {
            name = "toString",
            sig = () -> JString,
        },
    }
}

pub(super) struct ShareRequest {
    activity: Global<JObject<'static>>,
    action: String,
    chooser_took_focus: bool,
}

pub(super) fn native_methods() -> [NativeMethod<'static>; 3] {
    // SAFETY: Each function uses the JNI ABI and the signature in XRackActivity.
    unsafe {
        [
            NativeMethod::from_raw_parts(
                jni_str!("launchShare"),
                jni_str!("(JLjava/lang/String;)Z"),
                launch as *mut std::ffi::c_void,
            ),
            NativeMethod::from_raw_parts(
                jni_str!("shareFocusChanged"),
                jni_str!("(JZ)Z"),
                focus_changed as *mut std::ffi::c_void,
            ),
            NativeMethod::from_raw_parts(
                jni_str!("shareSelected"),
                jni_str!("(Landroid/content/Intent;)V"),
                selected as *mut std::ffi::c_void,
            ),
        ]
    }
}

extern "system" fn launch<'local>(
    mut env: EnvUnowned<'local>,
    activity: JObject<'local>,
    token: i64,
    text: JString<'local>,
) -> jboolean {
    const FLAG_ONE_SHOT: i32 = 0x4000_0000;
    const FLAG_CANCEL_CURRENT: i32 = 0x1000_0000;
    const FLAG_MUTABLE: i32 = 0x0200_0000;

    env.with_env(|env| -> jni::errors::Result<bool> {
        let context = env.as_cast::<Context>(&activity)?;
        let package = context.get_package_name(env)?.try_to_string(env)?;
        let uuid = Uuid::random_uuid(env)?.as_string(env)?.try_to_string(env)?;
        let action = format!("{package}.XRACK_SHARE.{uuid}");
        let callback_action = env.new_string(&action)?;
        let activity_ref = env.new_global_ref(&activity)?;
        {
            let Ok(mut pending) = PENDING.lock() else {
                return Ok(false);
            };
            let Some(reply) = pending.get_mut(&token).filter(|reply| {
                reply.kind == DialogKind::ShareText
                    && reply.share.is_none()
                    && !reply.send.is_closed()
            }) else {
                return Ok(false);
            };
            reply.share = Some(ShareRequest {
                activity: activity_ref,
                action,
                chooser_took_focus: false,
            });
        }

        let receiver = env.new_string("dev.xrack.platform.XRackActivity$ShareResultReceiver")?;
        let result = Intent::new(env, &callback_action)?;
        result.set_class_name(env, &*context, &receiver)?;
        let action_send = env.new_string("android.intent.action.SEND")?;
        let target = Intent::new(env, &action_send)?;
        let mime = env.new_string("text/plain")?;
        target.set_type(env, &mime)?;
        let extra_text = env.new_string("android.intent.extra.TEXT")?;
        target.put_string(env, &extra_text, &text)?;

        let mut flags = FLAG_ONE_SHOT | FLAG_CANCEL_CURRENT;
        if AndroidApp::sdk_version() >= 31 {
            // The chooser adds result extras to the explicit broadcast.
            flags |= FLAG_MUTABLE;
        }
        let callback = PendingIntent::get_broadcast(env, &*context, 0, &result, flags)?;
        let started = (|| -> jni::errors::Result<()> {
            let sender = callback.get_intent_sender(env)?;
            let chooser = Intent::create_chooser(env, &target, JCharSequence::null(), &sender)?;
            env.call_method(
                &activity,
                jni_str!("startShareChooser"),
                jni_sig!("(Landroid/content/Intent;Landroid/app/PendingIntent;)V"),
                &[
                    JValue::Object(chooser.as_ref()),
                    JValue::Object(callback.as_ref()),
                ],
            )?;
            Ok(())
        })();
        if started.is_err() {
            let _ = callback.cancel(env);
        }
        started?;
        Ok(true)
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

extern "system" fn focus_changed(
    mut env: EnvUnowned<'_>,
    _: JObject<'_>,
    token: i64,
    has_focus: jboolean,
) -> jboolean {
    env.with_env(|_| -> jni::errors::Result<bool> {
        let Ok(mut pending) = PENDING.lock() else {
            return Ok(false);
        };
        let Some(reply) = pending.get_mut(&token) else {
            // The Rust window may have dropped its receiver while the
            // system chooser was open. Release the bridge on return.
            return Ok(has_focus);
        };
        let Some(share) = reply.share.as_mut() else {
            return Ok(false);
        };
        if !has_focus {
            share.chooser_took_focus = true;
        } else if share.chooser_took_focus {
            // The chooser only broadcasts explicit selections. Reserve this
            // result so a later broadcast cannot finish the request again.
            reply.share.take();
            return Ok(true);
        }
        Ok(false)
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>()
}

extern "system" fn selected<'local>(
    mut env: EnvUnowned<'local>,
    _: JClass<'local>,
    result: JObject<'local>,
) {
    env.with_env(|env| -> jni::errors::Result<()> {
        let intent = env.cast_local::<Intent>(result)?;
        let chosen = env.new_string("android.intent.extra.CHOSEN_COMPONENT")?;
        if !intent.has_extra(env, &chosen)? {
            return Ok(());
        }
        let action = intent.get_action(env)?;
        if action.is_null() {
            return Ok(());
        }
        let action = action.try_to_string(env)?;
        let matching = PENDING.lock().ok().and_then(|mut pending| {
            let (token, reply) = pending.iter_mut().find(|(_, reply)| {
                reply
                    .share
                    .as_ref()
                    .is_some_and(|share| share.action == action)
            })?;
            reply.share.take().map(|share| (*token, share.activity))
        });
        if let Some((token, activity)) = matching {
            let payload = env.new_string("")?;
            // complete() releases the PendingIntent before calling finishDialog.
            // The registry lock was released before the JNI call can reenter it.
            env.call_method(
                &activity,
                jni_str!("complete"),
                jni_sig!("(JILjava/lang/String;)V"),
                &[
                    JValue::Long(token),
                    JValue::Int(2),
                    JValue::Object(payload.as_ref()),
                ],
            )?;
        }
        Ok(())
    })
    .resolve::<jni::errors::ThrowRuntimeExAndDefault>();
}
