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
fn player_report(song: &agb_song::Song) -> Value {
    match agb_song::lower(song, &agb_song::PlayOptions::default()) {
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
        out["player"] = player_report(&song);
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
        "player": player_report(&song),
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
    let opts = agb_song::PlayOptions { transpose: a.transpose, bpm: a.bpm };
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
        let l = agb_song::lower(&song, &agb_song::PlayOptions { transpose: a.transpose, bpm: a.bpm }).map_err(|e| e.to_string())?;
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
