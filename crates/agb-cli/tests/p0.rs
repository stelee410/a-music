//! P0 acceptance tests (SPEC §12.2 DoD / §12.3) at the CLI-library level,
//! plus a smoke test of the real `agb` binary.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(repo().join(rel)).unwrap()
}

fn sha(path: &Path) -> String {
    use sha2::Digest;
    format!("sha256:{}", hex::encode(sha2::Sha256::digest(std::fs::read(path).unwrap())))
}

/// Golden hash of examples 1 → 2.
#[test]
fn golden() {
    let manifest_path = repo().join("tests/golden/manifest.json");
    let mut manifest: Value = serde_json::from_str(&std::fs::read_to_string(&manifest_path).unwrap()).unwrap();
    let update = std::env::var("AGB_UPDATE_GOLDEN").is_ok();
    for case in manifest["cases"].as_array_mut().unwrap() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("song");
        for t in case["transactions"].as_array().unwrap() {
            agb_cli::txn_apply(&root, &read(t.as_str().unwrap()), false).unwrap();
        }
        let got = sha(&root.join(case["file"].as_str().unwrap()));
        if update {
            case["sha256"] = json!(got);
        } else {
            assert_eq!(got, case["sha256"].as_str().unwrap(), "golden mismatch for {}", case["id"]);
        }
    }
    if update {
        std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).unwrap() + "\n").unwrap();
    }
}

/// DoD 5 via CLI render: two renders, same seed → same mix SHA-256.
#[test]
fn render_twice_same_hash() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("song");
    agb_cli::txn_apply(&root, &read("examples/polymeter_280.json"), false).unwrap();
    let args = |dir: &str| agb_cli::RenderArgs {
        from_tick: Some(0),
        to_tick: Some(134_400),
        seed: Some(7),
        stems: true,
        tail_ms: Some(500),
        format: agb_protocol::SampleFormat::Pcm16,
        out_dir: Some(root.join(dir)),
        out: None,
    };
    let a = agb_cli::render_offline(&root, &args("renders/a")).unwrap();
    let b = agb_cli::render_offline(&root, &args("renders/b")).unwrap();
    assert_eq!(a["mixSha256"], b["mixSha256"]);
    assert_eq!(sha(&root.join("renders/a/mix.wav")), sha(&root.join("renders/b/mix.wav")));
    for f in ["events.json", "metrics.json", "render.json", "stems/drums.wav", "stems/bass.wav", "stems/keys.wav"] {
        assert!(root.join("renders/a").join(f).is_file(), "{f} missing");
    }
}

/// DoD 6: events.json shows global step 280 as the common origin of all three layers.
#[test]
fn events_json_lcm() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("song");
    agb_cli::txn_apply(&root, &read("examples/polymeter_280.json"), false).unwrap();
    agb_cli::txn_apply(&root, &read("examples/render_p0.json"), false).unwrap();
    let ev: Value = serde_json::from_str(&std::fs::read_to_string(root.join("renders/meet_280/events.json")).unwrap()).unwrap();
    assert_eq!(ev["commonOrigins"], json!([0, 280]));
    assert_eq!(ev["loopLcmSteps"], json!(280));
    assert_eq!(ev["toStep"], json!(280));
    for key in ["drums/kick78", "bass/riff58", "keys/pad44"] {
        let starts = ev["layerCycleStarts"][key].as_array().unwrap();
        assert_eq!(starts.last().unwrap(), &json!(280), "{key}");
    }
    assert_eq!(ev["sceneLaunches"][0]["step"], json!(280));
    // No event at or after the window end.
    assert!(ev["events"].as_array().unwrap().iter().all(|e| e["tick"].as_u64().unwrap() < 134_400));
}

/// DoD 7 via transactions: panic → render is near-silent.
#[test]
fn panic_then_render_is_silent() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("song");
    agb_cli::txn_apply(&root, &read("examples/polymeter_280.json"), false).unwrap();
    let panic = json!({"commandId":"panic_1","actor":"human.stage","quantize":{"type":"immediate"},"ops":[{"op":"system.panic"}]});
    agb_cli::txn_apply(&root, &panic.to_string(), false).unwrap();
    let render = json!({"commandId":"render_after_panic","actor":"agent.t","quantize":{"type":"immediate"},
        "ops":[{"op":"render.offline","fromTick":0,"toTick":134400,"tailMs":500,"seed":7,"outDir":"renders/panic"}]});
    let r = agb_cli::txn_apply(&root, &render.to_string(), false).unwrap();
    assert_eq!(r["results"][0]["nearSilent"], json!(true));
    let m: Value = serde_json::from_str(&std::fs::read_to_string(root.join("renders/panic/metrics.json")).unwrap()).unwrap();
    assert!(m["mix"]["peakDbfs"].is_null() || m["mix"]["peakDbfs"].as_f64().unwrap() < -60.0);
}

