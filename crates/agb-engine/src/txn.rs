//! Transaction processing (SPEC §8): idempotency, revision check, quantize /
//! late policy, atomic op application, commit.
//!
//! Check order: parse → envelope → idempotency replay → unsupported ops →
//! revision → quantize/late → ops (in order; panic state evolves per op) →
//! whole-timeline validation → (dry run stops here) → renders → commit.

use crate::clock::{bar_ticks, ceil_to_grid};
use crate::state::{self, check_engine, check_id, check_preset_exists, check_range};
use crate::store::{sha256_hex, IdempotencyRecord, ProjectStore};
use agb_protocol::{
    parse_transaction, AgbError, ErrorCode, ErrorResponse, LatePolicy, Op, Quantize, RenderOffline,
    Transaction, TxnResponse,
};
use agb_score::timeline::snapshot_at;
use agb_score::{AutomationLane, Change, PanicState, Project, Scene, TimelineEntry, TrackPatch};
use serde_json::json;
use std::time::Duration;

/// Performs `render.offline` ops on behalf of the engine (implemented by `agb-render`).
pub trait RenderHook {
    fn render(&mut self, project: &Project, req: &RenderOffline) -> Result<serde_json::Value, AgbError>;
}

/// Hook that refuses to render (for engine-only use / tests).
pub struct NoRender;

