//! Project normalisation, validation and voice-parameter resolution.

use crate::clock::ticks_per_step;
use agb_dsp::{engine_status, EngineParams, EngineStatus};
use agb_protocol::{AgbError, ErrorCode};
use agb_score::timeline::{head, snapshot_at};
use agb_score::{Change, Layer, Project, Snapshot, TimelineEntry, Track};

/// Budgets (§12.1) — design budgets, not measured limits.
pub const MAX_TRACKS: usize = 8;
pub const MAX_LAYERS_PER_TRACK: usize = 4;
pub const MAX_SCENES: usize = 8;
pub const MAX_VOICES: usize = 32;

/// Default trigger for `steps`-only layers.
pub const STEP_TRIGGER_PITCH: u8 = 36; // C2
pub const STEP_TRIGGER_VELOCITY: f64 = 0.8;

fn sv(msg: impl Into<String>) -> AgbError {
    AgbError::new(ErrorCode::SchemaViolation, msg)
}

/// Hand-written projects may carry `tracks` without a `timeline`: turn them
/// into a genesis entry so the timeline is always authoritative.
pub fn normalize(mut p: Project) -> Project {
    if p.timeline.is_empty() && !p.tracks.is_empty() {
        p.timeline.push(TimelineEntry {
            revision: 0,
            command_id: "genesis".into(),
            effective_tick: p.origin_tick,
            changes: p.tracks.iter().map(|t| Change::PutTrack { track: t.clone() }).collect(),
        });
    }
    p
}

/// Recompute the materialised head view (`project.tracks`).
pub fn refresh_head(p: &mut Project) {
    p.tracks = head(&p.timeline).tracks;
}

/// `[a-zA-Z0-9_-]{1,64}`
pub fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub fn check_id(kind: &str, s: &str) -> Result<(), AgbError> {
    if valid_id(s) {
        Ok(())
    } else {
        Err(sv(format!("{kind} {s:?} must match [a-zA-Z0-9_-]{{1,64}}")))
    }
}

pub fn check_range(name: &str, v: f64, lo: f64, hi: f64) -> Result<(), AgbError> {
    if v.is_finite() && v >= lo && v <= hi {
        Ok(())
    } else {
        Err(sv(format!("{name}={v} out of range {lo}..={hi}")))
    }
}

pub fn check_engine(engine_id: &str) -> Result<(), AgbError> {
    match engine_status(engine_id) {
        EngineStatus::Available => Ok(()),
        EngineStatus::Planned(phase) => Err(AgbError::new(
            ErrorCode::UnsupportedInPhase,
            format!("engine {engine_id} is planned for {phase}; P0 provides synth.waveform"),
        )),
        EngineStatus::Unknown => {
            Err(AgbError::new(ErrorCode::UnknownEngine, format!("unknown engine {engine_id:?}")))
        }
    }
}

pub fn check_preset_exists(preset_id: &str) -> Result<&'static agb_presets::Preset, AgbError> {
    agb_presets::get(preset_id)
        .ok_or_else(|| AgbError::new(ErrorCode::UnknownPreset, format!("unknown preset {preset_id:?}")))
}

/// Tempo map of a project (base tempo + changes).
pub fn tempo_map(p: &Project) -> crate::clock::TempoMap {
    let pts: Vec<(u64, f64)> = p.tempo_map.iter().map(|t| (t.tick, t.bpm)).collect();
    crate::clock::TempoMap::new(p.tempo_bpm, &pts, p.ppqn, p.sample_rate)
}

/// Project-level fields (§7.1).
pub fn validate_globals(p: &Project) -> Result<(), AgbError> {
    if p.name.is_empty() || p.name.chars().count() > 128 {
        return Err(sv("name must be 1-128 characters"));
    }
    check_range("tempoBpm", p.tempo_bpm, 20.0, 400.0)?;
    for t in &p.tempo_map {
        check_range("tempoMap.bpm", t.bpm, 20.0, 400.0)?;
    }
    if p.steps_per_quarter < 1 {
        return Err(sv("stepsPerQuarter must be >= 1"));
    }
    if ticks_per_step(p.ppqn, p.steps_per_quarter).is_none() {
        return Err(sv(format!(
            "ppqn {} must be divisible by stepsPerQuarter {}",
            p.ppqn, p.steps_per_quarter
        )));
    }
    if p.sample_rate != 44100 && p.sample_rate != 48000 {
        return Err(sv("sampleRate must be 44100 or 48000 in P0"));
    }
    check_range("masterGainDb", p.master_gain_db, -60.0, 0.0)?;
    if p.scenes.len() > MAX_SCENES {
        return Err(AgbError::new(
            ErrorCode::BudgetExceeded,
            format!("{} scenes exceed budget {MAX_SCENES}", p.scenes.len()),
        ));
    }
    Ok(())
}

