//! Phrase-by-phrase AI performance with a listener in the loop.
//!
//! The AI sends one JSON reply per *phrase* (score ops + how many bars the
//! phrase lasts + an optional spoken reply). Each reply is committed as one
//! transaction quantized to a bar line that has not been rendered yet, so a
//! change can never alter audio that was already emitted (SPEC §5/§8).
//!
//! The listener interacts through [`Command`]s (NDJSON on stdin for the CLI):
//! * `chat` — cancels the phrase being prepared and re-asks the AI with the
//!   message; the answer lands on the next free bar (not the phrase boundary);
//! * `set_tempo` — committed as a `human.*` transaction at the next free bar
//!   (tempo map), the band follows;
//! * `human_state` — the listener's live controls (mixer, FX, pads, healing
//!   generators), forwarded to the AI as context;
//! * `finish` — the phrase being prepared becomes the final one.
//!
//! Audio is rendered one bar at a time, cumulatively from tick 0; only the new
//! bar is emitted (mix + per-track stems). Deterministic rendering makes the
//! chunks join seamlessly.

use crate::llm::{extract_json, Llm, Msg, Reply};
use crate::prompt::{phrase_request, system_prompt, PhraseInfo, STEPS_PER_QUARTER};
use agb_engine::clock::{bar_ticks, ceil_to_grid};
use agb_engine::{apply, set_transport, ApplyOptions, NoRender, ProjectStore};
use agb_protocol::SampleFormat;
use agb_render::{encode_wav, render_tail, render_to_dir, RenderRequest};
use agb_score::{MasterLimiter, Project};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub const PERFORM_FILE: &str = "perform.json";
pub const FINAL_TAIL_MS: u32 = 2500;
pub const MIN_BARS: u32 = 2;
pub const MAX_BARS: u32 = 16;

#[derive(Clone, Debug)]
pub struct PerformConfig {
    pub project_root: PathBuf,
    pub brief: String,
    /// Number of phrases (AI turns) to perform.
    pub phrases: u32,
    /// Phrase length used when the AI does not say (`bars` in its reply).
    pub default_bars: u32,
    pub seed: u64,
    pub max_retries: u32,
    pub continue_existing: bool,
    pub realtime: bool,
    /// Realtime: how many bars to stay ahead of the listener.
    pub lookahead_bars: u32,
}

impl PerformConfig {
    pub fn new(project_root: impl Into<PathBuf>, brief: impl Into<String>) -> Self {
        PerformConfig {
            project_root: project_root.into(),
            brief: brief.into(),
            phrases: 6,
            default_bars: 8,
            seed: 0,
            max_retries: 3,
            continue_existing: false,
            realtime: false,
            lookahead_bars: 2,
        }
    }
}

/// Listener → performer commands.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Chat(String),
    SetTempo(f64),
    /// Absolute transposition in semitones relative to the written score.
    SetTranspose(i32),
    HumanState(Value),
    /// The API key became available after the engine was started (song playback).
    SetKey(String),
    Finish,
}

