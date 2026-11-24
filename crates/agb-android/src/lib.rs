//! The engine for the Android app (`android/`): a native library called through JNI from
//! `dev.agb.duo.Engine`. Built with `android/build.sh`.
//!
//! It plays `.agbw` files (AGBW.md), the same way the web player does: the app hands over the
//! sample files, then `song.json`, then asks for the piece a stretch at a time. Everything behind
//! the border is `agb-web`'s; only the border differs.
//!
//! Errors do not cross as exceptions: a call that fails returns null (or −1) and
//! `Engine.lastError()` says why.

use jni::objects::{JByteArray, JClass, JString};
use jni::sys::{jboolean, jdouble, jfloatArray, jint, jlong, jstring};
use jni::JNIEnv;
use std::sync::Mutex;

static ERROR: Mutex<String> = Mutex::new(String::new());

fn fail(e: impl Into<String>) {
    *ERROR.lock().unwrap_or_else(|p| p.into_inner()) = e.into();
}

fn text(env: &mut JNIEnv, s: &str) -> jstring {
    env.new_string(s).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}

/// Why the last failed call failed.
#[no_mangle]
pub extern "system" fn Java_dev_agb_duo_Engine_lastError<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>) -> jstring {
    let e = ERROR.lock().unwrap_or_else(|p| p.into_inner()).clone();
    text(&mut env, &e)
}

/// The version of the sound engine, e.g. `agb-dsp/0.6.0`.
#[no_mangle]
pub extern "system" fn Java_dev_agb_duo_Engine_version<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>) -> jstring {
    text(&mut env, agb_dsp::ENGINE_VERSION)
}

/// A WAV file of the song, under its asset id. True when it was taken.
#[no_mangle]
pub extern "system" fn Java_dev_agb_duo_Engine_sample<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, id: JString<'l>, wav: JByteArray<'l>) -> jboolean {
    let id: String = match env.get_string(&id) {
        Ok(s) => s.into(),
        Err(e) => {
            fail(e.to_string());
            return 0;
        }
    };
    let wav = match env.convert_byte_array(&wav) {
        Ok(b) => b,
        Err(e) => {
            fail(e.to_string());
            return 0;
        }
    };
    match agb_web::take_sample(&id, &wav) {
        Ok(()) => 1,
        Err(e) => {
            fail(e);
            0
        }
    }
}

/// Forget the samples of the song before: the next song brings its own.
#[no_mangle]
pub extern "system" fn Java_dev_agb_duo_Engine_clearSamples<'l>(_env: JNIEnv<'l>, _class: JClass<'l>) {
    agb_web::clear_samples();
}

/// Take a song (`song.json` of an `.agbw`). A JSON text about it, or null.
/// `bpm` ≤ 0 means the song's own tempo.
#[no_mangle]
pub extern "system" fn Java_dev_agb_duo_Engine_load<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>, json: JByteArray<'l>, transpose: jint, bpm: jdouble) -> jstring {
    let json = match env.convert_byte_array(&json) {
        Ok(b) => b,
        Err(e) => {
            fail(e.to_string());
            return std::ptr::null_mut();
        }
    };
    // A handheld cannot sing at full cost and keep up: see `agb_render::set_light_singers`.
    agb_render::set_light_singers(true);
    match agb_web::load(&json, transpose, (bpm > 0.0).then_some(bpm)) {
        Ok(info) => text(&mut env, &info.to_string()),
        Err(e) => {
            fail(e);
            std::ptr::null_mut()
        }
    }
}

/// The samples (mono) of ticks `[from, to)`; with `last`, what rings on after the end too. Null
/// when it could not be rendered.
#[no_mangle]
pub extern "system" fn Java_dev_agb_duo_Engine_render<'l>(env: JNIEnv<'l>, _class: JClass<'l>, from: jlong, to: jlong, last: jboolean) -> jfloatArray {
    let mix = match agb_web::stretch(from.max(0) as u64, to.max(0) as u64, last != 0) {
        Ok(m) => m,
        Err(e) => {
            fail(e);
            return std::ptr::null_mut();
        }
    };
    let made = env.new_float_array(mix.len() as i32).and_then(|a| env.set_float_array_region(&a, 0, &mix).map(|()| a));
    match made {
        Ok(a) => a.into_raw(),
        Err(e) => {
            fail(e.to_string());
            std::ptr::null_mut()
        }
    }
}

/// The sample at which a tick of the loaded song sounds.
#[no_mangle]
pub extern "system" fn Java_dev_agb_duo_Engine_tickToSample<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, tick: jlong) -> jlong {
    agb_web::agbw_tick_to_sample(tick.max(0) as f64) as jlong
}

/// After a jump: instruments and singers start afresh at the next stretch.
#[no_mangle]
pub extern "system" fn Java_dev_agb_duo_Engine_forget<'l>(_env: JNIEnv<'l>, _class: JClass<'l>) {
    agb_render::forget_plugins();
}

/// Keep the calling thread and the threads it starts from now on (the singers) on the fastest
/// cores: see [`agb_render::use_fast_cores`]. Returns how many cores that is.
#[no_mangle]
pub extern "system" fn Java_dev_agb_duo_Engine_useFastCores<'l>(_env: JNIEnv<'l>, _class: JClass<'l>) -> jint {
    agb_render::use_fast_cores() as jint
}
