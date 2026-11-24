//! Engine tests for SPEC §12.3 (txn and clock parts).

use crate::store::ProjectStore;
use crate::txn::{apply, set_transport, ApplyOptions, NoRender};
use agb_protocol::ErrorCode;
use serde_json::json;

const EX1: &str = include_str!("../../../examples/polymeter_280.json");
const EX2: &str = include_str!("../../../examples/render_p0.json");
const EX3: &str = include_str!("../../../examples/preset_load.json");

fn store() -> (tempfile::TempDir, ProjectStore) {
    let d = tempfile::tempdir().unwrap();
    let s = ProjectStore::new(d.path().join("song"));
    s.init(None).unwrap();
    (d, s)
}

fn run(s: &ProjectStore, raw: &str) -> Result<agb_protocol::TxnResponse, agb_protocol::ErrorResponse> {
    apply(s, raw, ApplyOptions::default(), &mut NoRender)
}

fn disk(s: &ProjectStore) -> String {
    std::fs::read_to_string(s.project_path()).unwrap()
}

#[test]
fn example_1_applies() {
    let (_d, s) = store();
    let r = run(&s, EX1).unwrap();
    assert_eq!(r.revision, 1);
    assert_eq!(r.effective_tick, 0);
    assert!(!r.idempotent_replay);
    let p = s.load().unwrap();
    assert_eq!(p.tracks.len(), 3);
    assert_eq!(p.scenes[0].launch_at_tick, Some(134_400));
    assert_eq!(crate::schedule::loop_lcm_steps(&p), 280);
}

/// T_txn_idempotent
#[test]
fn t_txn_idempotent() {
    let (_d, s) = store();
    let a = run(&s, EX1).unwrap();
    let before = disk(&s);
    let b = run(&s, EX1).unwrap();
    assert!(b.idempotent_replay);
    assert_eq!(b.revision, a.revision);
    assert_eq!(s.load().unwrap().revision, 1, "revision only +1 once");
    assert_eq!(disk(&s), before, "replay must not write");

    // same id, different payload
    let mut v: serde_json::Value = serde_json::from_str(EX1).unwrap();
    v["actor"] = json!("agent.other");
    let e = run(&s, &v.to_string()).unwrap_err();
    assert_eq!(e.error.code, ErrorCode::IdempotencyPayloadMismatch);
}

/// T_txn_conflict
#[test]
fn t_txn_conflict() {
    let (_d, s) = store();
    let mut v: serde_json::Value = serde_json::from_str(EX1).unwrap();
    v["ifMatchRevision"] = json!(5);
    let before = disk(&s);
    let e = run(&s, &v.to_string()).unwrap_err();
    assert_eq!(e.error.code, ErrorCode::RevisionConflict);
    assert_eq!(e.server_revision, Some(0));
    assert_eq!(disk(&s), before, "project on disk unchanged");
    // A failed command id is not burned: the corrected txn still applies.
    v["ifMatchRevision"] = json!(0);
    assert_eq!(run(&s, &v.to_string()).unwrap().revision, 1);
}

/// T_txn_atomic
#[test]
fn t_txn_atomic() {
    let (_d, s) = store();
    let before = disk(&s);
    let txn = json!({
        "commandId": "atomic_1", "actor": "agent.t", "quantize": {"type":"tick","tick":0},
        "ops": [
            {"op":"track.upsert","trackId":"ok_track"},
            {"op":"layer.upsert","trackId":"ok_track","layerId":"l","lengthSteps":4,"steps":[1,0,1,0]},
            {"op":"layer.upsert","trackId":"missing","layerId":"l"}
        ]
    });
    let e = run(&s, &txn.to_string()).unwrap_err();
    assert_eq!(e.error.code, ErrorCode::UnknownTrack);
    assert_eq!(e.error.op_index, Some(2));
    assert_eq!(disk(&s), before);

    // Cross-field invalid state (steps length != lengthSteps) is also atomic.
    let txn = json!({
        "commandId": "atomic_2", "actor": "agent.t", "quantize": {"type":"tick","tick":0},
        "ops": [
            {"op":"track.upsert","trackId":"a"},
            {"op":"layer.upsert","trackId":"a","layerId":"l","lengthSteps":3,"steps":[1,0]}
        ]
    });
    assert_eq!(run(&s, &txn.to_string()).unwrap_err().error.code, ErrorCode::SchemaViolation);
    assert_eq!(disk(&s), before);
}

