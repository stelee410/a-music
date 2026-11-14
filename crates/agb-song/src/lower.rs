//! Lower a [`Song`] to an engine [`Project`] (the part of the format this
//! build can play) and report everything that was left out.

use crate::model::*;
use crate::validate::Issue;
use agb_dsp::{engine_status, is_numeric_param, EngineStatus};
use agb_engine::clock::ticks_per_step;
use agb_engine::schedule::default_to_tick;
use agb_engine::state::{refresh_head, validate_globals, validate_timeline};
use agb_protocol::{AgbError, ErrorCode};
use agb_score::timeline::{AutomationLane, AutomationMode, Change, LayerPatch, TimelineEntry};
use agb_score::{self as score, ParamValue, Pitch, Project};
use std::collections::BTreeMap;

/// Features this build plays. Anything else in `requires` is a refusal;
/// anything else that is merely used is skipped and reported.
pub const PLAYER_FEATURES: &[&str] = &["core", "automation", "sampler"];

const MAX_CLIP_NOTES: usize = 200_000;
/// Loop-only songs with no declared length play one polymeter cycle, capped.
const MAX_AUTO_STEPS: u64 = 4096;

/// What the listener changed at play time (FORMAT.md §11.1).
#[derive(Clone, Debug, Default)]
pub struct PlayOptions {
    /// Semitones, applied to transposable tracks.
    pub transpose: i32,
    /// Absolute starting tempo; the tempo map is scaled by the same ratio.
    pub bpm: Option<f64>,
    /// Audio files of the package's sample assets on disk (asset id → path). A
    /// `sampler.multi` patch whose samples are not listed here cannot be played.
    pub samples: BTreeMap<String, String>,
}

pub struct Lowered {
    pub project: Project,
    /// Song length in ticks (declared or derived).
    pub length_ticks: u64,
    /// Parts of the file this player did not play.
    pub skipped: Vec<Issue>,
}

struct Voice {
    engine: String,
    preset: Option<String>,
    params: BTreeMap<String, ParamValue>,
}

fn flatten<'a>(p: &'a Patch, out: &mut Vec<&'a Patch>) {
    out.push(p);
    for f in &p.fallback {
        flatten(f, out);
    }
}

/// First candidate (the patch, then its fallbacks) this build can play.
/// The `zones` parameter of the sampler engine: the patch's zones with their
/// audio files resolved. `None` when a sample is not available.
fn sampler_zones(song: &Song, patch: &Patch, samples: &BTreeMap<String, String>) -> Option<String> {
    let mut zones = vec![];
    for z in &patch.zones {
        let path = samples.get(&z.sample)?;
        let root = z
            .root_pitch
            .as_ref()
            .or_else(|| song.assets.get(&z.sample).and_then(|a| a.root_pitch.as_ref()))
            .and_then(|p| p.to_midi().ok())
            .unwrap_or(60);
        let (lo, hi) = z.key_range.unwrap_or((0, 127));
        let (vel_lo, vel_hi) = z.vel_range.unwrap_or((0.0, 1.0));
        zones.push(serde_json::json!({
            "path": path, "lo": lo, "hi": hi, "root": root,
            "tuneCents": z.tune_cents.unwrap_or(0.0), "gainDb": z.gain_db.unwrap_or(0.0),
            "velLo": vel_lo, "velHi": vel_hi, "oneShot": z.one_shot,
            "start": z.start_frame.unwrap_or(0), "end": z.end_frame,
            "loopStart": z.loop_points.as_ref().map(|l| l.start_frame),
            "loopEnd": z.loop_points.as_ref().map(|l| l.end_frame),
        }));
    }
    (!zones.is_empty()).then(|| serde_json::Value::Array(zones).to_string())
}

