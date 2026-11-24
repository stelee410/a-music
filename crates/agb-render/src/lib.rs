//! Offline block renderer (SPEC §10) → `mix.wav`, `stems/*.wav`,
//! `events.json`, `metrics.json`, `render.json`.
//!
//! Determinism (§10.2): the output depends only on the project content, seed,
//! sample rate, window, tail and engine version. Internal mixing is f32;
//! oscillators use `agb_dsp::detmath` so results do not depend on the platform
//! libm. PCM16 export: clamp to [-1, 1], multiply by 32767, round half away
//! from zero.

use agb_dsp::detmath::{fnv1a, gain_to_db};
use agb_dsp::{EngineParams, Voice, ENGINE_VERSION};

mod plugins;
mod voices;
pub use plugins::forget_plugins;
pub use voices::set_light as set_light_singers;

mod cores;
pub use cores::use_fast_cores;
use agb_dsp::detmath::tanh_d;
use agb_score::MasterLimiter;
use agb_score::ParamValue;
use agb_engine::state::MAX_VOICES;
use agb_engine::{schedule, NoteEvent, RenderHook, Schedule};
use agb_protocol::{AgbError, RenderOffline, SampleFormat};
use agb_score::{Project, StealPolicy};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const DEFAULT_TAIL_MS: u32 = 500;
/// `T_panic` / `nearSilent` threshold.
pub const NEAR_SILENT_DBFS: f64 = -60.0;

#[derive(Clone, Debug, PartialEq)]
pub struct RenderRequest {
    pub from_tick: u64,
    pub to_tick: u64,
    pub tail_ms: u32,
    pub seed: u64,
    pub stems: bool,
    pub format: SampleFormat,
}

