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
        ("song.schema.json", agb_song::song_schema()),
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

// ---------------------------------------------------------------------------
// `.agbsong` files (FORMAT.md)
// ---------------------------------------------------------------------------

fn has_song_extension(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("agbsong"))
}

/// Create a new source folder holding a small playable song. `.agbsong` is
/// reserved for the packed single file (`song pack`), so the folder may not use it.
pub fn song_new(out: &Path, title: &str) -> CmdResult {
    if has_song_extension(out) {
        return Err(fail(AgbError::new(
            ErrorCode::SchemaViolation,
            "`.agbsong` names the packed file; give the source folder another name (e.g. my-song.src) and run `agb song pack` to produce the .agbsong",
        )));
    }
    if out.exists() {
        return Err(fail(AgbError::io(format!("{} already exists", out.display()))));
    }
    std::fs::create_dir_all(out).map_err(|e| fail(AgbError::io(e)))?;
    let mut bytes = serde_json::to_vec_pretty(&agb_song::template(title)).unwrap();
    bytes.push(b'\n');
    std::fs::write(out.join(agb_song::SONG_FILE), bytes).map_err(|e| fail(AgbError::io(e)))?;
    std::fs::write(out.join("mimetype"), agb_song::MIMETYPE).map_err(|e| fail(AgbError::io(e)))?;
    Ok(json!({ "ok": true, "song": out, "files": ["mimetype", agb_song::SONG_FILE] }))
}

