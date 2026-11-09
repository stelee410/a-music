//! `agb` command implementations. Every command returns JSON; errors are
//! [`ErrorResponse`]s with stable codes (SPEC §8.4).

use agb_assets::{AssetKind, AssetRef, LicenseRecord};
use agb_engine::clock::ticks_per_step;
use agb_engine::schedule::{default_to_tick, loop_lcm_steps};
use agb_engine::{apply, set_transport, ApplyOptions, ProjectStore};
use agb_protocol::{AgbError, ErrorCode, ErrorResponse, SampleFormat};
use agb_render::{render_to_dir, DiskRenderer, RenderRequest, DEFAULT_TAIL_MS};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub type CmdResult = Result<Value, ErrorResponse>;

fn fail(e: AgbError) -> ErrorResponse {
    ErrorResponse::new(None, e)
}

pub fn project_init(root: &Path, name: Option<&str>) -> CmdResult {
    let store = ProjectStore::new(root);
    let existed = store.exists();
    let p = store.init(name).map_err(fail)?;
    Ok(json!({ "ok": true, "project": root, "created": !existed, "revision": p.revision }))
}

pub fn project_snapshot(root: &Path, at_tick: Option<u64>) -> CmdResult {
    let store = ProjectStore::new(root);
    let p = store.load().map_err(fail)?;
    let tps = ticks_per_step(p.ppqn, p.steps_per_quarter).unwrap_or(0);
    let lcm = loop_lcm_steps(&p);
    let mut out = json!({
        "ok": true,
        "project": p,
        "derived": {
            "ticksPerStep": tps,
            "loopLcmSteps": lcm,
            "loopLcmTicks": lcm * tps,
            "defaultRenderToTick": default_to_tick(&p),
        }
    });
    if let Some(t) = at_tick {
        out["stateAtTick"] = json!({
            "tick": t,
            "tracks": agb_score::timeline::snapshot_at(&p.timeline, t).tracks,
        });
    }
    Ok(out)
}

pub fn txn_apply(root: &Path, raw: &str, dry_run: bool) -> CmdResult {
    let store = ProjectStore::new(root);
    let mut hook = DiskRenderer { project_root: root.to_path_buf() };
    let r = apply(&store, raw, ApplyOptions { dry_run }, &mut hook)?;
    Ok(serde_json::to_value(r).unwrap())
}

pub struct RenderArgs {
    pub from_tick: Option<u64>,
    pub to_tick: Option<u64>,
    pub seed: Option<u64>,
    pub stems: bool,
    pub tail_ms: Option<u32>,
    pub format: SampleFormat,
    pub out_dir: Option<PathBuf>,
    pub out: Option<PathBuf>,
}

pub fn render_offline(root: &Path, a: &RenderArgs) -> CmdResult {
    let store = ProjectStore::new(root);
    let p = store.load().map_err(fail)?;
    let req = RenderRequest {
        from_tick: a.from_tick.unwrap_or(p.origin_tick),
        to_tick: a.to_tick.unwrap_or_else(|| default_to_tick(&p)),
        tail_ms: a.tail_ms.unwrap_or(DEFAULT_TAIL_MS),
        seed: a.seed.unwrap_or(p.seed),
        stems: a.stems,
        format: a.format,
    };
    let (dir, mix_name) = match (&a.out, &a.out_dir) {
        (Some(out), _) => {
            let dir = out.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
            let name = out
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "mix.wav".into());
            (dir, name)
        }
        (None, Some(d)) => (d.clone(), "mix.wav".to_string()),
        (None, None) => (root.join("renders").join("latest"), "mix.wav".to_string()),
    };
    let mut v = render_to_dir(&p, &req, &dir, &mix_name).map_err(fail)?;
    v["ok"] = json!(true);
    v["projectRevision"] = json!(p.revision);
    Ok(v)
}

pub fn transport_set(root: &Path, tick: u64) -> CmdResult {
    let p = set_transport(&ProjectStore::new(root), tick).map_err(fail)?;
    Ok(json!({ "ok": true, "transportTick": p.transport_tick, "revision": p.revision }))
}

pub fn transport_show(root: &Path) -> CmdResult {
    let p = ProjectStore::new(root).load().map_err(fail)?;
    Ok(json!({ "ok": true, "transportTick": p.transport_tick, "revision": p.revision, "panic": p.panic }))
}

pub fn preset_search(tags: &[String], engine: Option<&str>, role: Option<&str>) -> CmdResult {
    let hits = agb_presets::search(tags, engine, role);
    Ok(json!({
        "ok": true,
        "count": hits.len(),
        "presets": hits.iter().map(|p| json!({
            "id": p.id, "engine": p.engine, "role": p.role, "tags": p.tags,
            "timbre": p.timbre, "articulation": p.articulation, "cpuClass": p.cpu_class,
        })).collect::<Vec<_>>(),
    }))
}