/// Parse one NDJSON command line, e.g. `{"type":"chat","text":"slower please"}`.
pub fn parse_command(line: &str) -> Option<Command> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    match v.get("type")?.as_str()? {
        "chat" => Some(Command::Chat(v.get("text")?.as_str()?.trim().to_string())).filter(|c| *c != Command::Chat(String::new())),
        "set_tempo" => Some(Command::SetTempo(v.get("bpm")?.as_f64()?)),
        "set_transpose" => Some(Command::SetTranspose(v.get("semitones")?.as_i64()? as i32)),
        "human_state" => Some(Command::HumanState(v.get("state")?.clone())),
        "set_key" => Some(Command::SetKey(v.get("key")?.as_str()?.trim().to_string())).filter(|c| *c != Command::SetKey(String::new())),
        "finish" => Some(Command::Finish),
        _ => None,
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PhraseRecord {
    pub index: usize,
    pub title: String,
    pub commentary: String,
    /// Tick where the phrase's ops took effect (planned start if skipped).
    pub start_tick: u64,
    pub bars: u32,
    pub applied: bool,
    pub model: Option<String>,
    pub revision: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DialogueLine {
    /// "listener" | "ai" | "event"
    pub who: String,
    pub text: String,
    /// Score position (rendered-until tick) when it was said.
    pub tick: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct PerformState {
    pub brief: String,
    pub seed: u64,
    pub phrases: Vec<PhraseRecord>,
    /// Audio has been emitted for `[0, renderedUntil)`.
    pub rendered_until: u64,
    pub chunks: Vec<String>,
    #[serde(default)]
    pub dialogue: Vec<DialogueLine>,
}

const FORBIDDEN_PREFIXES: &[&str] = &["render.", "system.", "scene."];

fn load_state(root: &Path) -> Option<PerformState> {
    serde_json::from_str(&std::fs::read_to_string(root.join(PERFORM_FILE)).ok()?).ok()
}

pub(crate) fn save_state(root: &Path, s: &PerformState) -> Result<(), String> {
    std::fs::write(root.join(PERFORM_FILE), serde_json::to_vec_pretty(s).unwrap()).map_err(|e| e.to_string())
}

fn init_project(store: &ProjectStore, brief: &str) -> Result<Project, String> {
    let mut p = store.init(None).map_err(|e| e.to_string())?;
    p.steps_per_quarter = STEPS_PER_QUARTER;
    p.master_limiter = MasterLimiter::Soft;
    p.name = brief.chars().take(60).collect::<String>().trim().to_string();
    if p.name.is_empty() {
        p.name = "performance".into();
    }
    store.save(&p).map_err(|e| e.to_string())?;
    Ok(p)
}

/// Validate a reply and turn phrase-relative ticks into absolute ones.
pub fn prepare_ops(reply: &Value, start_tick: u64) -> Result<Vec<Value>, String> {
    let ops = reply.get("ops").and_then(|o| o.as_array()).ok_or("reply has no \"ops\" array")?;
    let mut out = Vec::with_capacity(ops.len());
    for (i, op) in ops.iter().enumerate() {
        let name = op.get("op").and_then(|n| n.as_str()).unwrap_or("");
        if FORBIDDEN_PREFIXES.iter().any(|p| name.starts_with(p)) {
            return Err(format!("ops[{i}]: op {name:?} is managed by the performer; remove it"));
        }
        let mut op = op.clone();
        // Common slip: preset id without the `preset:` prefix.
        if let Some(pid) = op.get("presetId").and_then(|v| v.as_str()) {
            let fixed = format!("preset:{pid}");
            if !pid.starts_with("preset:") && agb_presets::get(&fixed).is_some() {
                op["presetId"] = json!(fixed);
            }
        }
        let shift = |v: &mut Value, key: &str| -> Result<(), String> {
            if let Some(x) = v.get_mut(key) {
                let rel = x.as_u64().ok_or_else(|| format!("ops[{i}].{key} must be a non-negative integer (relative tick)"))?;
                *x = json!(rel + start_tick);
            }
            Ok(())
        };
        match name {
            "automation.schedule" => {
                shift(&mut op, "fromTick")?;
                shift(&mut op, "toTick")?;
            }
            "clip.clear" => shift(&mut op, "toTick")?,
            "clip.schedule" => {
                if let Some(notes) = op.get_mut("notes").and_then(|n| n.as_array_mut()) {
                    for n in notes {
                        shift(n, "tick")?;
                    }
                }
            }
            _ => {}
        }
        out.push(op);
    }
    Ok(out)
}

/// The listener transposes the piece from bar line `tick` on. Emits the result
/// and returns the committed transaction.
pub(crate) fn listener_transpose(store: &ProjectStore, tick: u64, semitones: i32, emit: &mut dyn FnMut(Value)) -> Result<Option<Value>, String> {
    let p = store.load().map_err(|e| e.to_string())?;
    set_transport(store, tick).map_err(|e| e.to_string())?;
    let txn = json!({
        "commandId": format!("human-transpose-r{}-t{tick}", p.revision),
        "actor": "human.listener", "ifMatchRevision": p.revision,
        "quantize": {"type": "tick", "tick": tick}, "ops": [{"op": "project.setTranspose", "semitones": semitones}],
    });
    match apply(store, &txn.to_string(), ApplyOptions::default(), &mut NoRender) {
        Ok(r) => {
            emit(json!({"type": "human_change", "kind": "transpose", "semitones": semitones, "tick": tick, "revision": r.revision}));
            Ok(Some(txn))
        }
        Err(e) => {
            emit(json!({"type": "human_rejected", "kind": "transpose", "error": serde_json::to_value(&e).unwrap()["error"]}));
            Ok(None)
        }
    }
}

/// The phrase currently being negotiated with the AI.
struct Pending {
    index: usize,
    planned_start: u64,
    messages: Vec<Msg>,
    attempts: u32,
    in_flight: bool,
    /// Requests are tagged; replies from a superseded generation are dropped.
    generation: u64,
    /// Answer to the listener: commit on the next free bar.
    urgent: bool,
    /// Parsed reply waiting for its commit tick.
    ready: Option<(Value, String)>,
}

/// Conversation and listener context shared across requests.
#[derive(Default)]
struct Listener {
    unanswered: Vec<String>,
    human_state: Option<Value>,
    events: Vec<String>,
}

pub fn perform(
    cfg: &PerformConfig,
    llm: Box<dyn Llm + Send>,
    commands: mpsc::Receiver<Command>,
    emit: &mut dyn FnMut(Value),
) -> Result<PerformState, String> {
    let root = &cfg.project_root;
    let store = ProjectStore::new(root);
    let mut state = if cfg.continue_existing {
        let s = load_state(root).ok_or("nothing to continue: perform.json not found")?;
        store.load().map_err(|e| e.to_string())?;
        s
    } else {
        if store.exists() {
            return Err(format!("{} already contains a project; use a new directory or --continue", root.display()));
        }
        init_project(&store, &cfg.brief)?;
        PerformState { brief: cfg.brief.clone(), seed: cfg.seed, ..Default::default() }
    };
    let out_dir = root.join("renders").join("perform");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let p0 = store.load().map_err(|e| e.to_string())?;
    let bar = bar_ticks(p0.ppqn);
    let first = state.phrases.len();
    let mut total = first + cfg.phrases as usize;
    emit(json!({
        "type": "session", "projectRoot": root, "brief": state.brief, "phrases": total, "firstPhrase": first,
        "realtime": cfg.realtime, "barTicks": bar, "sampleRate": p0.sample_rate, "ppqn": p0.ppqn,
        "stepsPerQuarter": p0.steps_per_quarter, "renderedUntil": state.rendered_until,
    }));

    let system = system_prompt();
    let (reply_tx, reply_rx) = mpsc::channel::<(u64, Result<Reply, String>)>();
    let mut generation = 0u64;
    let mut rendered_until = state.rendered_until;
    let mut final_end: Option<u64> = None;
    let mut started_at: Option<(Instant, f64)> = None; // wall clock origin ↔ sample
    let mut any_committed = first > 0;
    let mut listener = Listener::default();

    let build_request = |index: usize, planned_start: u64, total: usize, state: &PerformState, l: &Listener| -> Result<Vec<Msg>, String> {
        let p = store.load().map_err(|e| e.to_string())?;
        let tracks = agb_score::timeline::head(&p.timeline).tracks;
        let history: Vec<(String, String, u32)> =
            state.phrases.iter().map(|r| (r.title.clone(), r.commentary.clone(), r.bars)).collect();
        let dialogue: Vec<(String, String)> = state
            .dialogue
            .iter()
            .filter(|d| d.who != "event")
            .map(|d| (d.who.clone(), d.text.clone()))
            .collect();
        let info = PhraseInfo {
            dialogue: &dialogue,
            unanswered: &l.unanswered,
            human_state: l.human_state.as_ref(),
            human_events: &l.events,
            index,
            total,
            planned_start,
            bar_ticks: bar,
            default_bars: cfg.default_bars,
            brief: &state.brief,
            history: &history,
            realtime: cfg.realtime,
        };
        Ok(vec![Msg::system(&system), Msg::user(phrase_request(&p, &tracks, &info))])
    };
    let new_pending = |index: usize, planned_start: u64, total: usize, state: &PerformState, l: &Listener, generation: &mut u64| -> Result<Pending, String> {
        *generation += 1;
        Ok(Pending {
            index,
            planned_start,
            messages: build_request(index, planned_start, total, state, l)?,
            attempts: 0,
            in_flight: false,
            generation: *generation,
            urgent: false,
            ready: None,
        })
    };

    let mut pending: Option<Pending> = if first < total {
        let ps = ceil_to_grid(rendered_until, 0, bar);
        Some(new_pending(first, ps, total, &state, &listener, &mut generation)?)
    } else {
        None
    };

    loop {
        // 0. Listener commands.
        while let Ok(cmd) = commands.try_recv() {
            match cmd {
                Command::Chat(text) => {
                    emit(json!({"type": "chat", "who": "listener", "text": text}));
                    state.dialogue.push(DialogueLine { who: "listener".into(), text: text.clone(), tick: rendered_until });
                    listener.unanswered.push(text);
                    // Re-ask now with the message (drop whatever was being prepared).
                    if let Some(pd) = pending.as_ref() {
                        let (idx, ps) = (pd.index, pd.planned_start);
                        let mut np = new_pending(idx, ps, total, &state, &listener, &mut generation)?;
                        np.urgent = any_committed;
                        pending = Some(np);
                    }
                }
                Command::SetTempo(bpm) => {
                    let p = store.load().map_err(|e| e.to_string())?;
                    let tick = ceil_to_grid(rendered_until, 0, bar);
                    set_transport(&store, tick).map_err(|e| e.to_string())?;
                    let txn = json!({
                        "commandId": format!("human-tempo-r{}-t{tick}", p.revision),
                        "actor": "human.listener", "ifMatchRevision": p.revision,
                        "quantize": {"type": "tick", "tick": tick}, "ops": [{"op": "project.setTempo", "tempoBpm": bpm}],
                    });
                    match apply(&store, &txn.to_string(), ApplyOptions::default(), &mut NoRender) {
                        Ok(r) => {
                            let bar_no = tick / bar + 1;
                            emit(json!({"type": "human_change", "kind": "tempo", "bpm": bpm, "tick": tick, "revision": r.revision}));
                            listener.events.push(format!("set tempo to {bpm} BPM from bar {bar_no}"));
                            state.dialogue.push(DialogueLine { who: "event".into(), text: format!("BPM → {bpm}（第 {bar_no} 小节起）"), tick });
                        }
                        Err(e) => emit(json!({"type": "human_rejected", "kind": "tempo", "error": serde_json::to_value(&e).unwrap()["error"]})),
                    }
                }
                Command::SetTranspose(n) => {
                    let tick = ceil_to_grid(rendered_until, 0, bar);
                    if listener_transpose(&store, tick, n, emit)?.is_some() {
                        listener.events.push(format!("transposed the piece by {n} semitones from bar {} (your written pitches are shifted for you)", tick / bar + 1));
                    }
                }
                Command::HumanState(v) => listener.human_state = Some(v),
                Command::SetKey(_) => {} // a performance cannot start without its key
                Command::Finish => {
                    if let Some(pd) = pending.as_ref() {
                        total = pd.index + 1;
                        let (idx, ps) = (pd.index, pd.planned_start);
                        pending = Some(new_pending(idx, ps, total, &state, &listener, &mut generation)?);
                        emit(json!({"type": "finishing", "finalPhrase": idx}));
                    }
                }
            }
        }

        // 1. Ask.
        if let Some(pd) = pending.as_mut() {
            if !pd.in_flight && pd.ready.is_none() {
                emit(json!({"type": "phrase_request", "phrase": pd.index, "attempt": pd.attempts,
                            "plannedStartTick": pd.planned_start, "final": pd.index + 1 == total}));
                let mut worker = llm.fork();
                let (tx, msgs, gen) = (reply_tx.clone(), pd.messages.clone(), pd.generation);
                std::thread::spawn(move || {
                    let _ = tx.send((gen, worker.chat(&msgs)));
                });
                pd.in_flight = true;
            }
        }

        // 2. Receive (batch mode waits; realtime polls so audio keeps flowing).
        let waiting = pending.as_ref().is_some_and(|pd| pd.in_flight);
        let got = if waiting {
            let wait = if cfg.realtime && any_committed { Duration::from_millis(30) } else { Duration::from_millis(100) };
            reply_rx.recv_timeout(wait).ok()
        } else {
            None
        };
        if let Some((gen, reply)) = got {
            let pd = pending.as_mut().filter(|pd| pd.generation == gen && pd.in_flight);
            if let Some(pd) = pd {
                pd.in_flight = false;
                pd.attempts += 1;
                match reply {
                    Err(e) => emit(json!({"type": "llm_error", "phrase": pd.index, "attempt": pd.attempts - 1, "message": e})),
                    Ok(r) => {
                        let raw_dir = root.join("llm");
                        let _ = std::fs::create_dir_all(&raw_dir);
                        let _ = std::fs::write(raw_dir.join(format!("p{:03}-g{gen}-a{}.txt", pd.index, pd.attempts - 1)), &r.content);
                        emit(json!({"type": "llm_reply", "phrase": pd.index, "attempt": pd.attempts - 1, "model": r.model, "elapsedMs": r.elapsed_ms as u64}));
                        match extract_json(&r.content).and_then(|v| prepare_ops(&v, 0).map(|_| v)) {
                            Ok(v) => {
                                let said = v["reply"].as_str().unwrap_or("").trim().to_string();
                                if !said.is_empty() {
                                    emit(json!({"type": "chat", "who": "ai", "text": said, "phrase": pd.index}));
                                    state.dialogue.push(DialogueLine { who: "ai".into(), text: said, tick: rendered_until });
                                    listener.unanswered.clear();
                                }
                                pd.ready = Some((v, r.model));
                            }
                            Err(e) => {
                                emit(json!({"type": "txn_rejected", "phrase": pd.index, "code": "BAD_REPLY", "message": e}));
                                pd.messages.push(Msg::assistant(r.content));
                                pd.messages.push(Msg::user(format!("That reply could not be used: {e}. Resend the full JSON object only.")));
                            }
                        }
                    }
                }
            }
        }

        // 3. Commit when the phrase can start.
        let commit_now = pending.as_ref().is_some_and(|pd| {
            pd.ready.is_some()
                && (!cfg.realtime || !any_committed || pd.urgent || ceil_to_grid(rendered_until, 0, bar) >= pd.planned_start)
        });
        if commit_now {
            let pd = pending.as_mut().unwrap();
            let (v, model) = pd.ready.take().unwrap();
            let earliest = ceil_to_grid(rendered_until, 0, bar);
            let start = if pd.urgent { earliest } else { pd.planned_start.max(earliest) };
            let bars = v.get("bars").and_then(|b| b.as_u64()).map_or(cfg.default_bars, |b| b as u32).clamp(MIN_BARS, MAX_BARS);
            let title = v["title"].as_str().unwrap_or("").to_string();
            let commentary = v["commentary"].as_str().unwrap_or("").to_string();
            let ops = prepare_ops(&v, start)?;
            set_transport(&store, start).map_err(|e| e.to_string())?;
            let cur = store.load().map_err(|e| e.to_string())?.revision;
            let result = if ops.is_empty() {
                Ok(cur)
            } else {
                let txn = json!({
                    "commandId": format!("perform-p{}-g{}-a{}-r{cur}", pd.index, pd.generation, pd.attempts),
                    "actor": "agent.performer", "ifMatchRevision": cur,
                    "quantize": {"type": "tick", "tick": start}, "latePolicy": "reject", "ops": ops,
                });
                apply(&store, &txn.to_string(), ApplyOptions::default(), &mut NoRender)
                    .map(|r| r.revision)
                    .map_err(|e| serde_json::to_value(&e).unwrap()["error"].clone())
            };
            match result {
                Ok(revision) => {
                    let late_bars = if pd.urgent { 0 } else { (start - pd.planned_start) / bar };
                    emit(json!({"type": "change", "phrase": pd.index, "tick": start, "bars": bars, "title": title,
                                "commentary": commentary, "revision": revision, "model": model, "lateBars": late_bars,
                                "opCount": ops.len(), "answer": pd.urgent, "final": pd.index + 1 == total}));
                    any_committed = true;
                    state.phrases.push(PhraseRecord { index: pd.index, title, commentary, start_tick: start, bars, applied: true, model: Some(model), revision });
                    save_state(root, &state)?;
                    let next_start = start + bars as u64 * bar;
                    if pd.index + 1 < total {
                        pending = Some(new_pending(pd.index + 1, next_start, total, &state, &listener, &mut generation)?);
                    } else {
                        final_end = Some(next_start);
                        pending = None;
                    }
                }
                Err(err) => {
                    emit(json!({"type": "txn_rejected", "phrase": pd.index, "code": err["code"], "message": err["message"]}));
                    pd.messages.push(Msg::assistant(v.to_string()));
                    pd.messages.push(Msg::user(format!(
                        "The engine rejected the transaction (nothing was applied): {err}\nFix the problem and resend the complete JSON object."
                    )));
                }
            }
        }

        // Give up on a phrase after too many attempts: keep looping, move on.
        if let Some(pd) = pending.as_ref() {
            if !pd.in_flight && pd.ready.is_none() && pd.attempts > cfg.max_retries {
                let (idx, ps) = (pd.index, pd.planned_start.max(ceil_to_grid(rendered_until, 0, bar)));
                emit(json!({"type": "phrase_skipped", "phrase": idx, "message": "no valid reply; the current score keeps looping"}));
                state.phrases.push(PhraseRecord { index: idx, title: "(loop continues)".into(), start_tick: ps, bars: cfg.default_bars, ..Default::default() });
                save_state(root, &state)?;
                if !any_committed {
                    return Err("the AI produced no usable opening phrase".into());
                }
                let next_start = ps + cfg.default_bars as u64 * bar;
                if idx + 1 < total {
                    pending = Some(new_pending(idx + 1, next_start, total, &state, &listener, &mut generation)?);
                } else {
                    final_end = Some(next_start);
                    pending = None;
                }
            }
        }

        // 4. Render bars.
        if any_committed {
            let p = store.load().map_err(|e| e.to_string())?;
            let tmap = agb_engine::state::tempo_map(&p);
            let target = match (cfg.realtime, final_end) {
                (true, fe) => {
                    let (t0, s0) = *started_at.get_or_insert((Instant::now(), tmap.tick_to_sample(rendered_until) as f64));
                    let heard = tmap.sample_to_tick(s0 + t0.elapsed().as_secs_f64() * p.sample_rate as f64);
                    let mut t = heard as u64 + cfg.lookahead_bars as u64 * bar;
                    // A ready phrase must land exactly on its planned bar line.
                    if let Some(pd) = pending.as_ref().filter(|pd| pd.ready.is_some() && !pd.urgent) {
                        t = t.min(pd.planned_start.max(rendered_until));
                    }
                    fe.map_or(t, |f| t.min(f))
                }
                (false, Some(f)) => f,
                (false, None) => pending.as_ref().map_or(rendered_until, |pd| pd.planned_start),
            };
            while rendered_until < target {
                let to = ceil_to_grid(rendered_until + 1, 0, bar);
                let is_last = final_end == Some(to);
                emit_chunk(&store, &out_dir, &mut state, rendered_until, to, is_last, emit)?;
                rendered_until = to;
                state.rendered_until = to;
                save_state(root, &state)?;
            }
        }

        if let Some(f) = final_end {
            if rendered_until >= f {
                break;
            }
        }
        if cfg.realtime && !waiting {
            std::thread::sleep(Duration::from_millis(30));
        }
    }

    // Full-piece render with stems and sidecars.
    let p = store.load().map_err(|e| e.to_string())?;
    let req = RenderRequest { from_tick: 0, to_tick: rendered_until, tail_ms: FINAL_TAIL_MS, seed: state.seed, stems: true, format: SampleFormat::Pcm16 };
    let full_dir = root.join("renders").join("full");
    let summary = render_to_dir(&p, &req, &full_dir, "mix.wav").map_err(|e| e.message)?;
    save_state(root, &state)?;
    emit(json!({"type": "done", "phrases": state.phrases.len(), "fullWav": full_dir.join("mix.wav"), "mixSha256": summary["mixSha256"],
                "durationSec": summary["totalSamples"].as_f64().unwrap_or(0.0) / p.sample_rate as f64}));
    Ok(state)
}

pub(crate) fn emit_chunk(
    store: &ProjectStore,
    out_dir: &Path,
    state: &mut PerformState,
    from: u64,
    to: u64,
    last: bool,
    emit: &mut dyn FnMut(Value),
) -> Result<(), String> {
    let p = store.load().map_err(|e| e.to_string())?;
    let tmap = agb_engine::state::tempo_map(&p);
    let req = RenderRequest {
        from_tick: 0,
        to_tick: to,
        tail_ms: if last { FINAL_TAIL_MS } else { 0 },
        seed: state.seed,
        stems: true,
        format: SampleFormat::F32,
    };
    // Only the end of the piece is worked out again: from a few seconds before this bar, or from the
    // start of the earliest note still held. (Rendering the whole piece for every bar took longer
    // than a bar lasts once the piece was a minute or two long, and playback fell behind.)
    let r = render_tail(&p, &req, from).map_err(|e| e.to_string())?;
    let s0 = tmap.tick_to_sample(from) as usize;
    let skip = s0 - tmap.tick_to_sample(r.start_tick) as usize; // where this bar begins in the buffers
    let n = state.chunks.len();
    let chunk = &r.mix[skip.min(r.mix.len())..];
    let name = format!("chunk_{n:05}.wav");
    let path = out_dir.join(&name);
    std::fs::write(&path, encode_wav(chunk, p.sample_rate, SampleFormat::F32)).map_err(|e| e.to_string())?;
    let mut stems = vec![];
    for (id, buf) in &r.stems {
        let sp = out_dir.join(format!("chunk_{n:05}_{id}.wav"));
        std::fs::write(&sp, encode_wav(&buf[skip.min(buf.len())..], p.sample_rate, SampleFormat::F32)).map_err(|e| e.to_string())?;
        stems.push(json!({"trackId": id, "wav": sp}));
    }
    state.chunks.push(format!("renders/perform/{name}"));
    let events: Vec<Value> = r
        .schedule
        .events
        .iter()
        .filter(|e| e.tick >= from && e.tick < to)
        .map(|e| json!([e.tick, e.track_id, e.layer_id, e.pitch, e.velocity, e.off_tick - e.tick]))
        .collect();
    emit(json!({
        "type": "chunk", "wav": path, "stems": stems, "startTick": from, "endTick": to, "startSample": s0,
        "samples": chunk.len(), "sampleRate": p.sample_rate, "tempoBpm": tmap.bpm_at(from), "last": last,
        "tempoMap": tmap.segments().iter().map(|(t, s, b)| json!([t, s, b])).collect::<Vec<_>>(),
        // [tick, semitones]: transposition in force from each tick on.
        "transposeMap": std::iter::once(json!([0, p.transpose]))
            .chain(p.transpose_map.iter().map(|t| json!([t.tick, t.semitones]))).collect::<Vec<_>>(),
        "ticksPerStep": agb_engine::clock::ticks_per_step(p.ppqn, p.steps_per_quarter).unwrap_or(240),
        "tracks": agb_score::timeline::snapshot_at(&p.timeline, from).tracks.iter().map(|t| json!({
            "trackId": t.id, "engineId": t.engine_id, "presetId": t.preset_id, "mute": t.mute,
            "layers": t.layers.iter().map(|l| json!({
                "layerId": l.id, "lengthSteps": l.length_steps, "phaseOffsetSteps": l.phase_offset_steps,
                "mute": l.mute || t.mute,
                "noteSteps": l.notes.as_ref().filter(|n| !n.is_empty()).map(|n| n.iter().map(|x| x.step).collect::<Vec<_>>())
                    .unwrap_or_else(|| l.steps.as_ref().map(|s| s.iter().enumerate().filter(|(_, v)| **v == 1).map(|(i, _)| i as u32).collect()).unwrap_or_default()),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "events": events,
        "peakDbfs": r.metrics.mix.peak_dbfs,
    }));
    Ok(())
}