fn pick(song: &Song, patch: &Patch, samples: &BTreeMap<String, String>) -> Option<(usize, Voice)> {
    let mut cands = vec![];
    flatten(patch, &mut cands);
    for (i, c) in cands.into_iter().enumerate() {
        let preset = c.preset.as_deref().and_then(agb_presets::get);
        if c.preset.is_some() && preset.is_none() {
            continue;
        }
        let Some(engine) = c.engine.clone().or_else(|| preset.map(|p| p.engine.clone())) else { continue };
        if engine_status(&engine) != EngineStatus::Available || preset.is_some_and(|p| p.engine != engine) {
            continue;
        }
        let mut params = c.params.clone();
        if engine == "sampler.multi" {
            let Some(zones) = sampler_zones(song, c, samples) else { continue };
            params.insert("zones".into(), ParamValue::Str(zones));
        }
        return Some((i, Voice { engine, preset: c.preset.clone(), params }));
    }
    None
}

fn shifted(p: &Pitch, shift: i32) -> Option<u8> {
    let m = p.to_midi().ok()? as i32 + shift;
    (0..=127).contains(&m).then_some(m as u8)
}

pub fn lower(song: &Song, opts: &PlayOptions) -> Result<Lowered, AgbError> {
    let unsupported: Vec<&str> =
        song.requires.iter().map(String::as_str).filter(|f| !PLAYER_FEATURES.contains(f)).collect();
    if !unsupported.is_empty() {
        return Err(AgbError::new(
            ErrorCode::UnsupportedInPhase,
            format!("the song requires {unsupported:?}, which this player does not support (it plays {PLAYER_FEATURES:?})"),
        ));
    }
    let tps = ticks_per_step(song.time.ppqn, song.time.steps_per_quarter)
        .ok_or_else(|| AgbError::new(ErrorCode::SchemaViolation, "time: ppqn must be a multiple of stepsPerQuarter"))?;

    let mut skipped: Vec<Issue> = vec![];
    let mut skip = |code: &'static str, path: String, message: String| skipped.push(Issue { code, path, message });

    let scale = opts.bpm.map_or(1.0, |b| b / song.time.bpm);
    let bpm = |b: f64| (b * scale).clamp(20.0, 400.0);

    let mut p = Project {
        name: song.meta.title.clone(),
        tempo_bpm: bpm(song.time.bpm),
        ppqn: song.time.ppqn,
        steps_per_quarter: song.time.steps_per_quarter,
        master_gain_db: song.master.gain_db,
        master_limiter: song.master.limiter,
        seed: song.seed,
        transpose: opts.transpose.clamp(-24, 24),
        tempo_map: song.time.tempo_map.iter().map(|t| score::TempoPoint { tick: t.tick, bpm: bpm(t.bpm) }).collect(),
        ..Project::default()
    };

    let mut genesis = vec![];
    let mut at_zero = vec![]; // clips + automation
    let mut gated: Vec<(u64, Change)> = vec![];
    let mut end = 0u64; // furthest explicit position
    let mut clip_notes = 0usize;

    for (ti, tr) in song.tracks.iter().enumerate() {
        let tp = format!("tracks[{ti}]");
        let Some(patch) = song.patches.get(&tr.patch) else {
            return Err(AgbError::new(ErrorCode::SchemaViolation, format!("{tp}: unknown patch {:?}", tr.patch)));
        };
        let Some((cand, voice)) = pick(song, patch, &opts.samples) else {
            skip(
                "PATCH_UNPLAYABLE",
                tp,
                format!(
                    "track {:?} is silent: patch {:?} (engine {}) and its fallbacks cannot be played by this build",
                    tr.id,
                    tr.patch,
                    patch.engine.as_deref().unwrap_or("?")
                ),
            );
            continue;
        };
        if cand > 0 {
            skip(
                "PATCH_FALLBACK",
                tp.clone(),
                format!("track {:?}: engine {} is not available, playing fallback {} ({})", tr.id, patch.engine.as_deref().unwrap_or("?"), cand, voice.engine),
            );
        }
        if tr.pan != 0.0 {
            skip("PAN", tp.clone(), "pan ignored: the engine renders mono".into());
        }
        if !tr.fx.is_empty() {
            skip("FX", format!("{tp}.fx"), format!("{} effect(s) not rendered", tr.fx.len()));
        }
        if !tr.controllers.is_empty() {
            skip("CONTROLLERS", format!("{tp}.controllers"), format!("{} controller lane(s) not rendered", tr.controllers.len()));
        }
        if !patch.modulation.is_empty() {
            skip("MODULATION", format!("patches.{}", tr.patch), "modulation matrix not rendered".into());
        }

        // Transposition is applied by the engine at scheduling time, so it can change while playing.
        let shift = 0;
        let mut dropped = 0usize;
        let mut expressive = 0usize;

        let mut t = score::Track::new(tr.id.clone());
        t.engine_id = voice.engine.clone();
        t.preset_id = voice.preset.clone();
        t.params = voice.params.clone();
        t.gain_db = tr.gain_db;
        t.mute = tr.mute;
        t.transposable = Some(tr.transposable.unwrap_or(!voice.engine.starts_with("drum.")));

        for l in &tr.loops {
            let mut notes: Vec<score::Note> = vec![];
            for n in &l.notes {
                if n.expression.is_some() || n.detune_cents.is_some() || n.release_velocity.is_some() {
                    expressive += 1;
                }
                match shifted(&n.pitch, shift) {
                    Some(m) => notes.push(score::Note {
                        step: n.step,
                        pitch: Pitch::Midi(m),
                        dur_steps: n.dur_steps,
                        velocity: n.velocity,
                    }),
                    None => dropped += 1,
                }
            }
            if notes.is_empty() && l.notes.is_empty() {
                for (i, on) in l.steps.iter().flatten().enumerate() {
                    if *on == 1 {
                        match shifted(&Pitch::Midi(36), shift) {
                            Some(m) => notes.push(score::Note { step: i as u32, pitch: Pitch::Midi(m), dur_steps: 1, velocity: 0.8 }),
                            None => dropped += 1,
                        }
                    }
                }
            }
            let from = l.from_tick.unwrap_or(0);
            if from == 0 {
                let mut layer = score::Layer::new(l.id.clone());
                layer.length_steps = l.length_steps;
                layer.phase_offset_steps = l.phase_offset_steps;
                layer.mute = l.mute;
                layer.gain_db = l.gain_db;
                layer.notes = Some(notes);
                t.layers.push(layer);
            } else {
                gated.push((
                    from,
                    Change::UpsertLayer {
                        patch: LayerPatch {
                            track_id: tr.id.clone(),
                            layer_id: l.id.clone(),
                            length_steps: Some(l.length_steps),
                            phase_offset_steps: Some(l.phase_offset_steps),
                            mute: Some(l.mute),
                            gain_db: Some(l.gain_db),
                            notes: Some(notes),
                            ..Default::default()
                        },
                    },
                ));
            }
            if let Some(to) = l.to_tick {
                end = end.max(to);
                gated.push((
                    to,
                    Change::UpsertLayer {
                        patch: LayerPatch {
                            track_id: tr.id.clone(),
                            layer_id: l.id.clone(),
                            mute: Some(true),
                            ..Default::default()
                        },
                    },
                ));
            }
        }
        genesis.push(Change::PutTrack { track: t });

        for (ci, c) in tr.clips.iter().enumerate() {
            if c.midi.is_some() {
                skip("MIDI_FILE", format!("{tp}.clips[{ci}]"), "Standard MIDI File clips are not read yet".into());
            }
            let mut notes = vec![];
            for rep in 0..c.repeat.max(1) as u64 {
                let base = c.at_tick + rep * c.length_ticks.unwrap_or(0);
                for n in &c.notes {
                    if c.length_ticks.is_some_and(|len| n.tick >= len) {
                        continue;
                    }
                    if rep == 0 && (n.expression.is_some() || n.detune_cents.is_some() || n.release_velocity.is_some()) {
                        expressive += 1;
                    }
                    match shifted(&n.pitch, shift) {
                        Some(m) => {
                            end = end.max(base + n.tick + n.dur_ticks);
                            notes.push(score::ClipNote {
                                tick: base + n.tick,
                                pitch: Pitch::Midi(m),
                                dur_ticks: n.dur_ticks,
                                velocity: Some(n.velocity),
                            });
                        }
                        None => dropped += 1,
                    }
                }
                if let Some(len) = c.length_ticks {
                    end = end.max(base + len);
                }
            }
            clip_notes += notes.len();
            if clip_notes > MAX_CLIP_NOTES {
                return Err(AgbError::new(ErrorCode::BudgetExceeded, format!("more than {MAX_CLIP_NOTES} clip notes")));
            }
            if !notes.is_empty() {
                at_zero.push(Change::Clip { track_id: tr.id.clone(), notes });
            }
        }

        for (ai, a) in tr.automation.iter().enumerate() {
            let ap = format!("{tp}.automation[{ai}]");
            let Some(param) = a.target.strip_prefix("patch.") else {
                skip("AUTOMATION_TARGET", ap, format!("target {:?} not rendered (only patch.* is)", a.target));
                continue;
            };
            if !is_numeric_param(&voice.engine, param) {
                skip("AUTOMATION_TARGET", ap, format!("{param:?} is not a numeric parameter of {}", voice.engine));
                continue;
            }
            let lane = |mode, from_tick, to_tick, from, to| Change::Automation {
                lane: AutomationLane { track_id: tr.id.clone(), param: param.to_string(), mode, from_tick, to_tick, from, to },
            };
            for (i, pt) in a.points.iter().enumerate() {
                end = end.max(pt.tick());
                if i == 0 {
                    at_zero.push(lane(AutomationMode::Step, pt.tick(), pt.tick(), pt.value(), pt.value()));
                } else {
                    let prev = &a.points[i - 1];
                    let mode = match pt.curve() {
                        Curve::Linear => AutomationMode::Ramp,
                        Curve::Step => AutomationMode::Step,
                    };
                    at_zero.push(lane(mode, prev.tick(), pt.tick(), prev.value(), pt.value()));
                }
            }
        }

        if dropped > 0 {
            skip("PITCH_RANGE", tp.clone(), format!("{dropped} note(s) left MIDI range 0..=127 after transposing by {shift}"));
        }
        if expressive > 0 {
            skip("EXPRESSION", tp, format!("{expressive} note(s) carry expression / detune / release velocity that is not rendered"));
        }
    }

    if !song.master.fx.is_empty() {
        skip("FX", "master.fx".into(), format!("{} master effect(s) not rendered", song.master.fx.len()));
    }
    if !song.master.automation.is_empty() {
        skip("AUTOMATION_TARGET", "master.automation".into(), "master automation not rendered".into());
    }
    if let Some(o) = &song.overlay {
        let on = o.layers.iter().filter(|l| l.enabled).count();
        if on > 0 {
            skip("OVERLAY", "overlay".into(), format!("{on} frequency layer(s) not rendered offline (they are produced by the live player)"));
        }
    }
    if (song.tuning.a4_hz - 440.0).abs() > 1e-9 {
        skip("TUNING", "tuning.a4Hz".into(), format!("rendered at A4 = 440 Hz, not {}", song.tuning.a4_hz));
    }

    // ---- timeline ----
    let mut rev = 0u64;
    let mut push = |p: &mut Project, tick: u64, changes: Vec<Change>| {
        p.timeline.push(TimelineEntry { revision: rev, command_id: format!("song.{rev}"), effective_tick: tick, changes });
        rev += 1;
    };
    push(&mut p, 0, genesis);
    if !at_zero.is_empty() {
        push(&mut p, 0, at_zero);
    }
    gated.sort_by_key(|g| g.0);
    for (tick, change) in gated {
        push(&mut p, tick, vec![change]);
    }
    p.revision = rev - 1;
    refresh_head(&mut p);
    validate_globals(&p)?;
    validate_timeline(&p, 0)?;

    // ---- length ----
    for s in &song.sections {
        end = end.max(s.to_tick);
    }
    let length_ticks = match song.time.length_ticks {
        Some(l) => l,
        None if end > 0 => end,
        None => {
            let cycle = default_to_tick(&p);
            let cap = MAX_AUTO_STEPS * tps;
            if cycle > cap {
                skipped.push(Issue {
                    code: "LENGTH",
                    path: "time.lengthTicks".into(),
                    message: format!("no lengthTicks: one polymeter cycle is {cycle} ticks, playing the first {cap}"),
                });
            }
            cycle.clamp(tps, cap)
        }
    };
    Ok(Lowered { project: p, length_ticks, skipped })
}