pub fn preset_show(id: &str) -> CmdResult {
    let p = agb_presets::get(id)
        .ok_or_else(|| fail(AgbError::new(ErrorCode::UnknownPreset, format!("unknown preset {id:?}"))))?;
    Ok(json!({ "ok": true, "preset": p }))
}

pub fn engine_list() -> CmdResult {
    let engines: Vec<Value> = agb_dsp::known_engines()
        .into_iter()
        .map(|(id, st)| {
            let status = match st {
                agb_dsp::EngineStatus::Available => "available".to_string(),
                agb_dsp::EngineStatus::Planned(ph) => format!("planned:{ph}"),
                agb_dsp::EngineStatus::Unknown => "unknown".to_string(),
            };
            json!({ "engineId": id, "status": status })
        })
        .collect();
    Ok(json!({ "ok": true, "engines": engines }))
}

pub fn engine_params(id: &str) -> CmdResult {
    agb_engine::state::check_engine(id).map_err(fail)?;
    Ok(json!({ "ok": true, "engineId": id, "params": agb_dsp::waveform::describe_params() }))
}

/// All exported schemas: (file name, schema).
pub fn schemas() -> Vec<(&'static str, Value)> {
    vec![
        ("transaction.schema.json", agb_protocol::transaction_schema()),
        ("project.schema.json", agb_score::project_schema()),
        ("preset.schema.json", agb_presets::preset_schema()),
    ]
}

pub fn schema_bytes(v: &Value) -> Vec<u8> {
    let mut b = serde_json::to_vec_pretty(v).unwrap();
    b.push(b'\n');
    b
}

pub fn schema_export(out_dir: &Path) -> CmdResult {
    std::fs::create_dir_all(out_dir).map_err(|e| fail(AgbError::io(e)))?;
    let mut files = vec![];
    for (name, s) in schemas() {
        let p = out_dir.join(name);
        std::fs::write(&p, schema_bytes(&s)).map_err(|e| fail(AgbError::io(e)))?;
        files.push(p);
    }
    Ok(json!({ "ok": true, "files": files }))
}

pub struct AssetAddArgs {
    pub file: PathBuf,
    pub id: String,
    pub license: String,
    pub redistributable: bool,
    pub source: Option<String>,
    pub kind: AssetKind,
}

pub fn asset_add(root: &Path, a: &AssetAddArgs) -> CmdResult {
    let store = ProjectStore::new(root);
    let _lock = store.lock(std::time::Duration::from_secs(10)).map_err(fail)?;
    let mut p = store.load().map_err(fail)?;
    agb_engine::state::check_id("assetId", &a.id).map_err(fail)?;
    if p.assets.iter().any(|x| x.id == a.id) {
        return Err(fail(AgbError::new(ErrorCode::SchemaViolation, format!("asset {:?} already exists", a.id))));
    }
    let name = a
        .file
        .file_name()
        .ok_or_else(|| fail(AgbError::new(ErrorCode::AssetMissing, "file has no name")))?;
    let rel = Path::new("assets").join(name);
    let dest = root.join(&rel);
    std::fs::create_dir_all(root.join("assets")).map_err(|e| fail(AgbError::io(e)))?;
    std::fs::copy(&a.file, &dest).map_err(|e| {
        fail(AgbError::new(ErrorCode::AssetMissing, format!("{}: {e}", a.file.display())))
    })?;
    let sha = agb_assets::sha256_file(&dest).map_err(|e| fail(AgbError::io(e)))?;
    let rel_s = rel.to_string_lossy().replace('\\', "/");
    p.assets.push(AssetRef { id: a.id.clone(), path: rel_s.clone(), kind: a.kind, sha256: sha.clone() });
    let rec = LicenseRecord {
        spdx: a.license.clone(),
        redistributable: a.redistributable,
        source: a.source.clone(),
        notes: None,
    };
    let mut warnings = vec![];
    if !agb_assets::factory_allowed(&rec) {
        warnings.push(format!(
            "license {} (redistributable={}) is fine for local use but not for factory content",
            rec.spdx, rec.redistributable
        ));
    }
    p.licenses_registry.insert(a.id.clone(), rec);
    p.revision += 1;
    store.save(&p).map_err(fail)?;
    Ok(json!({ "ok": true, "assetId": a.id, "path": rel_s, "sha256": sha, "revision": p.revision, "warnings": warnings }))
}

pub fn asset_verify(root: &Path, factory: bool) -> CmdResult {
    let p = ProjectStore::new(root).load().map_err(fail)?;
    let probs = agb_assets::verify(root, &p.assets, &p.licenses_registry, factory);
    if let Some(first) = probs.first() {
        let code = match first.code {
            "LICENSE_FORBIDDEN" | "LICENSE_MISSING" => ErrorCode::LicenseForbidden,
            _ => ErrorCode::AssetMissing,
        };
        let mut e = AgbError::new(code, format!("{} asset problem(s); first: {}: {}", probs.len(), first.asset_id, first.message));
        e.message.push_str(&format!(" | all: {}", serde_json::to_string(&probs).unwrap()));
        return Err(fail(e));
    }
    Ok(json!({ "ok": true, "assets": p.assets.len() }))
}