/// T_late_reject (+ other late policies)
#[test]
fn t_late_reject() {
    let (_d, s) = store();
    set_transport(&s, 1000).unwrap();
    let before = disk(&s);
    let e = run(&s, EX1).unwrap_err();
    assert_eq!(e.error.code, ErrorCode::LateRejected);
    assert_eq!(disk(&s), before);

    let mut v: serde_json::Value = serde_json::from_str(EX1).unwrap();
    v["commandId"] = json!("late_shift");
    v["latePolicy"] = json!("shift_to_next_legal");
    let r = run(&s, &v.to_string()).unwrap();
    assert_eq!(r.effective_tick, 3840, "next bar after 1000");
    assert!(r.warnings.iter().any(|w| w.starts_with("LATE_SHIFTED")));

    let mut v: serde_json::Value = serde_json::from_str(EX1).unwrap();
    v["commandId"] = json!("late_asap");
    v["ifMatchRevision"] = json!(1);
    v["latePolicy"] = json!("apply_asap_with_flag");
    let r = run(&s, &v.to_string()).unwrap();
    assert_eq!(r.effective_tick, 1000);
    assert!(r.warnings.iter().any(|w| w.starts_with("LATE_APPLIED_ASAP")));
}

/// Example 3 (P1): preset.load + automation ramp at the meeting point.
#[test]
fn example_3_applies_in_p1() {
    let (_d, s) = store();
    run(&s, EX1).unwrap();
    run(&s, EX2).unwrap(); // NoRender hook, still a committed txn → revision 2
    let r = run(&s, EX3).unwrap();
    assert_eq!((r.revision, r.effective_tick), (3, 134_400));
    let p = s.load().unwrap();
    let bass = p.tracks.iter().find(|t| t.id == "bass").unwrap();
    assert_eq!(bass.preset_id.as_deref(), Some("preset:synth.waveform:bass:simple_saw@v1.0"));
    // Before the meeting point the bass has no preset; after it does.
    let before = agb_score::timeline::snapshot_at(&p.timeline, 134_399);
    assert_eq!(before.track("bass").unwrap().preset_id, None);
    let sch = crate::schedule::schedule(&p, 0, 2 * 134_400).unwrap();
    assert_eq!(sch.automation.len(), 1);
    assert_eq!(sch.automation[0].lane.value_at(136_320.0), Some(0.35));
}

#[test]
fn p1_ops_validate_atomically() {
    let (_d, s) = store();
    run(&s, EX1).unwrap();
    let before = disk(&s);
    for (ops, code) in [
        (json!([{"op":"preset.load","trackId":"nope","presetId":"preset:synth.waveform:bass:simple_saw@v1.0"}]), ErrorCode::UnknownTrack),
        (json!([{"op":"preset.load","trackId":"bass","presetId":"preset:x:y:z@v1.0"}]), ErrorCode::UnknownPreset),
        (json!([{"op":"voice.setParams","trackId":"bass","params":{"filter.cutoffHz":500.0}}]), ErrorCode::SchemaViolation),
        (json!([{"op":"automation.schedule","trackId":"bass","param":"osc.wave","mode":"ramp","fromTick":0,"toTick":10,"from":0.0,"to":1.0}]), ErrorCode::SchemaViolation),
        (json!([{"op":"automation.schedule","trackId":"bass","param":"osc.pulseWidth","mode":"ramp","fromTick":0,"toTick":10,"from":0.0,"to":5.0}]), ErrorCode::SchemaViolation),
        (json!([{"op":"clip.schedule","trackId":"bass","notes":[{"tick":0,"pitch":"Z9","durTicks":10}]}]), ErrorCode::SchemaViolation),
    ] {
        let t = json!({"commandId": format!("bad-{ops}"), "actor":"agent.t","quantize":{"type":"tick","tick":0},"ops":ops});
        assert_eq!(run(&s, &t.to_string()).unwrap_err().error.code, code, "{ops}");
        assert_eq!(disk(&s), before);
    }
    // preset.load switches engine and clears stale params; then setParams works.
    let t = json!({"commandId":"ok","actor":"agent.t","quantize":{"type":"tick","tick":0},"ops":[
        {"op":"voice.setParams","trackId":"bass","params":{"osc.pulseWidth":0.3}},
        {"op":"preset.load","trackId":"bass","presetId":"preset:synth.moogish:bass:acid_ladder@v1.0","tailPolicy":"cut_old"},
        {"op":"voice.setParams","trackId":"bass","params":{"filter.cutoffHz":500.0}},
        {"op":"clip.schedule","trackId":"keys","notes":[{"tick":960,"pitch":"G3","durTicks":480,"velocity":0.5}]},
        {"op":"preset.search","tags":["acid"]}
    ]});
    let r = run(&s, &t.to_string()).unwrap();
    assert_eq!(r.results[0]["presets"][0]["id"], "preset:synth.moogish:bass:acid_ladder@v1.0");
    let p = s.load().unwrap();
    let bass = p.tracks.iter().find(|t| t.id == "bass").unwrap();
    assert_eq!(bass.engine_id, "synth.moogish");
    assert_eq!(bass.params.len(), 1);
    let sch = crate::schedule::schedule(&p, 0, 3840).unwrap();
    assert!(sch.events.iter().any(|e| e.layer_id == "clip" && e.tick == 960 && e.pitch == 55));
    assert_eq!(sch.cuts.len(), 1);
}

