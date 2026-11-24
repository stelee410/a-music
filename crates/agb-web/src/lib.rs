//! The engine for the web player: a WebAssembly module with a plain C interface (no bindings
//! generator). Built with `web/build.sh`; the JavaScript side is `web/js/engine-core.js`.
//!
//! One song at a time. The page hands over the sample files, then `song.json`, then asks for the
//! piece a stretch at a time; the stretches put end to end are the render of the whole.
//!
//! Text goes in as (pointer, length) into memory from [`agbw_alloc`]; text comes out through
//! [`agbw_text_ptr`] / [`agbw_text_len`]. Ticks cross the border as `f64`.

use agb_render::{render, render_tail, RenderRequest};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// How long the last stretch goes on after the end of the piece, for what is still ringing.
pub const TAIL_MS: u32 = 2000;

struct Loaded {
    project: agb_score::Project,
    seed: u64,
}

static SAMPLES: Mutex<Vec<String>> = Mutex::new(Vec::new());
static SONG: Mutex<Option<Loaded>> = Mutex::new(None);
static TEXT: Mutex<String> = Mutex::new(String::new());
static OUT: Mutex<Vec<f32>> = Mutex::new(Vec::new());

fn say(s: impl Into<String>) {
    *TEXT.lock().unwrap_or_else(|e| e.into_inner()) = s.into();
}

unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> &'a [u8] {
    if len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(ptr, len)
    }
}

#[no_mangle]
pub extern "C" fn agbw_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len.max(1));
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// # Safety
/// `ptr` and `len` as given to and returned by [`agbw_alloc`].
#[no_mangle]
pub unsafe extern "C" fn agbw_free(ptr: *mut u8, len: usize) {
    drop(Vec::from_raw_parts(ptr, 0, len.max(1)));
}

#[no_mangle]
pub extern "C" fn agbw_text_ptr() -> *const u8 {
    TEXT.lock().unwrap_or_else(|e| e.into_inner()).as_ptr()
}

#[no_mangle]
pub extern "C" fn agbw_text_len() -> usize {
    TEXT.lock().unwrap_or_else(|e| e.into_inner()).len()
}

/// What [`agbw_sample`] does, apart from the border.
pub fn take_sample(id: &str, wav: &[u8]) -> Result<(), String> {
    agb_dsp::sampler::provide_sample(&format!("mem:{id}"), wav)?;
    let mut have = SAMPLES.lock().unwrap_or_else(|e| e.into_inner());
    if !have.iter().any(|h| h == id) {
        have.push(id.to_string());
    }
    Ok(())
}

/// Forget which samples were handed over: the next song names its own. (The sound of a sample
/// stays in memory until one of the same id replaces it.)
pub fn clear_samples() {
    SAMPLES.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// A WAV file of the song, under its asset id. 0 when it was taken, −1 (and a text) when not.
///
/// # Safety
/// Both ranges must be readable.
#[no_mangle]
pub unsafe extern "C" fn agbw_sample(id: *const u8, id_len: usize, wav: *const u8, wav_len: usize) -> i32 {
    let id = String::from_utf8_lossy(bytes(id, id_len)).into_owned();
    match take_sample(&id, bytes(wav, wav_len)) {
        Ok(()) => 0,
        Err(e) => {
            say(e);
            -1
        }
    }
}

/// What [`agbw_load`] does, apart from the border.
pub fn load(json: &[u8], transpose: i32, bpm: Option<f64>) -> Result<serde_json::Value, String> {
    let song: agb_song::Song = serde_json::from_slice(json).map_err(|e| format!("song.json: {e}"))?;
    let report = agb_song::validate(&song, None);
    if let Some(first) = report.errors.first() {
        return Err(format!("{} ({})", first.message, first.path));
    }
    let samples: BTreeMap<String, String> =
        SAMPLES.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|id| (id.clone(), format!("mem:{id}"))).collect();
    let l = agb_song::lower(&song, &agb_song::PlayOptions { transpose, bpm, samples }).map_err(|e| e.to_string())?;
    let tmap = agb_engine::state::tempo_map(&l.project);
    let info = serde_json::json!({
        "engineVersion": agb_dsp::ENGINE_VERSION,
        "sampleRate": l.project.sample_rate,
        "ppqn": l.project.ppqn,
        "bpm": l.project.tempo_bpm,
        "lengthTicks": l.length_ticks,
        "lengthSamples": tmap.tick_to_sample(l.length_ticks),
        "tailSamples": TAIL_MS as u64 * l.project.sample_rate as u64 / 1000,
        "skipped": l.skipped,
    });
    agb_render::forget_plugins();
    *SONG.lock().unwrap_or_else(|e| e.into_inner()) = Some(Loaded { project: l.project, seed: song.seed });
    Ok(info)
}