pub struct PerformArgs {
    pub project: PathBuf,
    pub brief: String,
    pub phrases: u32,
    pub bars: u32,
    pub realtime: bool,
    pub seed: u64,
    pub models: Vec<String>,
    pub base_url: Option<String>,
    pub api_key_file: Option<PathBuf>,
    pub max_retries: u32,
    pub continue_existing: bool,
    pub mock: bool,
}

/// `agb perform`: prints one JSON event per line (NDJSON); returns the exit code.
pub fn perform_cmd(a: PerformArgs) -> i32 {
    use std::io::Write;
    let mut emit = |v: Value| {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{}", serde_json::to_string(&v).unwrap());
        let _ = out.flush();
    };
    if !(1..=128).contains(&a.phrases) || !(2..=16).contains(&a.bars) {
        emit(json!({"type": "fatal", "message": "--phrases must be 1..=128 and --bars 2..=16"}));
        return 2;
    }
    let brief = if a.brief.trim().is_empty() { "an evolving instrumental groove".to_string() } else { a.brief };
    let mut cfg = agb_agent::PerformConfig::new(a.project, brief);
    cfg.phrases = a.phrases;
    cfg.default_bars = a.bars;
    cfg.seed = a.seed;
    cfg.max_retries = a.max_retries;
    cfg.continue_existing = a.continue_existing;
    cfg.realtime = a.realtime;
    let llm: Box<dyn agb_agent::Llm + Send> = if a.mock {
        Box::new(agb_agent::MockPerformer::default())
    } else {
        match agb_agent::llm::load_api_key(a.api_key_file.as_deref()) {
            Ok(k) => Box::new(agb_agent::HttpLlm::new(a.base_url, k, a.models)),
            Err(e) => {
                emit(json!({"type": "fatal", "message": e}));
                return 1;
            }
        }
    };
    // Listener commands arrive as NDJSON on stdin (chat / set_tempo / human_state / finish).
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if let Some(c) = agb_agent::parse_command(&line) {
                if tx.send(c).is_err() {
                    break;
                }
            }
        }
    });
    match agb_agent::perform(&cfg, llm, rx, &mut emit) {
        Ok(_) => 0,
        Err(e) => {
            emit(json!({"type": "fatal", "message": e}));
            1
        }
    }
}

/// `agb oneshot`: render a single note of any engine/preset to a WAV (pad samples).
pub fn oneshot(engine: &str, preset: Option<&str>, params: Option<&str>, pitch: &str, len_ms: u32, tail_ms: u32, out: &Path) -> CmdResult {
    let mut track = json!({"op": "track.upsert", "trackId": "one", "engineId": engine});
    if let Some(p) = preset {
        track["presetId"] = json!(p);
    }
    if let Some(ps) = params {
        let v: Value = serde_json::from_str(ps)
            .map_err(|e| fail(AgbError::new(ErrorCode::InvalidJson, format!("--params: {e}"))))?;
        track["params"] = v;
    }
    // 120 BPM, 960 ppqn → 1 ms = 1.92 ticks
    let dur_ticks = ((len_ms as f64) * 1.92).round().max(1.0) as u64;
    let txn = json!({
        "commandId": "oneshot", "actor": "human.cli", "quantize": {"type": "tick", "tick": 0},
        "ops": [track, {"op": "clip.schedule", "trackId": "one",
                        "notes": [{"tick": 0, "pitch": pitch, "durTicks": dur_ticks, "velocity": 1.0}]}]
    });
    let (_, t) = agb_protocol::parse_transaction(&txn.to_string()).map_err(fail)?;
    let base = agb_score::Project { master_gain_db: 0.0, ..Default::default() };
    let p = agb_engine::plan(&base, &t).map_err(fail)?.project;
    let req = RenderRequest { from_tick: 0, to_tick: dur_ticks + 1, tail_ms, seed: 0, stems: false, format: SampleFormat::F32 };
    let r = agb_render::render(&p, &req).map_err(fail)?;
    // trim trailing silence
    let end = r.mix.iter().rposition(|x| x.abs() > 1e-5).map_or(0, |i| i + 1);
    let bytes = agb_render::encode_wav(&r.mix[..end.max(1)], p.sample_rate, SampleFormat::F32);
    if let Some(d) = out.parent() {
        std::fs::create_dir_all(d).map_err(|e| fail(AgbError::io(e)))?;
    }
    std::fs::write(out, bytes).map_err(|e| fail(AgbError::io(e)))?;
    Ok(json!({"ok": true, "out": out, "samples": end, "peakDbfs": r.metrics.mix.peak_dbfs}))
}
