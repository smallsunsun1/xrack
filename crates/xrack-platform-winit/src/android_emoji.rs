use crate::android_directory::with_activity;
use android_activity::AndroidApp;
use jni::{
    Env, jni_sig, jni_str,
    objects::{JFloatArray, JIntArray, JObject, JValue},
    refs::Global,
};
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
};
use xrack_renderer_femtovg::{AndroidGlyphBitmap, TextSystem};

struct Font {
    file: PathBuf,
    index: u32,
    object: Global<JObject<'static>>,
}

pub(crate) fn install(app: &AndroidApp, text: &mut TextSystem) {
    let sdk = with_activity(app, |env, _| {
        env.get_static_field(
            jni_str!("android/os/Build$VERSION"),
            jni_str!("SDK_INT"),
            jni_sig!("I"),
        )?
        .i()
    });
    if !sdk.is_ok_and(|sdk| sdk >= 31) {
        return;
    }
    let app = app.clone();
    let font = RefCell::<Option<Font>>::new(None);
    text.set_android_glyph_rasterizer(move |file, index, glyph, size| {
        with_activity(&app, |env, _| {
            let mut cached = font.borrow_mut();
            if cached
                .as_ref()
                .is_none_or(|font| font.file != file || font.index != index)
            {
                *cached = Some(Font {
                    file: file.into(),
                    index,
                    object: load_font(env, file, index)?,
                });
            }
            match cached.as_ref() {
                Some(font) => rasterize(env, font.object.as_ref(), glyph, size),
                None => Ok(None),
            }
        })
        .ok()
        .flatten()
    });
}

fn load_font(
    env: &mut Env<'_>,
    file: &Path,
    index: u32,
) -> jni::errors::Result<Global<JObject<'static>>> {
    let path = env.new_string(file.to_string_lossy())?;
    let file = env.new_object(
        jni_str!("java/io/File"),
        jni_sig!("(Ljava/lang/String;)V"),
        &[JValue::Object(path.as_ref())],
    )?;
    let builder = env.new_object(
        jni_str!("android/graphics/fonts/Font$Builder"),
        jni_sig!("(Ljava/io/File;)V"),
        &[JValue::Object(&file)],
    )?;
    env.call_method(
        &builder,
        jni_str!("setTtcIndex"),
        jni_sig!("(I)Landroid/graphics/fonts/Font$Builder;"),
        &[JValue::Int(index as i32)],
    )?;
    let font = env
        .call_method(
            &builder,
            jni_str!("build"),
            jni_sig!("()Landroid/graphics/fonts/Font;"),
            &[],
        )?
        .l()?;
    env.new_global_ref(font)
}

fn rasterize(
    env: &mut Env<'_>,
    font: &JObject<'_>,
    glyph: u16,
    size: u16,
) -> jni::errors::Result<Option<AndroidGlyphBitmap>> {
    let paint = env.new_object(
        jni_str!("android/graphics/Paint"),
        jni_sig!("(I)V"),
        &[JValue::Int(1)],
    )?;
    env.call_method(
        &paint,
        jni_str!("setTextSize"),
        jni_sig!("(F)V"),
        &[JValue::Float(size.into())],
    )?;
    let bounds = env.new_object(jni_str!("android/graphics/RectF"), jni_sig!("()V"), &[])?;
    env.call_method(
        font,
        jni_str!("getGlyphBounds"),
        jni_sig!("(ILandroid/graphics/Paint;Landroid/graphics/RectF;)F"),
        &[
            JValue::Int(glyph.into()),
            JValue::Object(&paint),
            JValue::Object(&bounds),
        ],
    )?;
    let mut edge = |name| env.get_field(&bounds, name, jni_sig!("F"))?.f();
    let left = edge(jni_str!("left"))?.floor();
    let top = edge(jni_str!("top"))?.floor();
    let right = edge(jni_str!("right"))?.ceil();
    let bottom = edge(jni_str!("bottom"))?.ceil();
    let width = (right - left) as i32;
    let height = (bottom - top) as i32;
    if !(1..=512).contains(&width) || !(1..=512).contains(&height) {
        return Ok(None);
    }
    let config = env
        .get_static_field(
            jni_str!("android/graphics/Bitmap$Config"),
            jni_str!("ARGB_8888"),
            jni_sig!("Landroid/graphics/Bitmap$Config;"),
        )?
        .l()?;
    let bitmap = env
        .call_static_method(
            jni_str!("android/graphics/Bitmap"),
            jni_str!("createBitmap"),
            jni_sig!("(IILandroid/graphics/Bitmap$Config;)Landroid/graphics/Bitmap;"),
            &[
                JValue::Int(width),
                JValue::Int(height),
                JValue::Object(&config),
            ],
        )?
        .l()?;
    let canvas = env.new_object(
        jni_str!("android/graphics/Canvas"),
        jni_sig!("(Landroid/graphics/Bitmap;)V"),
        &[JValue::Object(&bitmap)],
    )?;
    let ids = JIntArray::new(env, 1)?;
    ids.set_region(env, 0, &[glyph.into()])?;
    let positions = JFloatArray::new(env, 2)?;
    positions.set_region(env, 0, &[-left, -top])?;
    env.call_method(
        &canvas,
        jni_str!("drawGlyphs"),
        jni_sig!("([II[FIILandroid/graphics/fonts/Font;Landroid/graphics/Paint;)V"),
        &[
            JValue::Object(ids.as_ref()),
            JValue::Int(0),
            JValue::Object(positions.as_ref()),
            JValue::Int(0),
            JValue::Int(1),
            JValue::Object(font),
            JValue::Object(&paint),
        ],
    )?;
    let mut argb = vec![0; (width * height) as usize];
    let pixels = JIntArray::new(env, argb.len())?;
    env.call_method(
        &bitmap,
        jni_str!("getPixels"),
        jni_sig!("([IIIIIII)V"),
        &[
            JValue::Object(pixels.as_ref()),
            JValue::Int(0),
            JValue::Int(width),
            JValue::Int(0),
            JValue::Int(0),
            JValue::Int(width),
            JValue::Int(height),
        ],
    )?;
    pixels.get_region(env, 0, &mut argb)?;
    env.call_method(&bitmap, jni_str!("recycle"), jni_sig!("()V"), &[])?;
    let rgba = argb
        .into_iter()
        .flat_map(|pixel| {
            let [a, r, g, b] = pixel.to_be_bytes();
            [r, g, b, a]
        })
        .collect();
    Ok(Some(AndroidGlyphBitmap {
        offset: [left as i32, top as i32],
        size: [width as u32, height as u32],
        rgba,
    }))
}
