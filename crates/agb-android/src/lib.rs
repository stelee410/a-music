//! The engine for the Android app (`android/`): a native library called through JNI from
//! `dev.agb.duo.Engine`. Built with `android/build.sh`.
//!
//! It plays `.agbw` files (AGBW.md), the same way the web player does: the app hands over the
//! sample files, then `song.json`, then asks for the piece a stretch at a time. Everything behind
//! the border is `agb-web`'s; only the border differs.
//!
//! Errors do not cross as exceptions: a call that fails returns null (or −1) and
//! `Engine.lastError()` says why.

pub mod jam;
#[cfg(target_os = "android")]
mod aaudio;

use jni::objects::{JByteArray, JClass, JFloatArray, JString};
use jni::sys::{jboolean, jdouble, jfloat, jfloatArray, jint, jlong, jstring};
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

// ---------------------------------------------------------------------------- the jam page

/// Start the jam instrument's sound. The rate it runs at, or a negative number.
#[no_mangle]
pub extern "system" fn Java_dev_agb_duo_Jam_start<'l>(_env: JNIEnv<'l>, _class: JClass<'l>) -> jint {
    #[cfg(target_os = "android")]
    {
        aaudio::start()
    }
    #[cfg(not(target_os = "android"))]
    {
        -1
    }
}

#[no_mangle]
pub extern "system" fn Java_dev_agb_duo_Jam_stop<'l>(_env: JNIEnv<'l>, _class: JClass<'l>) {
    #[cfg(target_os = "android")]
    aaudio::stop();
}

/// The finger on the pad: `x` left to right and `y` bottom to top, 0…1.
#[no_mangle]
pub extern "system" fn Java_dev_agb_duo_Jam_touch<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, x: jfloat, y: jfloat, down: jboolean) {
    jam::SHARED.ask(jam::Ask::Touch { x, y, down: down != 0 });
}

/// Everything else the page asks for, by number (see `Jam.kt`).
#[no_mangle]
pub extern "system" fn Java_dev_agb_duo_Jam_ask<'l>(_env: JNIEnv<'l>, _class: JClass<'l>, what: jint, a: jint, b: jfloat) {
    use jam::Ask;
    let i = a.max(0) as usize;
    let ask = match what {
        1 => Ask::Track(i),
        2 => Ask::Program(i, b.max(0.0) as usize),
        3 => Ask::Rhythm(i),
        4 => Ask::Key(a),
        5 => Ask::Scale(i),
        6 => Ask::Bpm(b),
        7 => Ask::Bars(a.max(1) as u32),
        8 => Ask::Record(a != 0),
        9 => Ask::Play(a != 0),
        10 => Ask::Click(a != 0),
        11 => Ask::Clear(i),
        12 => Ask::Mute(i, b != 0.0),
        13 => Ask::Volume(i, b),
        _ => return,
    };
    jam::SHARED.ask(ask);
}

/// What the page shows, into `out` (14 numbers): where the loop is (0…1), the note under the
/// finger (−1: none), the level of each track, whether each has something recorded, recording,
/// playing, how often the output ran dry, and the frames it takes at a time.
#[no_mangle]
pub extern "system" fn Java_dev_agb_duo_Jam_state<'l>(env: JNIEnv<'l>, _class: JClass<'l>, out: JFloatArray<'l>) {
    use std::sync::atomic::Ordering::Relaxed;
    let s = &jam::SHARED.shown;
    let mut v = [0f32; 14];
    v[0] = f32::from_bits(s.phase.load(Relaxed));
    v[1] = s.note.load(Relaxed) as f32;
    for i in 0..jam::TRACKS {
        v[2 + i] = f32::from_bits(s.level[i].load(Relaxed));
        v[6 + i] = if s.filled[i].load(Relaxed) { 1.0 } else { 0.0 };
    }
    v[10] = if s.recording.load(Relaxed) { 1.0 } else { 0.0 };
    v[11] = if s.playing.load(Relaxed) { 1.0 } else { 0.0 };
    #[cfg(target_os = "android")]
    {
        let (dry, burst) = aaudio::health();
        v[12] = dry as f32;
        v[13] = burst as f32;
    }
    let _ = env.set_float_array_region(&out, 0, &v);
}

/// The names of what can be chosen, as JSON: programs (with whether each is a drum kit), rhythms,
/// and scales (with their steps).
#[no_mangle]
pub extern "system" fn Java_dev_agb_duo_Jam_names<'l>(mut env: JNIEnv<'l>, _class: JClass<'l>) -> jstring {
    let v = serde_json::json!({
        "programs": jam::PROGRAMS.iter().map(|p| serde_json::json!({ "name": p.name, "drums": p.is_drums() })).collect::<Vec<_>>(),
        "rhythms": jam::RHYTHMS.iter().map(|r| r.name).collect::<Vec<_>>(),
        "scales": jam::SCALES.iter().map(|s| serde_json::json!({ "name": s.0, "steps": s.1 })).collect::<Vec<_>>(),
    });
    text(&mut env, &v.to_string())
}