impl RenderRequest {
    /// Fill defaults from the project (`toTick` = one polymeter cycle).
    pub fn from_op(p: &Project, op: &RenderOffline) -> Self {
        RenderRequest {
            from_tick: op.from_tick.unwrap_or(p.origin_tick),
            to_tick: op.to_tick.unwrap_or_else(|| agb_engine::schedule::default_to_tick(p)),
            tail_ms: op.tail_ms.unwrap_or(DEFAULT_TAIL_MS),
            seed: op.seed.unwrap_or(p.seed),
            stems: op.stems,
            format: op.format.unwrap_or_default(),
        }
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RenderInfo {
    pub spec_version: String,
    pub engine_version: &'static str,
    pub project_name: String,
    pub project_revision: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub format: SampleFormat,
    pub seed: u64,
    pub from_tick: u64,
    pub to_tick: u64,
    pub tail_ms: u32,
    pub tempo_bpm: f64,
    pub ppqn: u32,
    pub steps_per_quarter: u32,
    pub ticks_per_step: u64,
    pub window_samples: u64,
    pub tail_samples: u64,
    pub total_samples: u64,
    pub master_gain_db: f64,
    pub steal_policy: StealPolicy,
    pub panic_active: bool,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct BufferMetrics {
    pub peak: f64,
    /// `null` when the buffer is exactly silent.
    pub peak_dbfs: Option<f64>,
    pub rms: f64,
    /// Samples whose pre-limiter magnitude exceeded 0 dBFS.
    pub clip_count: u64,
    pub all_silent: bool,
    /// peak < −60 dBFS.
    pub near_silent: bool,
    /// Sum of squares over the tail segment (after `toTick`).
    pub tail_energy: f64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub mix: BufferMetrics,
    pub stems: std::collections::BTreeMap<String, BufferMetrics>,
    pub note_on_count: usize,
    pub voices_stolen: u64,
    pub max_concurrent_voices: usize,
    /// `sha256:<hex>` of the exported `mix.wav` bytes (filled in by [`encode`]).
    pub mix_sha256: String,
    pub stem_sha256: std::collections::BTreeMap<String, String>,
}

/// In-memory render result.
pub struct Rendered {
    pub info: RenderInfo,
    pub schedule: Schedule,
    pub mix: Vec<f32>,
    pub stems: Vec<(String, Vec<f32>)>,
    pub metrics: Metrics,
    /// Tick at which the buffers begin: `fromTick`, or earlier for [`render_tail`].
    pub start_tick: u64,
}

struct Slot {
    voice: Box<dyn Voice>,
    note_id: u64,
    stem: usize,
    pitch: u8,
    start: u64,
}

fn voice_seed(seed: u64, ev: &NoteEvent) -> u64 {
    seed ^ fnv1a(&ev.track_id).rotate_left(17) ^ ev.note_id.wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

fn pick_steal(pool: &[Slot], policy: StealPolicy, ev: &NoteEvent) -> usize {
    let oldest = || {
        (0..pool.len()).min_by_key(|&i| (pool[i].start, pool[i].note_id)).unwrap()
    };
    match policy {
        StealPolicy::Oldest => oldest(),
        StealPolicy::Quietest => (0..pool.len())
            .min_by(|&a, &b| {
                pool[a]
                    .voice
                    .level()
                    .total_cmp(&pool[b].voice.level())
                    .then(pool[a].note_id.cmp(&pool[b].note_id))
            })
            .unwrap(),
        StealPolicy::SamePitch => (0..pool.len())
            .filter(|&i| pool[i].pitch == ev.pitch && pool[i].stem == ev.track_index)
            .min_by_key(|&i| (pool[i].start, pool[i].note_id))
            .unwrap_or_else(oldest),
    }
}

fn buffer_metrics(buf: &[f32], clip_count: u64, window: usize) -> BufferMetrics {
    let mut peak = 0f64;
    let mut sq = 0f64;
    let mut tail = 0f64;
    for (i, &x) in buf.iter().enumerate() {
        let a = (x as f64).abs();
        peak = peak.max(a);
        let s = (x as f64) * (x as f64);
        sq += s;
        if i >= window {
            tail += s;
        }
    }
    let peak_dbfs = gain_to_db(peak);
    BufferMetrics {
        peak,
        peak_dbfs,
        rms: if buf.is_empty() { 0.0 } else { (sq / buf.len() as f64).sqrt() },
        clip_count,
        all_silent: peak == 0.0,
        near_silent: peak_dbfs.map_or(true, |d| d < NEAR_SILENT_DBFS),
        tail_energy: tail,
    }
}

/// Soft knee above −3 dBFS that asymptotically approaches 0 dBFS.
pub fn soft_limit(x: f32) -> f32 {
    const T: f64 = 0.707_945_784; // −3 dBFS
    let a = (x as f64).abs();
    if a <= T {
        return x;
    }
    let y = T + (1.0 - T) * tanh_d((a - T) / (1.0 - T));
    (y.copysign(x as f64)) as f32
}

/// Render to memory. Never mutates the project.
pub fn render(p: &Project, req: &RenderRequest) -> Result<Rendered, AgbError> {
    render_inner(p, req, None)
}

/// How long before the wanted part the rendering starts, so that what is still ringing when it
/// begins (a cymbal, a room) is there: longer than any one-shot sample or reverb tail in use.
const RUN_IN_SECONDS: f64 = 7.0;

/// The end of a render, for playing a piece as it goes: the audio of `[keep_from, toTick)` without
/// working out everything before it again. `req.from_tick` is where the piece starts (0), so the
/// notes are numbered — and therefore sound — exactly as in a render of the whole. Rendering begins
/// [`RUN_IN_SECONDS`] before `keep_from`, or at the start of the earliest note still held then.
/// The buffers begin at `Rendered::start_tick`.
pub fn render_tail(p: &Project, req: &RenderRequest, keep_from: u64) -> Result<Rendered, AgbError> {
    render_inner(p, req, Some(keep_from))
}

fn render_inner(p: &Project, req: &RenderRequest, keep_from: Option<u64>) -> Result<Rendered, AgbError> {
    let mut sched = schedule(p, req.from_tick, req.to_tick)?;
    let sr = p.sample_rate;
    let tmap = agb_engine::state::tempo_map(p);
    let to_sample = |tick: u64| tmap.tick_to_sample(tick);
    // Sung tracks: every note from the start of the piece (a word's syllables are spread over the
    // notes after it), taken before the notes that no longer sound are dropped below.
    // While a piece is played bar by bar, the notes just after this bar are looked at as well: a
    // singer starts a note before its beat (the consonants come first), so the first note of the
    // next bar begins in this one.
    let ahead;
    let sung_from = if keep_from.is_some() && req.tail_ms == 0 && sched.events.iter().any(|e| matches!(e.params, EngineParams::Voice(_))) {
        ahead = schedule(p, req.from_tick, req.to_tick + 2 * p.ppqn as u64)?;
        &ahead
    } else {
        &sched
    };
    let sung: Vec<(usize, u64, u64, u8, f32, Option<String>, agb_dsp::VoiceParams)> = sung_from
        .events
        .iter()
        .filter_map(|ev| match &ev.params {
            EngineParams::Voice(spec) => {
                let mut off = ev.off_tick;
                for c in sung_from.cuts.iter().filter(|c| c.track_index == ev.track_index && c.tick > ev.tick) {
                    off = off.min(c.tick);
                }
                Some((ev.track_index, to_sample(ev.tick), to_sample(off), ev.pitch, ev.amp as f32, ev.lyric.clone(), spec.clone()))
            }
            _ => None,
        })
        .collect();
    let start_tick = match keep_from {
        None => req.from_tick,
        Some(keep) => {
            let keep_sample = to_sample(keep.max(req.from_tick));
            let run_in = keep_sample.saturating_sub((RUN_IN_SECONDS * sr as f64) as u64);
            let mut start = (tmap.sample_to_tick(run_in as f64) as u64).max(req.from_tick);
            for e in &sched.events {
                if e.tick < start && e.off_tick > keep {
                    start = e.tick; // a note that began earlier and is still held
                }
            }
            sched.events.retain(|e| e.tick >= start);
            sched.cuts.retain(|c| c.tick >= start);
            start
        }
    };
    let base = to_sample(start_tick);
    let window = to_sample(req.to_tick) - base;
    let tail = (req.tail_ms as u64 * sr as u64) / 1000;
    let total = (window + tail) as usize;

    let n_stems = sched.track_order.len();
    let mut stems: Vec<Vec<f32>> = vec![vec![0.0; total]; n_stems];
    let mut stolen = 0u64;
    let mut max_conc = 0usize;

    // Track effects (FORMAT.md §9). The chain hears the track at unity gain, whatever its fader says:
    // how hard an amplifier is driven must not change when the track is turned down in the mix.
    let head = agb_score::timeline::head(&p.timeline);
    let effects = |k: usize, stem: &mut Vec<f32>| -> Result<(), AgbError> {
        let Some(track) = head.track(&sched.track_order[k]) else { return Ok(()) };
        if track.fx.iter().all(|f| f.get("bypass").and_then(|b| b.as_bool()) == Some(true)) {
            return Ok(());
        }
        let fader = agb_dsp::detmath::db_to_gain(track.gain_db) as f32;
        if fader > 1e-6 {
            stem.iter_mut().for_each(|x| *x /= fader);
        }
        for (i, f) in track.fx.iter().enumerate() {
            if f.get("bypass").and_then(|b| b.as_bool()) == Some(true) {
                continue;
            }
            let kind = f.get("type").and_then(|t| t.as_str());
            if kind == Some(agb_dsp::plugin::ENGINE_ID) || kind == Some(agb_plugin::web::ENGINE_ID) {
                let mut spec = f.get("params").and_then(agb_dsp::PluginParams::from_json).ok_or_else(|| {
                    AgbError::new(agb_protocol::ErrorCode::SchemaViolation, format!("track {}: fx[{i}] needs params.plugin", track.id))
                })?;
                if kind == Some(agb_plugin::web::ENGINE_ID) {
                    spec.name = format!("{}{}", agb_plugin::web::PREFIX, spec.name);
                    if !agb_plugin::available(&spec.name, None) {
                        continue; // an effect of the web player, heard only there
                    }
                }
                plugins::render_fx(&track.id, i, &spec, stem, base, sr, tmap.bpm_at(start_tick))?;
            } else if let Some(mut fx) = agb_dsp::fx::build(f, sr as f64) {
                fx.process(stem);
            }
        }
        if fader > 1e-6 {
            stem.iter_mut().for_each(|x| *x *= fader);
        }
        Ok(())
    };
    // Which stems have been through their effects.
    let mut finished = vec![false; n_stems];

    if !p.panic.active {
        // A render of the whole releases what is still held at `toTick`; a bar of a piece being
        // played leaves it held for the next bar.
        let whole = keep_from.is_none() || req.tail_ms > 0;
        let end_sample = to_sample(req.to_tick);
        let keep = keep_from.map(|k| to_sample(k.max(req.from_tick)));

        // The sung tracks: each is rendered on a thread of its own while the others are rendered
        // here (a simulated voice costs more than all the rest of a song). Where there are no
        // threads (the web player) they are rendered here too, after the others.
        let mut songs: Vec<(usize, Vec<voices::Note>)> = vec![];
        for (k, id) in sched.track_order.iter().enumerate() {
            let mut lines: Vec<voices::Note> = sung
                .iter()
                .filter(|n| n.0 == k)
                .map(|(_, on, off, pitch, amp, lyric, spec)| voices::Note {
                    on: *on,
                    off: (if whole { (*off).min(end_sample) } else { *off }).max(on + 1),
                    pitch: *pitch,
                    amp: *amp,
                    lyric: lyric.as_deref(),
                    spec,
                })
                .collect();
            lines.sort_by_key(|n| n.on);
            if !lines.is_empty() || voices::has_track(id) {
                songs.push((k, lines));
            }
        }
        let order = &sched.track_order;
        let sing = move |k: usize, lines: &[voices::Note]| -> Result<Vec<f32>, AgbError> {
            let mut stem = vec![0f32; total];
            voices::render_track(&order[k], lines, &mut stem, base, keep, !whole, sr)?;
            Ok(stem)
        };

        std::thread::scope(|scope| -> Result<(), AgbError> {
            #[cfg(not(target_arch = "wasm32"))]
            let singing: Vec<_> = songs
                .iter()
                .map(|(k, lines)| {
                    let k = *k;
                    (k, scope.spawn(move || sing(k, lines)))
                })
                .collect();

            // Actions: (sample, kind 0=off 1=cut 2=on, note_id/track, event index)
            let mut actions: Vec<(u64, u8, u64, usize)> = Vec::with_capacity(sched.events.len() * 2);
            for (i, ev) in sched.events.iter().enumerate() {
                if matches!(ev.params, EngineParams::Plugin(_) | EngineParams::Voice(_)) {
                    continue; // played by a plug-in or sung, below
                }
                let on = to_sample(ev.tick) - base;
                let off = to_sample(ev.off_tick.min(req.to_tick)) - base;
                actions.push((on, 2, ev.note_id, i));
                actions.push((off.max(on), 0, ev.note_id, i));
            }
            for c in &sched.cuts {
                actions.push((to_sample(c.tick) - base, 1, c.track_index as u64, usize::MAX));
            }
            actions.sort_unstable();

            // Automation: evaluated every 64 samples per track lane.
            let lanes = &sched.automation;
            let lane_values = |abs_sample: u64, track: usize| -> Vec<(&str, f64)> {
                let tick = tmap.sample_to_tick(abs_sample as f64);
                let mut vals: Vec<(&str, f64, u64)> = vec![];
                for l in lanes.iter().filter(|l| l.track_index == track) {
                    if let Some(v) = l.lane.value_at(tick) {
                        // later-starting lane wins on the same param
                        match vals.iter_mut().find(|x| x.0 == l.lane.param) {
                            Some(x) if x.2 <= l.lane.from_tick => *x = (l.lane.param.as_str(), v, l.lane.from_tick),
                            Some(_) => {}
                            None => vals.push((l.lane.param.as_str(), v, l.lane.from_tick)),
                        }
                    }
                }
                vals.into_iter().map(|(a, b, _)| (a, b)).collect()
            };
            let apply_lanes = |pool: &mut Vec<Slot>, abs_sample: u64| {
                for s in pool.iter_mut() {
                    for (path, v) in lane_values(abs_sample, s.stem) {
                        let _ = s.voice.set_param(path, &ParamValue::Num(v));
                    }
                }
            };

            let mut pool: Vec<Slot> = Vec::with_capacity(MAX_VOICES);
            let mut pos = 0usize;
            let run = |pool: &mut Vec<Slot>, stems: &mut Vec<Vec<f32>>, from: usize, to: usize| {
                if to > from {
                    if lanes.is_empty() {
                        for s in pool.iter_mut() {
                            s.voice.render(&mut stems[s.stem][from..to]);
                        }
                    } else {
                        let mut a = from;
                        while a < to {
                            let b = (a + 64).min(to);
                            apply_lanes(pool, base + a as u64);
                            for s in pool.iter_mut() {
                                s.voice.render(&mut stems[s.stem][a..b]);
                            }
                            a = b;
                        }
                    }
                }
                pool.retain(|s| s.voice.is_active());
            };
            for &(sample, kind, id, ei) in &actions {
                let at = (sample as usize).min(total);
                run(&mut pool, &mut stems, pos, at);
                pos = pos.max(at);
                match kind {
                    0 => {
                        let ev = &sched.events[ei];
                        if let Some(s) = pool.iter_mut().find(|s| s.note_id == id) {
                            s.voice.note_off(ev.pitch, ev.off_tick);
                        }
                    }
                    1 => pool.retain(|s| s.stem as u64 != id),
                    _ => {
                        let ev = &sched.events[ei];
                        if pool.len() >= MAX_VOICES {
                            let i = pick_steal(&pool, p.steal_policy, ev);
                            pool.swap_remove(i);
                            stolen += 1;
                        }
                        let mut v = ev.params.make_voice(sr, voice_seed(req.seed, ev));
                        for (path, val) in lane_values(base + sample, ev.track_index) {
                            let _ = v.set_param(path, &ParamValue::Num(val));
                        }
                        v.set_velocity(ev.velocity as f32);
                        v.note_on(ev.pitch, ev.amp as f32, ev.tick);
                        if v.chokes_others() {
                            let g = v.choke_group();
                            pool.retain(|s| !(s.stem == ev.track_index && s.voice.choke_group() == g));
                        }
                        pool.push(Slot { voice: v, note_id: id, stem: ev.track_index, pitch: ev.pitch, start: sample });
                        max_conc = max_conc.max(pool.len());
                    }
                }
            }
            run(&mut pool, &mut stems, pos, total);

            // Tracks played by plug-ins.
            let mut notes: Vec<Vec<plugins::Note>> = (0..n_stems).map(|_| vec![]).collect();
            for ev in &sched.events {
                let EngineParams::Plugin(spec) = &ev.params else { continue };
                let on = to_sample(ev.tick);
                let mut off = to_sample(ev.off_tick);
                for c in sched.cuts.iter().filter(|c| c.track_index == ev.track_index && c.tick > ev.tick) {
                    off = off.min(to_sample(c.tick));
                }
                if whole {
                    off = off.min(end_sample);
                }
                let gain = if ev.velocity > 0.0 { (ev.amp / ev.velocity) as f32 } else { 1.0 };
                notes[ev.track_index].push(plugins::Note { on, off: off.max(on + 1), pitch: ev.pitch, velocity: ev.velocity as f32, gain, spec });
            }
            for (k, stem) in stems.iter_mut().enumerate() {
                let id = &sched.track_order[k];
                if !notes[k].is_empty() || plugins::has_track(id) {
                    plugins::render_track(id, &notes[k], stem, base, keep, !whole, sr, tmap.bpm_at(start_tick))?;
                }
            }

            // The effects of the tracks that are not sung, while the singers are still singing.
            for (k, stem) in stems.iter_mut().enumerate() {
                if !songs.iter().any(|s| s.0 == k) {
                    effects(k, stem)?;
                    finished[k] = true;
                }
            }

            // The sung tracks join their stems (after the plug-ins, as before).
            #[cfg(not(target_arch = "wasm32"))]
            let sung_stems: Vec<(usize, Result<Vec<f32>, AgbError>)> =
                singing.into_iter().map(|(k, h)| (k, h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))).collect();
            #[cfg(target_arch = "wasm32")]
            let sung_stems: Vec<(usize, Result<Vec<f32>, AgbError>)> = {
                let _ = scope;
                songs.iter().map(|(k, lines)| (*k, sing(*k, lines))).collect()
            };
            for (k, voice) in sung_stems {
                for (x, y) in stems[k].iter_mut().zip(&voice?) {
                    if *y != 0.0 {
                        *x += *y;
                    }
                }
            }
            Ok(())
        })?;
    }

    // The effects of the tracks that have not had them yet (all of them after a panic).
    for (k, stem) in stems.iter_mut().enumerate() {
        if !finished[k] {
            effects(k, stem)?;
        }
    }

    // Master: gain, then hard limit at 0 dBFS.
    let master = agb_dsp::detmath::db_to_gain(p.master_gain_db) as f32;
    let mut mix = vec![0f32; total];
    let mut mix_clips = 0u64;
    let mut stem_metrics = std::collections::BTreeMap::new();
    let mut stems_out = vec![];
    for (k, stem) in stems.iter_mut().enumerate() {
        let mut clips = 0u64;
        for (i, x) in stem.iter_mut().enumerate() {
            let v = *x * master;
            mix[i] += v;
            if v.abs() > 1.0 {
                clips += 1;
            }
            *x = v.clamp(-1.0, 1.0);
        }
        let id = sched.track_order[k].clone();
        stem_metrics.insert(id.clone(), buffer_metrics(stem, clips, window as usize));
        stems_out.push((id, std::mem::take(stem)));
    }
    let soft = p.master_limiter == MasterLimiter::Soft;
    for x in mix.iter_mut() {
        if x.abs() > 1.0 {
            mix_clips += 1;
        }
        if soft {
            *x = soft_limit(*x);
        }
        *x = x.clamp(-1.0, 1.0);
    }

    let metrics = Metrics {
        mix: buffer_metrics(&mix, mix_clips, window as usize),
        stems: stem_metrics,
        note_on_count: if p.panic.active { 0 } else { sched.events.len() },
        voices_stolen: stolen,
        max_concurrent_voices: max_conc,
        mix_sha256: String::new(),
        stem_sha256: Default::default(),
    };
    let info = RenderInfo {
        spec_version: p.spec_version.clone(),
        engine_version: ENGINE_VERSION,
        project_name: p.name.clone(),
        project_revision: p.revision,
        sample_rate: sr,
        channels: 1,
        format: req.format,
        seed: req.seed,
        from_tick: req.from_tick,
        to_tick: req.to_tick,
        tail_ms: req.tail_ms,
        tempo_bpm: p.tempo_bpm,
        ppqn: p.ppqn,
        steps_per_quarter: p.steps_per_quarter,
        ticks_per_step: sched.ticks_per_step,
        window_samples: window,
        tail_samples: tail,
        total_samples: total as u64,
        master_gain_db: p.master_gain_db,
        steal_policy: p.steal_policy,
        panic_active: p.panic.active,
    };
    let mut sched = sched;
    if p.panic.active {
        sched.events.clear(); // queue cleared by panic
    }
    Ok(Rendered { start_tick, info, schedule: sched, mix, stems: if req.stems { stems_out } else { vec![] }, metrics })
}

/// Encode a mono buffer as WAV bytes.
pub fn encode_wav(buf: &[f32], sample_rate: u32, format: SampleFormat) -> Vec<u8> {
    let spec = match format {
        SampleFormat::Pcm16 => hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
        SampleFormat::F32 => hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    };
    let mut cur = std::io::Cursor::new(Vec::with_capacity(buf.len() * 4 + 64));
    {
        let mut w = hound::WavWriter::new(&mut cur, spec).expect("wav header");
        for &x in buf {
            let x = x.clamp(-1.0, 1.0);
            match format {
                // f32::round = round half away from zero.
                SampleFormat::Pcm16 => w.write_sample((x * 32767.0).round() as i16),
                SampleFormat::F32 => w.write_sample(x),
            }
            .expect("in-memory write");
        }
        w.finalize().expect("finalize");
    }
    cur.into_inner()
}

pub fn sha256_tag(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

/// Encoded artifacts ready to write.
pub struct Encoded {
    pub files: Vec<(PathBuf, Vec<u8>)>,
    pub metrics: Metrics,
}

/// Encode all outputs. `mix_name` is the mix file name inside `out_dir`.
pub fn encode(r: &Rendered, mix_name: &str) -> Encoded {
    let mut files = vec![];
    let mut metrics = r.metrics.clone();
    let mix = encode_wav(&r.mix, r.info.sample_rate, r.info.format);
    metrics.mix_sha256 = sha256_tag(&mix);
    files.push((PathBuf::from(mix_name), mix));
    for (id, buf) in &r.stems {
        let b = encode_wav(buf, r.info.sample_rate, r.info.format);
        metrics.stem_sha256.insert(id.clone(), sha256_tag(&b));
        files.push((PathBuf::from("stems").join(format!("{id}.wav")), b));
    }
    let pretty = |v: &serde_json::Value| {
        let mut b = serde_json::to_vec_pretty(v).unwrap();
        b.push(b'\n');
        b
    };
    files.push(("events.json".into(), pretty(&serde_json::to_value(&r.schedule).unwrap())));
    files.push(("metrics.json".into(), pretty(&serde_json::to_value(&metrics).unwrap())));
    files.push(("render.json".into(), pretty(&serde_json::to_value(&r.info).unwrap())));
    Encoded { files, metrics }
}

/// Render and write to `out_dir`. Returns a JSON summary.
pub fn render_to_dir(
    p: &Project,
    req: &RenderRequest,
    out_dir: &Path,
    mix_name: &str,
) -> Result<serde_json::Value, AgbError> {
    let r = render(p, req)?;
    let enc = encode(&r, mix_name);
    std::fs::create_dir_all(out_dir).map_err(AgbError::io)?;
    let mut written = vec![];
    for (rel, bytes) in &enc.files {
        let path = out_dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(AgbError::io)?;
        }
        std::fs::write(&path, bytes).map_err(AgbError::io)?;
        written.push(rel.to_string_lossy().replace('\\', "/"));
    }
    let m = &enc.metrics;
    Ok(json!({
        "op": "render.offline",
        "outDir": out_dir.to_string_lossy(),
        "files": written,
        "mixSha256": m.mix_sha256,
        "peakDbfs": m.mix.peak_dbfs,
        "clipCount": m.mix.clip_count,
        "nearSilent": m.mix.near_silent,
        "noteOnCount": m.note_on_count,
        "totalSamples": r.info.total_samples,
        "fromTick": req.from_tick,
        "toTick": req.to_tick,
        "seed": req.seed,
        "commonOrigins": r.schedule.common_origins,
        "panicActive": r.info.panic_active,
    }))
}

/// [`RenderHook`] used by `txn apply`: writes into `<projectRoot>/<outDir>`.
pub struct DiskRenderer {
    pub project_root: PathBuf,
}

impl RenderHook for DiskRenderer {
    fn render(&mut self, project: &Project, op: &RenderOffline) -> Result<serde_json::Value, AgbError> {
        let req = RenderRequest::from_op(project, op);
        let mut v = render_to_dir(project, &req, &self.project_root.join(&op.out_dir), "mix.wav")?;
        v["outDir"] = json!(op.out_dir);
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agb_protocol::parse_transaction;

    const EX1: &str = include_str!("../../../examples/polymeter_280.json");

    fn project_ex1() -> Project {
        let (_, t) = parse_transaction(EX1).unwrap();
        agb_engine::plan(&Project::default(), &t).unwrap().project
    }

    fn req(tail_ms: u32) -> RenderRequest {
        RenderRequest { from_tick: 0, to_tick: 134_400, tail_ms, seed: 7, stems: true, format: SampleFormat::Pcm16 }
    }

    /// T_render_deterministic
    #[test]
    fn t_render_deterministic() {
        let p = project_ex1();
        let a = encode(&render(&p, &req(500)).unwrap(), "mix.wav");
        let b = encode(&render(&p, &req(500)).unwrap(), "mix.wav");
        assert_eq!(a.metrics.mix_sha256, b.metrics.mix_sha256);
        assert_eq!(a.metrics.stem_sha256, b.metrics.stem_sha256);
        assert!(!a.metrics.mix.near_silent);
        assert_eq!(a.metrics.mix.clip_count, 0);
    }

    /// T_render_tail
    #[test]
    fn t_render_tail() {
        let p = project_ex1();
        let with = render(&p, &req(500)).unwrap();
        let without = render(&p, &req(0)).unwrap();
        assert_eq!(with.info.window_samples, 3_360_000);
        assert_eq!(with.info.tail_samples, 24_000);
        assert_eq!(with.mix.len(), 3_360_000 + 24_000);
        assert_eq!(without.mix.len(), 3_360_000);
        assert!(with.metrics.mix.tail_energy > 0.0, "release continues into the tail");
        assert_eq!(without.metrics.mix.tail_energy, 0.0);
        // The window itself is identical; the tail only appends.
        assert_eq!(&with.mix[..3_360_000], &without.mix[..]);
    }

    /// T_panic
    #[test]
    fn t_panic() {
        let mut p = project_ex1();
        p.panic.active = true;
        let r = render(&p, &req(500)).unwrap();
        assert!(r.metrics.mix.near_silent);
        assert!(r.metrics.mix.peak_dbfs.map_or(true, |d| d < NEAR_SILENT_DBFS));
        assert!(r.schedule.events.is_empty());
    }

    #[test]
    fn seed_changes_noise_only() {
        let mut p = project_ex1();
        // Make the drums a noise voice: different seeds → different PCM.
        p.timeline[0].changes[0] = agb_score::Change::UpsertTrack {
            patch: agb_score::TrackPatch {
                track_id: "drums".into(),
                gain_db: Some(-6.0),
                preset_id: Some(Some("preset:synth.waveform:perc:noise_hat@v1.0".into())),
                ..Default::default()
            },
        };
        let a = encode(&render(&p, &req(0)).unwrap(), "m").metrics.mix_sha256;
        let mut r2 = req(0);
        r2.seed = 8;
        let b = encode(&render(&p, &r2).unwrap(), "m").metrics.mix_sha256;
        assert_ne!(a, b);
        // Seed has no effect on a project without noise.
        let q = project_ex1();
        let c = encode(&render(&q, &req(0)).unwrap(), "m").metrics.mix_sha256;
        let d = encode(&render(&q, &r2).unwrap(), "m").metrics.mix_sha256;
        assert_eq!(c, d);
    }

    #[test]
    fn voice_budget_steals() {
        // 40 simultaneous notes on one step → 8 steals with a 32-voice budget.
        let notes: Vec<_> = (0..40).map(|i| serde_json::json!({"step":0,"pitch":40+i,"durSteps":4,"velocity":0.1})).collect();
        let t = serde_json::json!({"commandId":"v","actor":"agent.t","quantize":{"type":"tick","tick":0},"ops":[
            {"op":"track.upsert","trackId":"a","params":{"osc.wave":"sine"}},
            {"op":"layer.upsert","trackId":"a","layerId":"l","lengthSteps":8,"notes":notes}
        ]});
        let (_, t) = parse_transaction(&t.to_string()).unwrap();
        let p = agb_engine::plan(&Project::default(), &t).unwrap().project;
        let r = render(&p, &RenderRequest { to_tick: 8 * 480, ..req(0) }).unwrap();
        assert_eq!(r.metrics.voices_stolen, 8);
        assert_eq!(r.metrics.max_concurrent_voices, 32);
    }

    #[test]
    fn pcm16_rounding() {
        let b = encode_wav(&[0.0, 1.0, -1.0, 0.5, 2.0, 1.0 / 65534.0], 48000, SampleFormat::Pcm16);
        let mut rd = hound::WavReader::new(std::io::Cursor::new(b)).unwrap();
        let s: Vec<i16> = rd.samples::<i16>().map(|x| x.unwrap()).collect();
        // 0.5*32767 = 16383.5 → 16384 (half away from zero); 1/65534*32767 = 0.5 → 1
        assert_eq!(s, vec![0, 32767, -32767, 16384, 32767, 1]);
    }
}