/// Per-note / per-layer field checks that need no context.
pub fn validate_layer(track_id: &str, l: &Layer) -> Result<(), AgbError> {
    let ctx = format!("layer {track_id}/{}", l.id);
    check_id("layerId", &l.id)?;
    if l.length_steps < 1 {
        return Err(sv(format!("{ctx}: lengthSteps must be >= 1")));
    }
    check_range(&format!("{ctx}: gainDb"), l.gain_db, -60.0, 12.0)?;
    if let Some(steps) = &l.steps {
        if steps.len() != l.length_steps as usize {
            return Err(sv(format!(
                "{ctx}: steps has {} entries but lengthSteps is {}",
                steps.len(),
                l.length_steps
            )));
        }
        if steps.iter().any(|s| *s > 1) {
            return Err(sv(format!("{ctx}: steps entries must be 0 or 1")));
        }
    }
    if let Some(notes) = &l.notes {
        for (i, n) in notes.iter().enumerate() {
            if n.step >= l.length_steps {
                return Err(sv(format!(
                    "{ctx}: notes[{i}].step {} must be < lengthSteps {}",
                    n.step, l.length_steps
                )));
            }
            if n.dur_steps < 1 {
                return Err(sv(format!("{ctx}: notes[{i}].durSteps must be >= 1")));
            }
            check_range(&format!("{ctx}: notes[{i}].velocity"), n.velocity, 0.0, 1.0)?;
            n.pitch.to_midi().map_err(|e| sv(format!("{ctx}: notes[{i}]: {e}")))?;
        }
    }
    Ok(())
}

/// Resolved synthesis settings for one track.
#[derive(Clone, Debug)]
pub struct VoiceConfig {
    pub params: EngineParams,
    pub preset_gain_db: f64,
}

/// defaults ← preset parameters ← track `params` overrides.
pub fn resolve_voice(track: &Track, preset_override: Option<&str>) -> Result<VoiceConfig, AgbError> {
    check_engine(&track.engine_id)?;
    let preset_id = preset_override.or(track.preset_id.as_deref());
    let preset = preset_id.map(check_preset_exists).transpose()?;
    if let Some(pr) = preset {
        if pr.engine != track.engine_id {
            return Err(sv(format!(
                "track {}: preset {} is for engine {}, track engine is {}",
                track.id, pr.id, pr.engine, track.engine_id
            )));
        }
    }
    let empty = Default::default();
    let pmap = preset.map(|p| &p.parameters).unwrap_or(&empty);
    let params = EngineParams::resolve(&track.engine_id, &[pmap, &track.params])
        .map_err(|e| sv(format!("track {}: {e}", track.id)))?;
    Ok(VoiceConfig { params, preset_gain_db: preset.map_or(0.0, |p| p.gain) })
}

pub fn validate_track(t: &Track) -> Result<(), AgbError> {
    check_id("trackId", &t.id)?;
    check_range(&format!("track {}: gainDb", t.id), t.gain_db, -60.0, 12.0)?;
    resolve_voice(t, None)?;
    if t.layers.len() > MAX_LAYERS_PER_TRACK {
        return Err(AgbError::new(
            ErrorCode::BudgetExceeded,
            format!("track {}: {} layers exceed budget {MAX_LAYERS_PER_TRACK}", t.id, t.layers.len()),
        ));
    }
    for l in &t.layers {
        validate_layer(&t.id, l)?;
    }
    Ok(())
}

pub fn validate_snapshot(s: &Snapshot) -> Result<(), AgbError> {
    if s.tracks.len() > MAX_TRACKS {
        return Err(AgbError::new(
            ErrorCode::BudgetExceeded,
            format!("{} tracks exceed budget {MAX_TRACKS}", s.tracks.len()),
        ));
    }
    for t in &s.tracks {
        validate_track(t)?;
    }
    Ok(())
}

/// Validate every score state in force from `from_tick` on (one per distinct
/// change tick), so that an insertion in the past cannot invalidate the future.
pub fn validate_timeline(p: &Project, from_tick: u64) -> Result<(), AgbError> {
    let mut ticks: Vec<u64> = p
        .timeline
        .iter()
        .map(|e| e.effective_tick)
        .filter(|t| *t >= from_tick)
        .collect();
    ticks.push(from_tick);
    ticks.sort_unstable();
    ticks.dedup();
    for t in ticks {
        validate_snapshot(&snapshot_at(&p.timeline, t))
            .map_err(|e| AgbError { message: format!("at tick {t}: {}", e.message), ..e })?;
    }
    Ok(())
}

/// Scenes: ids, step alignment, patches.
pub fn validate_scenes(p: &Project) -> Result<(), AgbError> {
    let tps = ticks_per_step(p.ppqn, p.steps_per_quarter).unwrap_or(1);
    let head = head(&p.timeline);
    for s in &p.scenes {
        check_id("sceneId", &s.id)?;
        if let Some(t) = s.launch_at_tick {
            if t < p.origin_tick || (t - p.origin_tick) % tps != 0 {
                return Err(sv(format!(
                    "scene {}: launchAtTick {t} must be >= originTick and a multiple of ticksPerStep {tps}",
                    s.id
                )));
            }
        }
        for (track, preset) in &s.track_patches {
            let Some(tr) = head.track(track) else {
                return Err(AgbError::new(
                    ErrorCode::UnknownTrack,
                    format!("scene {}: trackPatches references unknown track {track:?}", s.id),
                ));
            };
            resolve_voice(tr, Some(preset))?;
        }
    }
    Ok(())
}