impl RenderHook for NoRender {
    fn render(&mut self, _: &Project, req: &RenderOffline) -> Result<serde_json::Value, AgbError> {
        Ok(serde_json::json!({ "op": "render.offline", "skipped": true, "outDir": req.out_dir }))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ApplyOptions {
    /// Force dry run regardless of the transaction's `dryRun`.
    pub dry_run: bool,
}

/// Outcome of planning a transaction against a project (pure, no I/O).
#[derive(Clone, Debug)]
pub struct Plan {
    pub project: Project,
    pub effective_tick: u64,
    pub warnings: Vec<String>,
    pub renders: Vec<(usize, RenderOffline)>,
    /// Results of read-only ops (e.g. `preset.search`), by op index.
    pub results: Vec<(usize, serde_json::Value)>,
}

fn unknown_track(id: &str, eff: u64) -> AgbError {
    AgbError::new(ErrorCode::UnknownTrack, format!("track {id:?} does not exist at tick {eff}"))
}

fn sv(msg: impl Into<String>) -> AgbError {
    AgbError::new(ErrorCode::SchemaViolation, msg)
}

fn validate_envelope(t: &Transaction) -> Result<(), AgbError> {
    if t.command_id.is_empty() || t.command_id.len() > 256 {
        return Err(sv("commandId must be 1-256 characters"));
    }
    let actor_ok = ["agent.", "human."]
        .iter()
        .any(|p| t.actor.len() > p.len() && t.actor.starts_with(p));
    if !actor_ok {
        return Err(sv(format!("actor {:?} must be agent.* or human.*", t.actor)));
    }
    if t.ops.is_empty() {
        return Err(sv("ops must not be empty"));
    }
    Ok(())
}

/// Resolve the effective tick (§8.2 quantize + latePolicy).
fn effective_tick(p: &Project, t: &Transaction, writes: bool, warnings: &mut Vec<String>) -> Result<u64, AgbError> {
    let now = p.transport_tick;
    let next_bar = ceil_to_grid(now, p.origin_tick, bar_ticks(p.ppqn));
    match &t.quantize {
        Quantize::Immediate {} => {
            if writes {
                Err(sv("quantize.type=immediate is only allowed for system.panic* and read-only ops"))
            } else {
                Ok(now)
            }
        }
        Quantize::NextBar {} => Ok(next_bar),
        Quantize::Tick { tick } => {
            if *tick >= now || !writes {
                return Ok(*tick);
            }
            match t.late_policy {
                LatePolicy::Reject => Err(AgbError::new(
                    ErrorCode::LateRejected,
                    format!("quantize tick {tick} is before transportTick {now} (latePolicy=reject)"),
                )),
                LatePolicy::ShiftToNextLegal => {
                    warnings.push(format!("LATE_SHIFTED: tick {tick} < transport {now}; shifted to next bar {next_bar}"));
                    Ok(next_bar)
                }
                LatePolicy::ApplyAsapWithFlag => {
                    warnings.push(format!("LATE_APPLIED_ASAP: tick {tick} < transport {now}; applied at {now}"));
                    Ok(now)
                }
            }
        }
    }
}

/// Validate fields supplied by a `track.upsert` that can be checked in isolation.
fn check_track_patch(p: &agb_score::TrackPatch) -> Result<(), AgbError> {
    check_id("trackId", &p.track_id)?;
    if let Some(e) = &p.engine_id {
        check_engine(e)?;
    }
    if let Some(Some(pr)) = &p.preset_id {
        check_preset_exists(pr)?;
    }
    if let Some(g) = p.gain_db {
        check_range("gainDb", g, -60.0, 12.0)?;
    }
    Ok(())
}

fn check_layer_patch(p: &agb_score::LayerPatch) -> Result<(), AgbError> {
    check_id("trackId", &p.track_id)?;
    check_id("layerId", &p.layer_id)?;
    if p.length_steps == Some(0) {
        return Err(sv("lengthSteps must be >= 1"));
    }
    if let Some(g) = p.gain_db {
        check_range("gainDb", g, -60.0, 12.0)?;
    }
    Ok(())
}

/// Plan a transaction: everything except I/O and rendering.
pub fn plan(project: &Project, t: &Transaction) -> Result<Plan, AgbError> {
    let mut warnings = vec![];
    for (i, op) in t.ops.iter().enumerate() {
        if let Some(phase) = op.planned_phase() {
            return Err(AgbError::new(
                ErrorCode::UnsupportedInPhase,
                format!("op {} is planned for {phase}; not available in P0 (transaction not applied)", op.name()),
            )
            .at_op(i));
        }
    }
    if let Some(expected) = t.if_match_revision {
        if expected != project.revision {
            return Err(AgbError {
                server_revision: Some(project.revision),
                ..AgbError::new(
                    ErrorCode::RevisionConflict,
                    format!("ifMatchRevision {expected} != current revision {}", project.revision),
                )
            });
        }
    }
    let writes = t.ops.iter().any(Op::writes_score);
    let eff = effective_tick(project, t, writes, &mut warnings)?;

    let mut p = project.clone();
    let new_rev = p.revision + 1;
    let mut snap = snapshot_at(&p.timeline, eff);
    let mut changes = vec![];
    let mut renders = vec![];
    let mut results = vec![];

    for (i, op) in t.ops.iter().enumerate() {
        let r: Result<(), AgbError> = (|| {
            if p.panic.active && op.writes_score() {
                return Err(AgbError::new(
                    ErrorCode::PanicActive,
                    "panic is active; send system.panic.clear before writing the score",
                ));
            }
            match op {
                Op::ProjectSetTempo(s) => {
                    check_range("tempoBpm", s.tempo_bpm, 20.0, 400.0)?;
                    if eff <= p.origin_tick {
                        p.tempo_bpm = s.tempo_bpm;
                    } else {
                        // Tempo change from the effective tick on (tempo map).
                        p.tempo_map.retain(|t| t.tick != eff);
                        p.tempo_map.push(agb_score::TempoPoint { tick: eff, bpm: s.tempo_bpm });
                        p.tempo_map.sort_by_key(|t| t.tick);
                    }
                }
                Op::TrackUpsert(patch) => {
                    check_track_patch(patch)?;
                    // A preset implies its engine: fill engineId when omitted.
                    let mut patch = patch.clone();
                    if patch.engine_id.is_none() {
                        if let Some(Some(pid)) = &patch.preset_id {
                            let preset = check_preset_exists(pid)?;
                            let cur = snap.track(&patch.track_id).map(|t| t.engine_id.clone());
                            if cur.as_deref() != Some(preset.engine.as_str()) {
                                patch.engine_id = Some(preset.engine.clone());
                            }
                        }
                    }
                    let patch = &patch;
                    let c = Change::UpsertTrack { patch: patch.clone() };
                    snap.apply(&c);
                    changes.push(c);
                    state::resolve_voice(snap.track(&patch.track_id).unwrap(), None)?;
                }
                Op::ProjectSetMix(m) => {
                    if let Some(g) = m.master_gain_db {
                        check_range("masterGainDb", g, -60.0, 0.0)?;
                        p.master_gain_db = g;
                    }
                    if let Some(l) = m.master_limiter {
                        p.master_limiter = l;
                    }
                    warnings.push("project.setMix: mix settings are global (whole timeline)".into());
                }
                Op::PresetLoad(pl) => {
                    let preset = check_preset_exists(&pl.preset_id)?;
                    if snap.track(&pl.track_id).is_none() {
                        return Err(unknown_track(&pl.track_id, eff));
                    }
                    let c = Change::UpsertTrack {
                        patch: TrackPatch {
                            track_id: pl.track_id.clone(),
                            engine_id: Some(preset.engine.clone()),
                            preset_id: Some(Some(preset.id.clone())),
                            replace_params: true,
                            ..Default::default()
                        },
                    };
                    snap.apply(&c);
                    changes.push(c);
                    if pl.tail_policy == agb_protocol::TailPolicy::CutOld {
                        changes.push(Change::Cut { track_id: pl.track_id.clone() });
                    }
                    state::resolve_voice(snap.track(&pl.track_id).unwrap(), None)?;
                }
                Op::VoiceSetParams(vp) => {
                    if snap.track(&vp.track_id).is_none() {
                        return Err(unknown_track(&vp.track_id, eff));
                    }
                    let c = Change::UpsertTrack {
                        patch: TrackPatch {
                            track_id: vp.track_id.clone(),
                            params: Some(vp.params.clone()),
                            ..Default::default()
                        },
                    };
                    snap.apply(&c);
                    changes.push(c);
                    state::resolve_voice(snap.track(&vp.track_id).unwrap(), None)?;
                }
                Op::AutomationSchedule(a) => {
                    let Some(tr) = snap.track(&a.track_id) else {
                        return Err(unknown_track(&a.track_id, eff));
                    };
                    let metas = agb_dsp::EngineParams::describe(&tr.engine_id).unwrap_or_default();
                    let Some(meta) = metas
                        .iter()
                        .find(|m| m.path == a.param && m.kind == agb_dsp::ParamKind::Continuous)
                    else {
                        return Err(sv(format!(
                            "automation.schedule: {:?} is not a numeric param of {}",
                            a.param, tr.engine_id
                        )));
                    };
                    let (lo, hi) = (meta.min.unwrap(), meta.max.unwrap());
                    check_range("automation from", a.from, lo, hi)?;
                    check_range("automation to", a.to, lo, hi)?;
                    if a.from_tick < eff {
                        return Err(AgbError::new(
                            ErrorCode::LateRejected,
                            format!("automation fromTick {} is before the effective tick {eff}", a.from_tick),
                        ));
                    }
                    if a.to_tick < a.from_tick {
                        return Err(sv("automation toTick must be >= fromTick"));
                    }
                    changes.push(Change::Automation {
                        lane: AutomationLane {
                            track_id: a.track_id.clone(),
                            param: a.param.clone(),
                            mode: a.mode,
                            from_tick: a.from_tick,
                            to_tick: a.to_tick,
                            from: a.from,
                            to: a.to,
                        },
                    });
                }
                Op::ClipSchedule(cs) => {
                    if snap.track(&cs.track_id).is_none() {
                        return Err(unknown_track(&cs.track_id, eff));
                    }
                    for (k, n) in cs.notes.iter().enumerate() {
                        if n.tick < eff {
                            return Err(AgbError::new(
                                ErrorCode::LateRejected,
                                format!("clip note {k} tick {} is before the effective tick {eff}", n.tick),
                            ));
                        }
                        if n.dur_ticks < 1 {
                            return Err(sv(format!("clip note {k}: durTicks must be >= 1")));
                        }
                        if let Some(v) = n.velocity {
                            check_range(&format!("clip note {k} velocity"), v, 0.0, 1.0)?;
                        }
                        n.pitch.to_midi().map_err(|e| sv(format!("clip note {k}: {e}")))?;
                    }
                    if cs.notes.len() > 4096 {
                        return Err(AgbError::new(ErrorCode::BudgetExceeded, "at most 4096 clip notes per op"));
                    }
                    changes.push(Change::Clip { track_id: cs.track_id.clone(), notes: cs.notes.clone() });
                }
                Op::PresetSearch(q) => {
                    let hits = agb_presets::search(&q.tags, q.engine.as_deref(), q.role.as_deref());
                    results.push((
                        i,
                        json!({
                            "op": "preset.search",
                            "presets": hits.iter().map(|p| json!({
                                "id": p.id, "engine": p.engine, "role": p.role, "tags": p.tags,
                                "timbre": p.timbre, "articulation": p.articulation,
                            })).collect::<Vec<_>>(),
                        }),
                    ));
                }
                Op::LayerUpsert(patch) => {
                    check_layer_patch(patch)?;
                    if snap.track(&patch.track_id).is_none() {
                        return Err(AgbError::new(
                            ErrorCode::UnknownTrack,
                            format!("track {:?} does not exist at tick {eff}", patch.track_id),
                        ));
                    }
                    if let (Some(st), Some(nt)) = (&patch.steps, &patch.notes) {
                        if !nt.is_empty() {
                            let note_steps: std::collections::BTreeSet<u32> = nt.iter().map(|n| n.step).collect();
                            let trig_steps: std::collections::BTreeSet<u32> = st
                                .iter()
                                .enumerate()
                                .filter(|(_, v)| **v == 1)
                                .map(|(k, _)| k as u32)
                                .collect();
                            if note_steps != trig_steps {
                                warnings.push(format!(
                                    "layer {}/{}: steps and notes disagree; notes take precedence",
                                    patch.track_id, patch.layer_id
                                ));
                            }
                        }
                    }
                    let c = Change::UpsertLayer { patch: patch.clone() };
                    snap.apply(&c);
                    changes.push(c);
                }
                Op::SceneDefine(d) => {
                    check_id("sceneId", &d.scene_id)?;
                    let idx = match p.scenes.iter().position(|s| s.id == d.scene_id) {
                        Some(i) => i,
                        None => {
                            p.scenes.push(Scene {
                                id: d.scene_id.clone(),
                                launch_at_tick: None,
                                phase_policy: Default::default(),
                                track_patches: Default::default(),
                            });
                            p.scenes.len() - 1
                        }
                    };
                    let s = &mut p.scenes[idx];
                    if d.launch_at_tick.is_some() {
                        s.launch_at_tick = d.launch_at_tick;
                    }
                    if let Some(pp) = d.phase_policy {
                        s.phase_policy = pp;
                    }
                    if let Some(tp) = &d.track_patches {
                        s.track_patches = tp.clone();
                    }
                }
                Op::SceneLaunch(l) => {
                    let Some(s) = p.scenes.iter_mut().find(|s| s.id == l.scene_id) else {
                        return Err(sv(format!("unknown scene {:?}; define it first with scene.define", l.scene_id)));
                    };
                    s.launch_at_tick = Some(l.at_tick.unwrap_or(eff));
                }
                Op::RenderOffline(r) => {
                    check_out_dir(&r.out_dir)?;
                    if let (Some(f), Some(to)) = (r.from_tick, r.to_tick) {
                        if to <= f {
                            return Err(sv(format!("render.offline: toTick {to} must be > fromTick {f}")));
                        }
                    }
                    renders.push((i, r.clone()));
                }
                Op::SystemPanic(_) => {
                    p.panic = PanicState { active: true, since_revision: Some(new_rev) };
                }
                Op::SystemPanicClear(_) => {
                    p.panic = PanicState::default();
                }
            }
            Ok(())
        })();
        r.map_err(|e| e.at_op(i))?;
    }

    if !changes.is_empty() {
        p.timeline.push(TimelineEntry {
            revision: new_rev,
            command_id: t.command_id.clone(),
            effective_tick: eff,
            changes,
        });
    }
    state::validate_globals(&p)?;
    state::validate_timeline(&p, eff)?;
    state::validate_scenes(&p)?;
    state::refresh_head(&mut p);
    p.revision = new_rev;
    Ok(Plan { project: p, effective_tick: eff, warnings, renders, results })
}

/// `outDir` must stay inside the project root.
pub fn check_out_dir(d: &str) -> Result<(), AgbError> {
    let path = std::path::Path::new(d);
    let escapes = path.is_absolute()
        || path.components().any(|c| matches!(c, std::path::Component::ParentDir));
    if d.is_empty() || escapes {
        return Err(sv(format!("outDir {d:?} must be a relative path inside the project")));
    }
    Ok(())
}

fn err_resp(cmd: Option<String>, e: AgbError) -> ErrorResponse {
    ErrorResponse::new(cmd, e)
}

/// Full transaction against an on-disk project.
pub fn apply(
    store: &ProjectStore,
    raw: &str,
    opts: ApplyOptions,
    hook: &mut dyn RenderHook,
) -> Result<TxnResponse, ErrorResponse> {
    let (value, txn) = parse_transaction(raw).map_err(|e| {
        let cmd = serde_json::from_str::<serde_json::Value>(raw)
            .ok()
            .and_then(|v| v.get("commandId").and_then(|c| c.as_str()).map(str::to_string));
        err_resp(cmd, e)
    })?;
    let cmd = Some(txn.command_id.clone());
    validate_envelope(&txn).map_err(|e| err_resp(cmd.clone(), e))?;

    let _lock = store.lock(Duration::from_secs(10)).map_err(|e| err_resp(cmd.clone(), e))?;
    let mut init_warning = None;
    if !store.exists() {
        store.init(None).map_err(|e| err_resp(cmd.clone(), e))?;
        init_warning = Some(format!("initialized new project at {}", store.root.display()));
    }
    let project = store.load().map_err(|e| err_resp(cmd.clone(), e))?;

    // Canonical payload: serde_json maps are sorted, output compact.
    let payload_hash = sha256_hex(&serde_json::to_vec(&value).unwrap());
    if let Some(rec) = store.idempotency_get(&txn.command_id).map_err(|e| err_resp(cmd.clone(), e))? {
        if rec.payload_sha256 == payload_hash {
            let mut r = rec.response;
            r.idempotent_replay = true;
            return Ok(r);
        }
        return Err(err_resp(
            cmd,
            AgbError::new(
                ErrorCode::IdempotencyPayloadMismatch,
                format!("commandId {:?} was already used with a different payload", txn.command_id),
            ),
        ));
    }

    let plan = plan(&project, &txn).map_err(|e| err_resp(cmd.clone(), e))?;
    let mut warnings = plan.warnings.clone();
    if let Some(w) = init_warning {
        warnings.insert(0, w);
    }
    let dry = opts.dry_run || txn.dry_run;
    if dry {
        if !plan.renders.is_empty() {
            warnings.push("dry run: render.offline skipped".into());
        }
        return Ok(TxnResponse {
            ok: true,
            command_id: txn.command_id,
            revision: project.revision,
            effective_tick: plan.effective_tick,
            idempotent_replay: false,
            warnings,
            dry_run: true,
            results: plan.results.iter().map(|(_, v)| v.clone()).collect(),
        });
    }

    // Renders read the post-commit state; on failure nothing is committed.
    let mut indexed = plan.results.clone();
    for (i, r) in &plan.renders {
        let v = hook.render(&plan.project, r).map_err(|e| err_resp(cmd.clone(), e.at_op(*i)))?;
        indexed.push((*i, v));
    }
    indexed.sort_by_key(|(i, _)| *i);
    let results: Vec<serde_json::Value> = indexed.into_iter().map(|(_, v)| v).collect();

    let resp = TxnResponse {
        ok: true,
        command_id: txn.command_id.clone(),
        revision: plan.project.revision,
        effective_tick: plan.effective_tick,
        idempotent_replay: false,
        warnings,
        dry_run: false,
        results,
    };
    store.save(&plan.project).map_err(|e| err_resp(cmd.clone(), e))?;
    store
        .idempotency_put(&IdempotencyRecord {
            command_id: txn.command_id.clone(),
            payload_sha256: payload_hash,
            response: resp.clone(),
        })
        .map_err(|e| err_resp(cmd, e))?;
    Ok(resp)
}

/// Move the logical transport (used by late-policy decisions). Not a transaction:
/// does not bump `revision`.
pub fn set_transport(store: &ProjectStore, tick: u64) -> Result<Project, AgbError> {
    let _lock = store.lock(Duration::from_secs(10))?;
    let mut p = store.load()?;
    p.transport_tick = tick;
    store.save(&p)?;
    Ok(p)
}