/// Take a song (`song.json` of an `.agbw`). 0 and a JSON text about it, or −1 and why not.
///
/// # Safety
/// The range must be readable.
#[no_mangle]
pub unsafe extern "C" fn agbw_load(json: *const u8, len: usize, transpose: i32, bpm: f64) -> i32 {
    match load(bytes(json, len), transpose, (bpm > 0.0).then_some(bpm)) {
        Ok(info) => {
            say(info.to_string());
            0
        }
        Err(e) => {
            say(e);
            -1
        }
    }
}

/// What [`agbw_render`] does: the samples of `[from, to)` in ticks, and the tail when `last`.
pub fn stretch(from: u64, to: u64, last: bool) -> Result<Vec<f32>, String> {
    let song = SONG.lock().unwrap_or_else(|e| e.into_inner());
    let Some(song) = song.as_ref() else { return Err("no song is loaded".into()) };
    if to <= from {
        return Err(format!("nothing to render between ticks {from} and {to}"));
    }
    let req = RenderRequest { from_tick: 0, to_tick: to, tail_ms: if last { TAIL_MS } else { 0 }, seed: song.seed, stems: false, format: Default::default() };
    let r = if from == 0 && last { render(&song.project, &req) } else { render_tail(&song.project, &req, from) }.map_err(|e| e.to_string())?;
    let tmap = agb_engine::state::tempo_map(&song.project);
    let skip = (tmap.tick_to_sample(from) - tmap.tick_to_sample(r.start_tick)) as usize;
    let mut mix = r.mix;
    mix.drain(..skip.min(mix.len()));
    Ok(mix)
}

/// Render a stretch. Returns the number of samples (mono, at [`agbw_out_ptr`]) or −1 and a text.
#[no_mangle]
pub extern "C" fn agbw_render(from_tick: f64, to_tick: f64, last: i32) -> i32 {
    match stretch(from_tick.max(0.0) as u64, to_tick.max(0.0) as u64, last != 0) {
        Ok(mix) => {
            let n = mix.len() as i32;
            *OUT.lock().unwrap_or_else(|e| e.into_inner()) = mix;
            n
        }
        Err(e) => {
            say(e);
            -1
        }
    }
}

#[no_mangle]
pub extern "C" fn agbw_out_ptr() -> *const f32 {
    OUT.lock().unwrap_or_else(|e| e.into_inner()).as_ptr()
}

/// The sample at which a tick of the loaded song sounds.
#[no_mangle]
pub extern "C" fn agbw_tick_to_sample(tick: f64) -> f64 {
    let song = SONG.lock().unwrap_or_else(|e| e.into_inner());
    song.as_ref().map_or(0.0, |s| agb_engine::state::tempo_map(&s.project).tick_to_sample(tick.max(0.0) as u64) as f64)
}

/// Start every plug-in and singer afresh at the next stretch (after a jump).
#[no_mangle]
pub extern "C" fn agbw_forget() {
    agb_render::forget_plugins();
}