#[test]
fn dry_run_does_not_write() {
    let (_d, s) = store();
    let before = disk(&s);
    let r = apply(&s, EX1, ApplyOptions { dry_run: true }, &mut NoRender).unwrap();
    assert!(r.dry_run);
    assert_eq!(r.revision, 0);
    assert_eq!(disk(&s), before);
    // Not recorded for idempotency: the real apply still works.
    assert!(!run(&s, EX1).unwrap().idempotent_replay);
}

#[test]
fn panic_blocks_writes_until_clear() {
    let (_d, s) = store();
    run(&s, EX1).unwrap();
    let panic = json!({"commandId":"p1","actor":"human.op","quantize":{"type":"immediate"},"ops":[{"op":"system.panic"}]});
    run(&s, &panic.to_string()).unwrap();
    assert!(s.load().unwrap().panic.active);
    let w = json!({"commandId":"w1","actor":"agent.t","quantize":{"type":"tick","tick":0},
        "ops":[{"op":"project.setTempo","tempoBpm":100.0}]});
    assert_eq!(run(&s, &w.to_string()).unwrap_err().error.code, ErrorCode::PanicActive);
    let clear = json!({"commandId":"c1","actor":"human.op","quantize":{"type":"immediate"},"ops":[{"op":"system.panic.clear"}]});
    run(&s, &clear.to_string()).unwrap();
    assert_eq!(run(&s, &w.to_string()).unwrap().revision, 4);
}

#[test]
fn immediate_write_rejected_and_budgets() {
    let (_d, s) = store();
    let w = json!({"commandId":"i1","actor":"agent.t","quantize":{"type":"immediate"},
        "ops":[{"op":"track.upsert","trackId":"a"}]});
    assert_eq!(run(&s, &w.to_string()).unwrap_err().error.code, ErrorCode::SchemaViolation);

    let ops: Vec<_> = (0..9).map(|i| json!({"op":"track.upsert","trackId":format!("t{i}")})).collect();
    let w = json!({"commandId":"b1","actor":"agent.t","quantize":{"type":"tick","tick":0},"ops":ops});
    assert_eq!(run(&s, &w.to_string()).unwrap_err().error.code, ErrorCode::BudgetExceeded);

    let w = json!({"commandId":"e1","actor":"agent.t","quantize":{"type":"tick","tick":0},
        "ops":[{"op":"track.upsert","trackId":"a","engineId":"synth.nope"}]});
    assert_eq!(run(&s, &w.to_string()).unwrap_err().error.code, ErrorCode::UnknownEngine);
    let w = json!({"commandId":"e2","actor":"agent.t","quantize":{"type":"tick","tick":0},
        "ops":[{"op":"track.upsert","trackId":"a","engineId":"synth.granular"}]});
    assert_eq!(run(&s, &w.to_string()).unwrap_err().error.code, ErrorCode::UnsupportedInPhase);
    let w = json!({"commandId":"e3","actor":"agent.t","quantize":{"type":"tick","tick":0},
        "ops":[{"op":"track.upsert","trackId":"a","presetId":"preset:synth.waveform:bass:nope@v1.0"}]});
    assert_eq!(run(&s, &w.to_string()).unwrap_err().error.code, ErrorCode::UnknownPreset);
    let w = json!({"commandId":"bad actor","actor":"robot","quantize":{"type":"tick","tick":0},
        "ops":[{"op":"track.upsert","trackId":"a"}]});
    assert_eq!(run(&s, &w.to_string()).unwrap_err().error.code, ErrorCode::SchemaViolation);
    assert_eq!(run(&s, "{not json").unwrap_err().error.code, ErrorCode::InvalidJson);
}

