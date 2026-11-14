//! Scheduler: turns committed score state into note events, automation lanes
//! and voice cuts.
//!
//! All layers share one global step grid anchored at `originTick`. A layer's
//! local step at global step `g` is `(g - o + phaseOffsetSteps) mod lengthSteps`,
//! where `o` is the step of the latest launched `hard_reset_on_scene` scene
//! (or 0). `free_phase` scenes do not move `o`.

use crate::clock::{lcm_all, ticks_per_step};
use crate::state::{resolve_voice, STEP_TRIGGER_PITCH, STEP_TRIGGER_VELOCITY};
use agb_dsp::detmath::db_to_gain;
use agb_dsp::EngineParams;
use agb_protocol::{AgbError, ErrorCode};
use agb_score::timeline::{head, ordered, snapshot_at, Cursor};
use agb_score::{AutomationLane, Change, PhasePolicy, Project, Scene, Track};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct NoteEvent {
    pub note_id: u64,
    pub track_id: String,
    /// Layer id, or `"clip"` for `clip.schedule` notes.
    pub layer_id: String,
    pub tick: u64,
    pub off_tick: u64,
    /// Global step containing `tick` (exact for layer notes).
    pub global_step: u64,
    pub layer_step: u32,
    pub pitch: u8,
    pub velocity: f64,
    pub engine_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset_id: Option<String>,
    /// Index into [`Schedule::track_order`].
    #[serde(skip)]
    pub track_index: usize,
    /// velocity × layer gain × track gain × preset gain (linear).
    #[serde(skip)]
    pub amp: f64,
    #[serde(skip)]
    pub params: EngineParams,
    /// Semitones added to the written pitch (project transposition).
    #[serde(skip)]
    pub transpose: i32,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SceneLaunchInfo {
    pub scene_id: String,
    pub tick: u64,
    pub step: u64,
    pub phase_policy: PhasePolicy,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LaneInfo {
    #[serde(skip)]
    pub track_index: usize,
    #[serde(flatten)]
    pub lane: AutomationLane,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CutInfo {
    pub tick: u64,
    pub track_id: String,
    #[serde(skip)]
    pub track_index: usize,
}

/// Result of scheduling a window `[fromTick, toTick)`.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub from_tick: u64,
    pub to_tick: u64,
    pub ticks_per_step: u64,
    /// First global step at or after `fromTick`.
    pub from_step: u64,
    /// Last global step at or before `toTick` (inclusive, for phase checks).
    pub to_step: u64,
    /// LCM of all layer lengths in the head state.
    pub loop_lcm_steps: u64,
    pub track_order: Vec<String>,
    pub scene_launches: Vec<SceneLaunchInfo>,
    /// `"track/layer"` → global steps where that layer is at its local step 0,
    /// over `[fromStep, toStep]` inclusive.
    pub layer_cycle_starts: BTreeMap<String, Vec<u64>>,
    /// Global steps where *every* existing layer is at local step 0 simultaneously.
    pub common_origins: Vec<u64>,
    /// Automation lanes overlapping the window (applied by the renderer).
    pub automation: Vec<LaneInfo>,
    /// Voice cuts inside the window.
    pub cuts: Vec<CutInfo>,
    pub events: Vec<NoteEvent>,
}

/// Default render end: one full polymeter cycle from `originTick`.
pub fn default_to_tick(p: &Project) -> u64 {
    let tps = ticks_per_step(p.ppqn, p.steps_per_quarter).unwrap_or(1);
    p.origin_tick + loop_lcm_steps(p) * tps
}

pub fn loop_lcm_steps(p: &Project) -> u64 {
    let h = head(&p.timeline);
    lcm_all(h.tracks.iter().flat_map(|t| t.layers.iter().map(|l| l.length_steps as u64)))
}

fn launched(p: &Project, tps: u64) -> Vec<(&Scene, u64, u64)> {
    let mut v: Vec<(&Scene, u64, u64)> = p
        .scenes
        .iter()
        .filter_map(|s| s.launch_at_tick.map(|t| (s, t, (t - p.origin_tick) / tps)))
        .collect();
    v.sort_by(|a, b| (a.1, &a.0.id).cmp(&(b.1, &b.0.id)));
    v
}

/// Phase origin step and preset patches in force at `tick`.
fn scene_state<'a>(scenes: &[(&'a Scene, u64, u64)], tick: u64) -> (u64, BTreeMap<&'a str, &'a str>) {
    let mut o = 0u64;
    let mut patches = BTreeMap::new();
    for (s, t, st) in scenes {
        if *t > tick {
            break;
        }
        if s.phase_policy == PhasePolicy::HardResetOnScene {
            o = *st;
        }
        for (tr, pr) in &s.track_patches {
            patches.insert(tr.as_str(), pr.as_str());
        }
    }
    (o, patches)
}