fn validator(name: &str) -> jsonschema::Validator {
    let schema: Value = serde_json::from_str(&read(&format!("schemas/{name}"))).unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

/// T_schema: examples validate against the committed transaction schema.
#[test]
fn t_schema() {
    let v = validator("transaction.schema.json");
    for ex in ["examples/polymeter_280.json", "examples/render_p0.json", "examples/preset_load.json"] {
        let inst: Value = serde_json::from_str(&read(ex)).unwrap();
        let errs: Vec<String> = v.iter_errors(&inst).map(|e| e.to_string()).collect();
        assert!(errs.is_empty(), "{ex}: {errs:#?}");
    }
    let bad = json!({"commandId":"x","actor":"agent.a","quantize":{"type":"tick","tick":0},
        "ops":[{"op":"track.upsert","trackId":"t","nope":1}]});
    assert!(!v.is_valid(&bad), "unknown fields must be rejected by the schema");

    let pv = validator("preset.schema.json");
    for p in agb_presets::catalog() {
        assert!(pv.is_valid(&serde_json::to_value(p).unwrap()), "{}", p.id);
    }

    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("song");
    agb_cli::txn_apply(&root, &read("examples/polymeter_280.json"), false).unwrap();
    let proj: Value = serde_json::from_str(&std::fs::read_to_string(root.join("project.json")).unwrap()).unwrap();
    let errs: Vec<String> = validator("project.schema.json").iter_errors(&proj).map(|e| e.to_string()).collect();
    assert!(errs.is_empty(), "{errs:#?}");
}

/// schemas/ must match what the code exports (`agb schema export`).
#[test]
fn schemas_in_sync() {
    for (name, s) in agb_cli::schemas() {
        let on_disk = read(&format!("schemas/{name}"));
        assert_eq!(
            on_disk.as_bytes(),
            agb_cli::schema_bytes(&s).as_slice(),
            "schemas/{name} is stale; run `cargo run -p agb-cli -- schema export`"
        );
    }
}

/// Smoke test of the real binary: exit codes and JSON on stdout.
#[test]
fn binary_smoke() {
    let bin = env!("CARGO_BIN_EXE_agb");
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("song");
    let run = |args: &[&str]| {
        let o = std::process::Command::new(bin).args(args).output().unwrap();
        let v: Value = serde_json::from_slice(&o.stdout).unwrap_or(Value::Null);
        (o.status.code().unwrap(), v)
    };
    let r = root.to_str().unwrap();
    let ex1 = repo().join("examples/polymeter_280.json");
    let ex3 = repo().join("examples/preset_load.json");
    let (code, v) = run(&["txn", "apply", "--project", r, "--file", ex1.to_str().unwrap(), "--dry-run"]);
    assert_eq!((code, v["dryRun"].clone()), (0, json!(true)));
    let (code, v) = run(&["txn", "apply", "--project", r, "--file", ex1.to_str().unwrap()]);
    assert_eq!((code, v["revision"].clone()), (0, json!(1)));
    // Example 3 expects revision 2; after only example 1 the project is at 1.
    let (code, v) = run(&["txn", "apply", "--project", r, "--file", ex3.to_str().unwrap()]);
    assert_eq!((code, v["error"]["code"].clone()), (1, json!("REVISION_CONFLICT")));
    assert_eq!(v["serverRevision"], json!(1));
    let (code, v) = run(&["project", "snapshot", "--project", r]);
    assert_eq!((code, v["derived"]["loopLcmTicks"].clone()), (0, json!(134_400)));
    let (code, v) = run(&["render", "offline", "--project", r, "--to-tick", "3840", "--tail-ms", "0"]);
    assert_eq!(code, 0, "{v}");
    assert!(root.join("renders/latest/mix.wav").is_file());
    let (code, v) = run(&["preset", "search", "--tags", "bass"]);
    assert!(code == 0 && v["count"].as_u64().unwrap() >= 2);
}