/// What this build would do with the song: playable or not, and what it skips.
/// Audio files of the sample assets a song's sampler patches use, as plain files:
/// in place for a directory package, extracted into `dir` from a zip.
fn sample_files(pkg: &agb_song::Package, song: &agb_song::Song, dir: &Path) -> Result<std::collections::BTreeMap<String, String>, String> {
    fn ids<'a>(p: &'a agb_song::model::Patch, out: &mut Vec<&'a str>) {
        out.extend(p.zones.iter().map(|z| z.sample.as_str()));
        p.fallback.iter().for_each(|f| ids(f, out));
    }
    let mut wanted = vec![];
    song.patches.values().for_each(|p| ids(p, &mut wanted));
    let mut files = std::collections::BTreeMap::new();
    for id in wanted {
        let Some(asset) = song.assets.get(id) else { continue };
        if files.contains_key(id) {
            continue;
        }
        let path = if pkg.is_zip() {
            let sub = dir.join("samples");
            std::fs::create_dir_all(&sub).map_err(|e| e.to_string())?;
            let dest = sub.join(format!("{id}-{}", asset.path.rsplit('/').next().unwrap_or("sample")));
            std::fs::write(&dest, pkg.read(&asset.path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            dest
        } else {
            pkg.location().join(&asset.path)
        };
        files.insert(id.to_string(), path.to_string_lossy().into_owned());
    }
    Ok(files)
}

/// A scratch directory for samples extracted while validating or rendering.
fn sample_scratch() -> PathBuf {
    std::env::temp_dir().join(format!("agb-samples-{}", std::process::id()))
}

fn player_report(song: &agb_song::Song, pkg: &agb_song::Package) -> Value {
    let samples = sample_files(pkg, song, &sample_scratch()).unwrap_or_default();
    match agb_song::lower(song, &agb_song::PlayOptions { samples, ..Default::default() }) {
        Ok(l) => json!({
            "playable": true,
            "features": agb_song::PLAYER_FEATURES,
            "lengthTicks": l.length_ticks,
            "skipped": l.skipped,
        }),
        Err(e) => json!({ "playable": false, "features": agb_song::PLAYER_FEATURES, "error": e }),
    }
}

/// Validate against the format, then report what this player can do with it.
/// `ok` is false when the file is not a valid `.agbsong`.
pub fn song_validate(path: &Path) -> CmdResult {
    let pkg = agb_song::Package::open(path).map_err(fail)?;
    let song = pkg.load_song().map_err(fail)?;
    let report = agb_song::validate(&song, Some(&pkg));
    let valid = report.is_valid();
    let mut out = json!({
        "ok": valid,
        "valid": valid,
        "formatVersion": song.format_version,
        "errors": report.errors,
        "warnings": report.warnings,
    });
    if valid {
        out["player"] = player_report(&song, &pkg);
    }
    Ok(out)
}

pub fn song_info(path: &Path) -> CmdResult {
    let pkg = agb_song::Package::open(path).map_err(fail)?;
    let song = pkg.load_song().map_err(fail)?;
    let files = pkg.list().map_err(fail)?;
    let tracks: Vec<Value> = song
        .tracks
        .iter()
        .map(|t| {
            json!({
                "id": t.id, "role": t.role, "patch": t.patch,
                "engine": song.patches.get(&t.patch).and_then(|p| p.engine.clone()),
                "loops": t.loops.iter().map(|l| json!({ "id": l.id, "lengthSteps": l.length_steps })).collect::<Vec<_>>(),
                "clips": t.clips.len(),
            })
        })
        .collect();
    Ok(json!({
        "ok": true,
        "container": if pkg.is_zip() { "zip" } else { "directory" },
        "format": song.format,
        "formatVersion": song.format_version,
        "requires": song.requires,
        "uses": song.uses,
        "featuresDetected": agb_song::validate::features_used(&song),
        "meta": song.meta,
        "time": song.time,
        "tuning": song.tuning,
        "tracks": tracks,
        "sections": song.sections.iter().map(|s| json!({ "id": s.id, "fromTick": s.from_tick, "toTick": s.to_tick, "intent": s.intent })).collect::<Vec<_>>(),
        "overlay": song.overlay,
        "performance": song.performance,
        "files": files.iter().map(|(n, s)| json!({ "path": n, "bytes": s })).collect::<Vec<_>>(),
        "player": player_report(&song, &pkg),
    }))
}

pub fn song_pack(dir: &Path, out: &Path) -> CmdResult {
    if !has_song_extension(out) {
        return Err(fail(AgbError::new(ErrorCode::SchemaViolation, "the packed song must be named *.agbsong")));
    }
    let files = agb_song::pack(dir, out).map_err(fail)?;
    let sha = agb_assets::sha256_file(out).map_err(|e| fail(AgbError::io(e)))?;
    Ok(json!({ "ok": true, "out": out, "files": files, "sha256": sha }))
}

/// `agb song export`: a performance as a new song. The project is scheduled once from the first bar
/// to the last one played, and what comes out — every note at the pitch, length and loudness it was
/// played with, after all the changes made along the way — is written down as clips. A track whose
/// sound was changed during the performance (another preset, other parameters, another level) becomes
/// one track per sound, each holding the notes played with it.
pub fn song_export(project: &Path, out: &Path, title: Option<&str>) -> CmdResult {
    use agb_score::ParamValue;
    use std::collections::BTreeMap;
    if !has_song_extension(out) {
        return Err(fail(AgbError::new(ErrorCode::SchemaViolation, "the song must be named *.agbsong")));
    }
    let io = |e: std::io::Error| fail(AgbError::io(e));
    let p = agb_engine::ProjectStore::new(project).load().map_err(fail)?;
    let state: Option<agb_agent::PerformState> =
        std::fs::read_to_string(project.join("perform.json")).ok().and_then(|s| serde_json::from_str(&s).ok());
    let bar = agb_engine::clock::bar_ticks(p.ppqn);
    let mut end = state.as_ref().map_or(0, |s| s.rendered_until);
    if end == 0 {
        // No record of how far it was played: up to the last thing written into the score.
        for e in &p.timeline {
            end = end.max(e.effective_tick);
            for c in &e.changes {
                if let agb_score::Change::Clip { notes, .. } = c {
                    end = end.max(notes.iter().map(|n| n.tick + n.dur_ticks).max().unwrap_or(0));
                }
            }
        }
        end = end.div_ceil(bar).max(1) * bar;
    }
    let sched = agb_engine::schedule(&p, 0, end).map_err(fail)?;
    if sched.events.is_empty() {
        return Err(fail(AgbError::new(ErrorCode::SchemaViolation, "nothing was played: there are no notes to save")));
    }

    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let src = std::env::temp_dir().join(format!("agb-export-{}-{stamp}.src", std::process::id()));
    std::fs::create_dir_all(&src).map_err(io)?;
    let result = (|| -> CmdResult {
        struct Sound {
            source: String,
            patch: Value,
            gain_db: f64,
            transposable: bool,
            notes: Vec<Value>,
        }
        let mut sounds: Vec<Sound> = vec![];
        let mut by_key: BTreeMap<String, usize> = BTreeMap::new();
        let mut assets = serde_json::Map::new();
        let mut copied: BTreeMap<String, String> = BTreeMap::new(); // sample path → asset id
        let mut uses_sampler = false;
        // Score state changes only at timeline entries: look it up once per entry tick.
        let mut snaps: BTreeMap<u64, agb_score::Snapshot> = BTreeMap::new();
        let mut ticks: Vec<u64> = p.timeline.iter().map(|e| e.effective_tick).collect();
        ticks.sort_unstable();
        ticks.dedup();

        for ev in &sched.events {
            let at = ticks.iter().rev().find(|t| **t <= ev.tick).copied().unwrap_or(0);
            let snap = snaps.entry(at).or_insert_with(|| agb_score::timeline::snapshot_at(&p.timeline, at));
            let Some(track) = snap.track(&ev.track_id) else { continue };
            let layer_gain = track.layers.iter().find(|l| l.id == ev.layer_id).map_or(0.0, |l| l.gain_db);
            let key = serde_json::to_string(&json!([ev.track_id, ev.engine_id, ev.preset_id, track.params, track.gain_db])).unwrap();
            let si = match by_key.get(&key) {
                Some(i) => *i,
                None => {
                    let mut params = track.params.clone();
                    let mut patch = serde_json::Map::new();
                    if let Some(ParamValue::Str(zones)) = params.remove("zones") {
                        // A sampler: its samples travel with the new song.
                        uses_sampler = true;
                        let list: Vec<Value> = serde_json::from_str(&zones).unwrap_or_default();
                        let mut out_zones = vec![];
                        for z in &list {
                            let Some(path) = z["path"].as_str() else { continue };
                            let id = match copied.get(path) {
                                Some(id) => id.clone(),
                                None => {
                                    let id = format!("smp{}", copied.len() + 1);
                                    let name = Path::new(path).file_name().map_or("sample.wav".into(), |n| n.to_string_lossy().into_owned());
                                    let rel = format!("samples/{id}_{name}");
                                    std::fs::create_dir_all(src.join("samples")).map_err(io)?;
                                    let bytes = std::fs::read(path).map_err(io)?;
                                    std::fs::write(src.join(&rel), &bytes).map_err(io)?;
                                    assets.insert(
                                        id.clone(),
                                        json!({ "path": rel, "kind": "sample", "sha256": agb_song::package::sha256_tag(&bytes),
                                                "license": { "spdx": "LicenseRef-inherited", "redistributable": false,
                                                             "notes": "copied from the song this performance was played from; its terms apply" } }),
                                    );
                                    copied.insert(path.to_string(), id.clone());
                                    id
                                }
                            };
                            let mut zone = json!({ "sample": id, "keyRange": [z["lo"], z["hi"]], "rootPitch": z["root"],
                                                   "tuneCents": z["tuneCents"], "gainDb": z["gainDb"],
                                                   "velRange": [z["velLo"], z["velHi"]], "oneShot": z["oneShot"], "startFrame": z["start"] });
                            if !z["end"].is_null() {
                                zone["endFrame"] = z["end"].clone();
                            }
                            if !z["loopStart"].is_null() && !z["loopEnd"].is_null() {
                                zone["loop"] = json!({ "startFrame": z["loopStart"], "endFrame": z["loopEnd"] });
                            }
                            out_zones.push(zone);
                        }
                        patch.insert("zones".into(), Value::Array(out_zones));
                    }
                    match &ev.preset_id {
                        Some(preset) => patch.insert("preset".into(), json!(preset)),
                        None => patch.insert("engine".into(), json!(ev.engine_id)),
                    };
                    if !params.is_empty() {
                        patch.insert("params".into(), json!(params));
                    }
                    sounds.push(Sound {
                        source: ev.track_id.clone(),
                        patch: Value::Object(patch),
                        gain_db: track.gain_db,
                        transposable: track.follows_transpose(),
                        notes: vec![],
                    });
                    by_key.insert(key, sounds.len() - 1);
                    sounds.len() - 1
                }
            };
            // A cut silences what is sounding on the track.
            let off = sched
                .cuts
                .iter()
                .filter(|c| c.track_id == ev.track_id && c.tick > ev.tick && c.tick < ev.off_tick)
                .map(|c| c.tick)
                .min()
                .unwrap_or(ev.off_tick);
            let velocity = (ev.velocity * agb_dsp::detmath::db_to_gain(layer_gain)).clamp(0.01, 1.0);
            sounds[si].notes.push(json!({ "tick": ev.tick, "pitch": ev.pitch, "durTicks": off.saturating_sub(ev.tick).max(1),
                                          "velocity": (velocity * 1000.0).round() / 1000.0 }));
        }

        // Parameter moves, per source track and parameter, as points.
        let mut lanes: BTreeMap<(String, String), Vec<&agb_score::AutomationLane>> = BTreeMap::new();
        for l in &sched.automation {
            lanes.entry((l.lane.track_id.clone(), l.lane.param.clone())).or_default().push(&l.lane);
        }
        let automation = |source: &str| -> Vec<Value> {
            let mut out = vec![];
            for ((track, param), ls) in &lanes {
                if track != source {
                    continue;
                }
                let mut ls = ls.clone();
                ls.sort_by_key(|l| l.from_tick);
                let mut points: Vec<Value> = vec![];
                for l in ls {
                    points.push(if points.is_empty() { json!([l.from_tick, l.from]) } else { json!([l.from_tick, l.from, "step"]) });
                    if l.to_tick > l.from_tick {
                        let curve = if l.mode == agb_score::AutomationMode::Ramp { "linear" } else { "step" };
                        points.push(json!([l.to_tick, l.to, curve]));
                    }
                }
                out.push(json!({ "target": format!("patch.{param}"), "points": points }));
            }
            out
        };

        let mut patches = serde_json::Map::new();
        let mut tracks = vec![];
        let mut count: BTreeMap<String, usize> = BTreeMap::new();
        let mut note_total = 0usize;
        for s in &sounds {
            let n = count.entry(s.source.clone()).or_insert(0);
            *n += 1;
            let id = if *n == 1 { s.source.clone() } else { format!("{}_v{}", s.source, n) };
            let id: String = id.chars().take(64).collect();
            patches.insert(id.clone(), s.patch.clone());
            note_total += s.notes.len();
            let mut t = json!({ "id": id, "name": if *n == 1 { s.source.clone() } else { format!("{} {}", s.source, n) },
                                "patch": id, "gainDb": s.gain_db, "transposable": s.transposable,
                                "clips": [{ "atTick": 0, "notes": s.notes }] });
            let auto = automation(&s.source);
            if !auto.is_empty() {
                t["automation"] = Value::Array(auto);
            }
            tracks.push(t);
        }

        // The phrases of the performance become the sections.
        let mut sections = vec![];
        if let Some(st) = &state {
            let mut ph: Vec<&agb_agent::perform::PhraseRecord> = st.phrases.iter().filter(|r| r.applied && r.start_tick < end).collect();
            ph.sort_by_key(|r| r.start_tick);
            ph.dedup_by_key(|r| r.start_tick);
            for (i, r) in ph.iter().enumerate() {
                let to = ph.get(i + 1).map_or(end, |n| n.start_tick);
                let mut s = json!({ "id": format!("p{}", i + 1), "fromTick": r.start_tick, "toTick": to });
                if !r.title.is_empty() {
                    s["name"] = json!(r.title);
                }
                if !r.commentary.is_empty() {
                    s["intent"] = json!(r.commentary);
                }
                sections.push(s);
            }
        }

        let brief = state.as_ref().map(|s| s.brief.clone()).filter(|b| !b.is_empty());
        let name = title.map(str::to_string).or_else(|| brief.clone()).unwrap_or_else(|| p.name.clone());
        let model = state.as_ref().and_then(|s| s.phrases.iter().rev().find_map(|r| r.model.clone()));
        let mut artist = json!({ "name": "Agent Groovebox", "kind": "ai" });
        if let Some(m) = model {
            artist["model"] = json!(m);
        }
        let mut uses = vec!["agent"];
        if tracks.iter().any(|t| t.get("automation").is_some()) {
            uses.push("automation");
        }
        if uses_sampler {
            uses.push("sampler");
        }
        let mut time = json!({ "ppqn": p.ppqn, "bpm": p.tempo_bpm, "meter": [4, 4], "stepsPerQuarter": p.steps_per_quarter, "lengthTicks": end });
        if !p.tempo_map.is_empty() {
            time["tempoMap"] = json!(p.tempo_map.iter().map(|t| json!({ "tick": t.tick, "bpm": t.bpm })).collect::<Vec<_>>());
        }
        let mut song = json!({
            "format": "agbsong", "formatVersion": "0.1.0", "requires": ["core"], "uses": uses,
            "meta": { "title": name, "artists": [artist],
                      "description": "一次实时演出的记录：按实际奏出的音符保存，包含演出过程中的全部改动。",
                      "generator": { "name": "agb song export", "version": env!("CARGO_PKG_VERSION") } },
            "time": time, "seed": state.as_ref().map_or(p.seed, |s| s.seed), "patches": patches, "tracks": tracks,
            "master": { "gainDb": p.master_gain_db, "limiter": p.master_limiter },
        });
        if !assets.is_empty() {
            song["assets"] = Value::Object(assets);
        }
        if !sections.is_empty() {
            song["sections"] = Value::Array(sections);
        }
        if let Some(b) = brief {
            song["performance"] = json!({ "agent": { "brief": b } });
        }
        if let Some(st) = &state {
            let talk: Vec<Value> = st.dialogue.iter().filter(|d| d.who != "event").map(|d| json!({ "who": d.who, "text": d.text, "tick": d.tick })).collect();
            if !talk.is_empty() {
                song["extensions"] = json!({ "agb.session": { "dialogue": talk } });
            }
        }
        let mut bytes = serde_json::to_vec_pretty(&song).unwrap();
        bytes.push(b'\n');
        std::fs::write(src.join(agb_song::SONG_FILE), bytes).map_err(io)?;
        std::fs::write(src.join("mimetype"), agb_song::MIMETYPE).map_err(io)?;

        // Never hand out a file this player would refuse.
        let pkg = agb_song::Package::open(&src).map_err(fail)?;
        let parsed = pkg.load_song().map_err(fail)?;
        let report = agb_song::validate(&parsed, Some(&pkg));
        if let Some(first) = report.errors.first() {
            return Err(fail(AgbError::new(ErrorCode::SchemaViolation, format!("the exported song is not valid: {} ({})", first.message, first.path))));
        }
        if let Some(parent) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        if out.exists() {
            std::fs::remove_file(out).map_err(io)?;
        }
        agb_song::pack(&src, out).map_err(fail)?;
        Ok(json!({ "ok": true, "out": out, "title": parsed.meta.title, "tracks": parsed.tracks.len(), "notes": note_total,
                   "lengthTicks": end, "bars": end / bar, "sections": parsed.sections.len(),
                   "warnings": report.warnings.iter().map(|w| &w.message).collect::<Vec<_>>() }))
    })();
    let _ = std::fs::remove_dir_all(&src);
    result
}

pub fn song_unpack(file: &Path, out_dir: &Path) -> CmdResult {
    let files = agb_song::unpack(file, out_dir).map_err(fail)?;
    Ok(json!({ "ok": true, "outDir": out_dir, "files": files }))
}

pub struct SongRenderArgs {
    pub out_dir: PathBuf,
    pub transpose: i32,
    pub bpm: Option<f64>,
    pub from_tick: Option<u64>,
    pub to_tick: Option<u64>,
    pub seed: Option<u64>,
    pub stems: bool,
    pub tail_ms: Option<u32>,
    pub format: SampleFormat,
}

/// Render a song file. Transposition and tempo are applied at play time: the
/// file is not changed.
pub fn song_render(path: &Path, a: &SongRenderArgs) -> CmdResult {
    let pkg = agb_song::Package::open(path).map_err(fail)?;
    let song = pkg.load_song().map_err(fail)?;
    let report = agb_song::validate(&song, Some(&pkg));
    if !report.is_valid() {
        return Ok(json!({ "ok": false, "valid": false, "errors": report.errors, "warnings": report.warnings }));
    }
    let samples = sample_files(&pkg, &song, &sample_scratch()).map_err(|e| fail(AgbError::io(e)))?;
    let opts = agb_song::PlayOptions { transpose: a.transpose, bpm: a.bpm, samples };
    let l = agb_song::lower(&song, &opts).map_err(fail)?;
    let req = RenderRequest {
        from_tick: a.from_tick.unwrap_or(0),
        to_tick: a.to_tick.unwrap_or(l.length_ticks),
        tail_ms: a.tail_ms.unwrap_or(2000),
        seed: a.seed.unwrap_or(song.seed),
        stems: a.stems,
        format: a.format,
    };
    let mut v = render_to_dir(&l.project, &req, &a.out_dir, "mix.wav").map_err(fail)?;
    v["op"] = json!("song.render");
    v["ok"] = json!(true);
    v["title"] = json!(song.meta.title);
    v["transpose"] = json!(a.transpose);
    v["bpm"] = json!(l.project.tempo_bpm);
    v["lengthTicks"] = json!(l.length_ticks);
    v["skipped"] = json!(l.skipped);
    Ok(v)
}

pub struct SongPlayArgs {
    /// Working directory (engine project, bar chunks, final render, take). Must not exist yet.
    pub work_dir: PathBuf,
    pub transpose: i32,
    pub bpm: Option<f64>,
    /// Pace by the wall clock instead of rendering as fast as possible.
    pub realtime: bool,
    /// Answer the listener with the built-in offline performer.
    pub mock: bool,
    pub models: Vec<String>,
    pub base_url: Option<String>,
    pub api_key_file: Option<PathBuf>,
    pub max_retries: u32,
}

/// Play a song for the app: the same NDJSON event stream as `agb perform`
/// (`session` / `change` / `chunk` / `chat` / `done` / `fatal`), one chunk per
/// bar with per-track stems. Sections are the phrases shown on stage. The
/// listener talks to the performer through NDJSON commands on stdin (`chat`,
/// `set_tempo`, `human_state`, `finish`); without an API key the song still
/// plays, it just cannot be changed by conversation.
pub fn song_play(path: &Path, a: &SongPlayArgs) -> i32 {
    use std::io::Write;
    let mut emit = |v: Value| {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{v}");
        let _ = out.flush();
    };
    let prepared = (|| -> Result<(agb_score::Project, agb_agent::SongContext, u64), String> {
        let pkg = agb_song::Package::open(path).map_err(|e| e.to_string())?;
        let song = pkg.load_song().map_err(|e| e.to_string())?;
        let report = agb_song::validate(&song, Some(&pkg));
        if let Some(first) = report.errors.first() {
            return Err(format!("不是有效的 .agbsong：{}（{}）", first.message, first.path));
        }
        let samples = sample_files(&pkg, &song, &a.work_dir)?;
        let l = agb_song::lower(&song, &agb_song::PlayOptions { transpose: a.transpose, bpm: a.bpm, samples }).map_err(|e| e.to_string())?;
        let agent = song.performance.as_ref().and_then(|p| p.agent.as_ref());
        // The app shows the cover and plays the video from plain files: use the
        // file in place for a directory package, extract it from a zip.
        let on_disk = |asset_id: Option<&String>| -> Result<Option<String>, String> {
            let Some(asset) = asset_id.and_then(|id| song.assets.get(id)) else { return Ok(None) };
            if !pkg.is_zip() {
                return Ok(Some(pkg.location().join(&asset.path).to_string_lossy().into_owned()));
            }
            let name = asset.path.rsplit('/').next().unwrap_or("asset");
            let dir = a.work_dir.join("assets");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let dest = dir.join(name);
            std::fs::write(&dest, pkg.read(&asset.path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            Ok(Some(dest.to_string_lossy().into_owned()))
        };
        let video = song.visuals.as_ref().and_then(|v| v.video.as_ref());
        let ctx = agb_agent::SongContext {
            cover: on_disk(song.meta.cover.as_ref())?,
            video: on_disk(video.map(|v| &v.asset))?,
            video_sync: video.and_then(|v| v.sync.clone()),
            written_bpm: song.time.tempo_map.is_empty().then_some(song.time.bpm),
            video_aspect: video
                .and_then(|v| song.assets.get(&v.asset))
                .and_then(|a| pkg.read(&a.path).ok())
                .and_then(|b| agb_song::package::video_dimensions(&b))
                .map(|(w, h)| w as f64 / h as f64),
            song_path: path.to_string_lossy().into_owned(),
            title: song.meta.title.clone(),
            description: song.meta.description.clone().unwrap_or_default(),
            author: song.meta.artists.first().map(|x| x.model.clone().unwrap_or_else(|| x.name.clone())).unwrap_or_else(|| "agbsong".into()),
            key: song.tuning.key.clone(),
            scale: song.tuning.scale.clone(),
            sections: song
                .sections
                .iter()
                .map(|s| agb_agent::SongSection {
                    from_tick: s.from_tick,
                    to_tick: s.to_tick,
                    name: s.name.clone().unwrap_or_else(|| s.id.clone()),
                    intent: s.intent.clone().unwrap_or_default(),
                })
                .collect(),
            brief: agent.and_then(|x| x.brief.clone()),
            persona: agent.and_then(|x| x.persona.clone()),
            rules: agent.map(|x| x.rules.clone()).unwrap_or_default(),
            allow: agent.map(|x| x.allow.clone()).filter(|x| !x.is_empty()),
            length_ticks: l.length_ticks,
            transpose: a.transpose,
            transpose_range: song.performance.as_ref().and_then(|p| p.transpose.as_ref()).map(|r| (r.min, r.max)),
            bpm_range: song.performance.as_ref().and_then(|p| p.bpm.as_ref()).map(|r| (r.min, r.max)),
            skipped: json!(l.skipped),
        };
        Ok((l.project, ctx, song.seed))
    })();
    let (project, ctx, seed) = match prepared {
        Ok(x) => x,
        Err(message) => {
            emit(json!({ "type": "fatal", "message": message }));
            return 1;
        }
    };
    let llm: Option<Box<dyn agb_agent::Llm + Send>> = if a.mock {
        Some(Box::new(agb_agent::MockPerformer::default()))
    } else {
        agb_agent::llm::load_api_key(a.api_key_file.as_deref())
            .ok()
            .map(|k| Box::new(agb_agent::HttpLlm::new(a.base_url.clone(), k, a.models.clone())) as Box<dyn agb_agent::Llm + Send>)
    };
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
    let cfg = agb_agent::JamConfig { root: a.work_dir.clone(), seed, max_retries: a.max_retries, realtime: a.realtime };
    let (base_url, models) = (a.base_url.clone(), a.models.clone());
    let connect = move |key: &str| -> Box<dyn agb_agent::Llm + Send> {
        Box::new(agb_agent::HttpLlm::new(base_url.clone(), key.to_string(), models.clone()))
    };
    match agb_agent::jam(&cfg, project, &ctx, llm, Some(&connect), rx, &mut emit) {
        Ok(()) => 0,
        Err(message) => {
            emit(json!({ "type": "fatal", "message": message }));
            1
        }
    }
}