#[allow(clippy::too_many_arguments)]
fn make_event(
    track: &Track,
    track_index: usize,
    preset_override: Option<&str>,
    layer_id: &str,
    layer_gain_db: f64,
    tick: u64,
    off_tick: u64,
    global_step: u64,
    layer_step: u32,
    pitch: u8,
    vel: f64,
    transpose: i32,
) -> Result<Option<NoteEvent>, AgbError> {
    // Written pitch → sounding pitch; a note transposed out of MIDI range is not played.
    let transpose = if track.follows_transpose() { transpose } else { 0 };
    let sounding = pitch as i32 + transpose;
    if !(0..=127).contains(&sounding) {
        return Ok(None);
    }
    let pitch = sounding as u8;
    let c = resolve_voice(track, preset_override)?;
    let gain = db_to_gain(layer_gain_db) * db_to_gain(track.gain_db) * db_to_gain(c.preset_gain_db);
    Ok(Some(NoteEvent {
        transpose,
        note_id: 0,
        track_id: track.id.clone(),
        layer_id: layer_id.to_string(),
        tick,
        off_tick,
        global_step,
        layer_step,
        pitch,
        velocity: vel,
        engine_id: track.engine_id.clone(),
        preset_id: preset_override.map(str::to_string).or_else(|| track.preset_id.clone()),
        track_index,
        amp: vel * gain,
        params: c.params,
    }))
}