/// Polymeter: LCM meeting point is visible in the schedule.
#[test]
fn schedule_polymeter_meets_at_280() {
    let (_d, s) = store();
    run(&s, EX1).unwrap();
    let p = s.load().unwrap();
    let sch = crate::schedule::schedule(&p, 0, 134_400).unwrap();
    assert_eq!(sch.common_origins, vec![0, 280]);
    assert_eq!(sch.layer_cycle_starts["drums/kick78"].len(), 41); // 40 cycles + step 280
    assert_eq!(sch.layer_cycle_starts["bass/riff58"].len(), 57);
    assert_eq!(sch.layer_cycle_starts["keys/pad44"].len(), 36);
    // event counts: kick 3/7 → 120, bass 2/5 → 112, pad 1/8 → 35
    let count = |t: &str| sch.events.iter().filter(|e| e.track_id == t).count();
    assert_eq!((count("drums"), count("bass"), count("keys")), (120, 112, 35));
    assert!(sch.events.iter().all(|e| e.tick < 134_400));
    assert_eq!(sch.scene_launches[0].step, 280);
}

/// Hard reset scene re-anchors phase; free_phase does not.
#[test]
fn scene_phase_policies() {
    let (_d, s) = store();
    let t = json!({"commandId":"s1","actor":"agent.t","quantize":{"type":"tick","tick":0},"ops":[
        {"op":"track.upsert","trackId":"a"},
        {"op":"layer.upsert","trackId":"a","layerId":"l3","lengthSteps":3,"steps":[1,0,0]},
        {"op":"scene.define","sceneId":"hr","launchAtTick": 4 * 480}
    ]});
    run(&s, &t.to_string()).unwrap();
    let p = s.load().unwrap();
    let sch = crate::schedule::schedule(&p, 0, 8 * 480).unwrap();
    // cycle starts: 0, 3, then reset at 4 → 4, 7
    assert_eq!(sch.layer_cycle_starts["a/l3"], vec![0, 3, 4, 7]);

    let t = json!({"commandId":"s2","actor":"agent.t","quantize":{"type":"tick","tick":0},"ops":[
        {"op":"scene.define","sceneId":"hr","phasePolicy":"free_phase"}
    ]});
    run(&s, &t.to_string()).unwrap();
    let p = s.load().unwrap();
    let sch = crate::schedule::schedule(&p, 0, 8 * 480).unwrap();
    assert_eq!(sch.layer_cycle_starts["a/l3"], vec![0, 3, 6]);

    let bad = json!({"commandId":"s3","actor":"agent.t","quantize":{"type":"tick","tick":0},"ops":[
        {"op":"scene.define","sceneId":"x","launchAtTick": 100}
    ]});
    assert_eq!(run(&s, &bad.to_string()).unwrap_err().error.code, ErrorCode::SchemaViolation);
}

/// Changes take effect at their quantized tick.
#[test]
fn quantized_change_takes_effect_at_tick() {
    let (_d, s) = store();
    run(&s, EX1).unwrap();
    let t = json!({"commandId":"q1","actor":"agent.t","ifMatchRevision":1,
        "quantize":{"type":"tick","tick":134400},"ops":[
        {"op":"layer.upsert","trackId":"keys","layerId":"pad44","mute":true}
    ]});
    run(&s, &t.to_string()).unwrap();
    let p = s.load().unwrap();
    let sch = crate::schedule::schedule(&p, 0, 2 * 134_400).unwrap();
    let keys: Vec<_> = sch.events.iter().filter(|e| e.track_id == "keys").collect();
    assert_eq!(keys.len(), 35, "pad plays only before the meeting point");
    assert!(keys.iter().all(|e| e.tick < 134_400));
}

#[test]
fn preset_implies_engine() {
    let (_d, s) = store();
    let t = json!({"commandId":"pe","actor":"agent.t","quantize":{"type":"tick","tick":0},"ops":[
        {"op":"track.upsert","trackId":"pad","presetId":"preset:synth.subtractive:pad:noise_air@v1.0"}
    ]});
    run(&s, &t.to_string()).unwrap();
    assert_eq!(s.load().unwrap().tracks[0].engine_id, "synth.subtractive");
}