pub fn schedule(p: &Project, from_tick: u64, to_tick: u64) -> Result<Schedule, AgbError> {
    if to_tick <= from_tick {
        return Err(AgbError::new(
            ErrorCode::SchemaViolation,
            format!("toTick {to_tick} must be > fromTick {from_tick}"),
        ));
    }
    let tps = ticks_per_step(p.ppqn, p.steps_per_quarter)
        .ok_or_else(|| AgbError::new(ErrorCode::SchemaViolation, "ppqn not divisible by stepsPerQuarter"))?;
    let origin = p.origin_tick;
    let track_order: Vec<String> = head(&p.timeline).tracks.iter().map(|t| t.id.clone()).collect();
    let tindex = |id: &str| track_order.iter().position(|t| t == id).unwrap_or(0);
    let scenes = launched(p, tps);

    let from_step = if from_tick <= origin { 0 } else { (from_tick - origin).div_ceil(tps) };
    let to_step_incl = if to_tick < origin { None } else { Some((to_tick - origin) / tps) };

    let mut out = Schedule {
        from_tick,
        to_tick,
        ticks_per_step: tps,
        from_step,
        to_step: to_step_incl.unwrap_or(0),
        loop_lcm_steps: loop_lcm_steps(p),
        track_order: track_order.clone(),
        scene_launches: scenes
            .iter()
            .map(|(s, t, st)| SceneLaunchInfo {
                scene_id: s.id.clone(),
                tick: *t,
                step: *st,
                phase_policy: s.phase_policy,
            })
            .collect(),
        layer_cycle_starts: BTreeMap::new(),
        common_origins: vec![],
        automation: vec![],
        cuts: vec![],
        events: vec![],
    };

    // `clip.clear`: (track, from, to, revision) — hides clip notes of earlier revisions.
    let clears: Vec<(&str, u64, u64, u64)> = p
        .timeline
        .iter()
        .flat_map(|e| {
            e.changes.iter().filter_map(move |c| match c {
                Change::ClipClear { track_id, to_tick } => {
                    Some((track_id.as_str(), e.effective_tick, to_tick.unwrap_or(u64::MAX), e.revision))
                }
                _ => None,
            })
        })
        .collect();

    // Time-addressed changes: automation, clips, cuts.
    for e in ordered(&p.timeline) {
        for c in &e.changes {
            match c {
                Change::Automation { lane } if lane.from_tick < to_tick => out
                    .automation
                    .push(LaneInfo { track_index: tindex(&lane.track_id), lane: lane.clone() }),
                Change::Cut { track_id } if (from_tick..to_tick).contains(&e.effective_tick) => {
                    out.cuts.push(CutInfo {
                        tick: e.effective_tick,
                        track_id: track_id.clone(),
                        track_index: tindex(track_id),
                    })
                }
                Change::Clip { track_id, notes } => {
                    for n in notes {
                        if !(from_tick..to_tick).contains(&n.tick) {
                            continue;
                        }
                        if clears.iter().any(|(t, a, b, rev)| *t == track_id && *rev > e.revision && (*a..*b).contains(&n.tick)) {
                            continue;
                        }
                        let snap = snapshot_at(&p.timeline, n.tick);
                        let Some(track) = snap.track(track_id) else { continue };
                        if track.mute {
                            continue;
                        }
                        let (_, patches) = scene_state(&scenes, n.tick);
                        let g = n.tick.saturating_sub(origin) / tps;
                        out.events.extend(make_event(
                            track,
                            tindex(track_id),
                            patches.get(track_id.as_str()).copied(),
                            "clip",
                            0.0,
                            n.tick,
                            n.tick + n.dur_ticks,
                            g,
                            0,
                            n.pitch.to_midi().unwrap_or(STEP_TRIGGER_PITCH),
                            n.velocity.unwrap_or(0.8),
                            p.transpose_at(n.tick),
                        )?);
                    }
                }
                _ => {}
            }
        }
    }

    if let Some(to_step) = to_step_incl {
        let mut cursor = Cursor::new(&p.timeline);
        for g in from_step..=to_step {
            let tick = origin + g * tps;
            cursor.advance_to(tick);
            let (o, patches) = scene_state(&scenes, tick);
            let emit = tick < to_tick;
            let mut any_layer = false;
            let mut all_zero = true;
            for track in &cursor.snapshot.tracks {
                let track_index = tindex(&track.id);
                let preset_override = patches.get(track.id.as_str()).copied();
                for layer in &track.layers {
                    let len = layer.length_steps.max(1) as i64;
                    let local =
                        ((g as i64 - o as i64) + layer.phase_offset_steps as i64).rem_euclid(len) as u32;
                    any_layer = true;
                    if local == 0 {
                        out.layer_cycle_starts.entry(format!("{}/{}", track.id, layer.id)).or_default().push(g);
                    } else {
                        all_zero = false;
                    }
                    if !emit || track.mute || layer.mute {
                        continue;
                    }
                    let hits: Vec<(u8, u32, f64)> = match &layer.notes {
                        Some(notes) if !notes.is_empty() => notes
                            .iter()
                            .filter(|n| n.step == local)
                            .map(|n| (n.pitch.to_midi().unwrap_or(STEP_TRIGGER_PITCH), n.dur_steps, n.velocity))
                            .collect(),
                        _ => match &layer.steps {
                            Some(st) if st.get(local as usize) == Some(&1) => {
                                vec![(STEP_TRIGGER_PITCH, 1, STEP_TRIGGER_VELOCITY)]
                            }
                            _ => vec![],
                        },
                    };
                    for (pitch, dur, vel) in hits {
                        out.events.extend(make_event(
                            track,
                            track_index,
                            preset_override,
                            &layer.id,
                            layer.gain_db,
                            tick,
                            tick + dur as u64 * tps,
                            g,
                            local,
                            pitch,
                            vel,
                            p.transpose_at(tick),
                        )?);
                    }
                }
            }
            if any_layer && all_zero {
                out.common_origins.push(g);
            }
        }
    }

    // Stable order by tick: same-tick clip notes precede layer notes (pushed first).
    out.events.sort_by_key(|e| e.tick);
    for (i, e) in out.events.iter_mut().enumerate() {
        e.note_id = i as u64;
    }
    Ok(out)
}
