//! `.agbsong` acceptance tests (FORMAT.md) at the CLI-library level.

use agb_cli::{song_pack, song_render, song_unpack, song_validate, SongRenderArgs};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn example() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-light.src").canonicalize().unwrap()
}

fn args(out_dir: PathBuf) -> SongRenderArgs {
    SongRenderArgs {
        out_dir,
        transpose: 0,
        bpm: None,
        from_tick: None,
        to_tick: None,
        seed: None,
        stems: true,
        tail_ms: Some(500),
        format: agb_protocol::SampleFormat::Pcm16,
    }
}

/// Write a song directory from a JSON value.
fn song_dir(root: &Path, song: &Value) -> PathBuf {
    let dir = root.join("t.src");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("song.json"), serde_json::to_vec(song).unwrap()).unwrap();
    dir
}

fn codes(v: &Value, key: &str) -> Vec<String> {
    v[key].as_array().unwrap().iter().map(|i| i["code"].as_str().unwrap().to_string()).collect()
}

fn samples(path: &Path) -> Vec<i16> {
    let bytes = std::fs::read(path).unwrap();
    bytes[44..].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect()
}

fn peak(s: &[i16]) -> i32 {
    s.iter().map(|x| (*x as i32).abs()).max().unwrap_or(0)
}

#[test]
fn example_is_valid_and_reports_what_it_skips() {
    let v = song_validate(&example()).unwrap();
    assert_eq!(v["valid"], json!(true), "{v}");
    assert!(v["warnings"].as_array().unwrap().is_empty(), "{}", v["warnings"]);
    assert_eq!(v["player"]["playable"], json!(true));
    let skipped = codes(&v["player"], "skipped");
    for c in ["PATCH_FALLBACK", "EXPRESSION", "FX", "OVERLAY", "TUNING"] {
        assert!(skipped.iter().any(|s| s == c), "expected {c} in {skipped:?}");
    }
}

#[test]
fn template_is_valid() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("new.src");
    agb_cli::song_new(&dir, "t").unwrap();
    let v = song_validate(&dir).unwrap();
    assert_eq!(v["valid"], json!(true), "{v}");
    assert!(v["player"]["skipped"].as_array().unwrap().is_empty());
}

/// Directory and zip render to the same PCM; packing is reproducible.
#[test]
fn pack_roundtrip_and_deterministic_render() {
    let d = tempfile::tempdir().unwrap();
    let zip = d.path().join("fl.agbsong");
    let a = song_pack(&example(), &zip).unwrap();
    assert_eq!(a["files"][0], json!("mimetype"));
    let zip2 = d.path().join("fl2.agbsong");
    let b = song_pack(&example(), &zip2).unwrap();
    assert_eq!(a["sha256"], b["sha256"], "packing must be reproducible");

    // mimetype is the first entry, stored uncompressed (EPUB-style sniffing).
    let raw = std::fs::read(&zip).unwrap();
    assert_eq!(&raw[30..38], b"mimetype");
    assert_eq!(&raw[38..38 + agb_song::MIMETYPE.len()], agb_song::MIMETYPE.as_bytes());

    assert_eq!(song_validate(&zip).unwrap()["valid"], json!(true));
    let from_dir = song_render(&example(), &args(d.path().join("r1"))).unwrap();
    let from_zip = song_render(&zip, &args(d.path().join("r2"))).unwrap();
    assert_eq!(from_dir["mixSha256"], from_zip["mixSha256"]);
    assert_eq!(from_dir["nearSilent"], json!(false));

    let unpacked = d.path().join("unpacked");
    song_unpack(&zip, &unpacked).unwrap();
    assert_eq!(
        std::fs::read(unpacked.join("song.json")).unwrap(),
        std::fs::read(example().join("song.json")).unwrap()
    );
    assert!(song_unpack(&zip, &unpacked).is_err(), "must not unpack over a non-empty directory");
}

/// The `.agbsong` files committed next to the sources are exactly what `pack` produces,
/// and a source folder may not pose as a song file.
#[test]
fn committed_songs_match_their_sources() {
    let d = tempfile::tempdir().unwrap();
    let examples = example().parent().unwrap().to_path_buf();
    for name in ["first-light", "when-the-mist-lifts"] {
        let packed = examples.join(format!("{name}.agbsong"));
        assert!(packed.is_file(), "{} must be a single file", packed.display());
        let fresh = song_pack(&examples.join(format!("{name}.src")), &d.path().join(format!("{name}.agbsong"))).unwrap();
        assert_eq!(
            fresh["sha256"].as_str().unwrap(),
            agb_assets::sha256_file(&packed).unwrap(),
            "examples/{name}.agbsong is stale: run `agb song pack examples/{name}.src --out examples/{name}.agbsong`"
        );
        assert_eq!(song_validate(&packed).unwrap()["valid"], json!(true));
    }
    assert!(agb_cli::song_new(&d.path().join("x.agbsong"), "x").is_err(), "a folder must not be named .agbsong");
    assert!(song_pack(&example(), &d.path().join("x.zip")).is_err(), "a packed song must be named .agbsong");
}

/// Loops gated by fromTick / toTick only sound inside their window.
#[test]
fn loop_gating() {
    let d = tempfile::tempdir().unwrap();
    song_render(&example(), &args(d.path().join("r"))).unwrap();
    let drums = samples(&d.path().join("r/stems/drums.wav"));
    // 84 BPM, ppqn 960, 48 kHz: tick 15360 (bar 5) = 16 beats.
    let bar5 = (16.0 * 60.0 / 84.0 * 48000.0) as usize;
    assert!(peak(&drums[..bar5 - 100]) == 0, "drums must be silent before fromTick");
    assert!(peak(&drums[bar5..bar5 * 2]) > 1000, "drums must sound after fromTick");
    // toTick 46080 = 3 × bar 5; allow two seconds for tails.
    assert!(peak(&drums[bar5 * 3 + 96000..]) < 50, "drums must stop after toTick");
}

/// Play-time changes: the file is the same, the audio is not.
#[test]
fn transpose_and_tempo_at_play_time() {
    let d = tempfile::tempdir().unwrap();
    let base = song_render(&example(), &args(d.path().join("a"))).unwrap();
    let up = song_render(&example(), &SongRenderArgs { transpose: 3, ..args(d.path().join("b")) }).unwrap();
    assert_ne!(base["mixSha256"], up["mixSha256"]);
    let sha = |dir: &str, stem: &str| agb_assets::sha256_file(&d.path().join(dir).join("stems").join(stem)).unwrap();
    assert_eq!(sha("a", "drums.wav"), sha("b", "drums.wav"), "drums are not transposable");
    assert_ne!(sha("a", "bass.wav"), sha("b", "bass.wav"));

    let fast = song_render(&example(), &SongRenderArgs { bpm: Some(168.0), ..args(d.path().join("c")) }).unwrap();
    let (n, m) = (base["totalSamples"].as_u64().unwrap(), fast["totalSamples"].as_u64().unwrap());
    let tail = 24000; // 500 ms
    assert!(((n - tail) as i64 - 2 * (m - tail) as i64).abs() <= 2, "double tempo halves the length: {n} vs {m}");
}

#[test]
fn rejects_broken_files() {
    let d = tempfile::tempdir().unwrap();
    let mut song: Value = serde_json::from_slice(&std::fs::read(example().join("song.json")).unwrap()).unwrap();
    song["patches"]["lead"] = json!({ "engine": "synth.wavetable", "wavetable": "wt_dawn" });

    // Asset declared but missing from the package.
    let dir = song_dir(d.path(), &song);
    let v = song_validate(&dir).unwrap();
    assert_eq!(v["ok"], json!(false));
    assert!(codes(&v, "errors").contains(&"ASSET_MISSING".to_string()), "{v}");

    // Tampered asset.
    std::fs::create_dir_all(dir.join("wavetables")).unwrap();
    std::fs::write(dir.join("wavetables/dawn.wav"), b"not the file").unwrap();
    std::fs::copy(example().join("cover.svg"), dir.join("cover.svg")).unwrap();
    let v = song_validate(&dir).unwrap();
    assert_eq!(codes(&v, "errors"), vec!["ASSET_HASH_MISMATCH"], "{v}");

    // Paths may not leave the package.
    song["assets"]["wt_dawn"]["path"] = json!("../../etc/passwd");
    let v = song_validate(&song_dir(d.path(), &song)).unwrap();
    assert!(codes(&v, "errors").contains(&"UNSAFE_PATH".to_string()), "{v}");

    // $ref may not leave the package either.
    song["patches"]["lead"] = json!({ "$ref": "../secrets.json" });
    let e = song_validate(&song_dir(d.path(), &song)).unwrap_err();
    assert_eq!(e.error.code, agb_protocol::ErrorCode::SchemaViolation);

    // A newer incompatible format line is refused.
    let v = song_validate(&song_dir(d.path(), &json!({
        "format": "agbsong", "formatVersion": "0.2.0", "meta": { "title": "x" }, "tracks": []
    })))
    .unwrap();
    assert_eq!(codes(&v, "errors"), vec!["FORMAT_VERSION"]);
}

/// `requires` is a refusal, `uses` degrades; unknown fields are ignored.
#[test]
fn feature_negotiation() {
    let d = tempfile::tempdir().unwrap();
    let mut song = agb_song::template("t");
    song["futureField"] = json!({ "anything": [1, 2, 3] });
    song["tracks"][0]["x-vendor.thing"] = json!(true);
    song["patches"]["kit"] = json!({ "engine": "x-acme.physical-drums", "params": { "tension": 0.4 } });
    song["uses"] = json!(["x-acme.drums"]);

    // No fallback: the track is skipped and reported, the rest plays.
    let dir = song_dir(d.path(), &song);
    let v = song_validate(&dir).unwrap();
    assert_eq!(v["valid"], json!(true), "{v}");
    assert_eq!(codes(&v["player"], "skipped"), vec!["PATCH_UNPLAYABLE"]);
    let r = song_render(&dir, &args(d.path().join("r"))).unwrap();
    assert_eq!(r["files"].as_array().unwrap().iter().filter(|f| f.as_str().unwrap().starts_with("stems/")).count(), 1);

    // Required but unsupported: valid file, not playable here.
    song["requires"] = json!(["core", "sampler"]);
    let dir = song_dir(d.path(), &song);
    let v = song_validate(&dir).unwrap();
    assert_eq!(v["valid"], json!(true));
    assert_eq!(v["player"]["playable"], json!(false));
    let e = song_render(&dir, &args(d.path().join("r2"))).unwrap_err();
    assert_eq!(e.error.code, agb_protocol::ErrorCode::UnsupportedInPhase);
}

/// FORMAT.md §15 minimal example parses, validates and renders.
#[test]
fn spec_minimal_example() {
    let spec = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../FORMAT.md")).unwrap();
    let section = spec.split("## 15. 最小示例").nth(1).unwrap();
    let body = section.split("```json").nth(1).unwrap().split("```").next().unwrap();
    let d = tempfile::tempdir().unwrap();
    let dir = song_dir(d.path(), &serde_json::from_str(body).unwrap());
    let v = song_validate(&dir).unwrap();
    assert_eq!(v["valid"], json!(true), "{v}");
    let r = song_render(&dir, &args(d.path().join("r"))).unwrap();
    assert_eq!(r["nearSilent"], json!(false));
    // 7 and 5 steps of eighth notes: one polymeter cycle = 35 steps.
    assert_eq!(r["lengthTicks"], json!(35 * 480));
}

// ---------------------------------------------------------------------------
// Talking to the performer while a song plays
// ---------------------------------------------------------------------------

/// An LLM that answers with scripted replies, in order.
#[derive(Clone)]
struct Scripted(std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<String>>>);

impl Scripted {
    fn new(replies: &[Value]) -> Self {
        Scripted(std::sync::Arc::new(std::sync::Mutex::new(replies.iter().map(|r| r.to_string()).collect())))
    }
}

impl agb_agent::Llm for Scripted {
    fn fork(&self) -> Box<dyn agb_agent::Llm + Send> {
        Box::new(self.clone())
    }
    fn chat(&mut self, _messages: &[agb_agent::Msg]) -> Result<agb_agent::Reply, String> {
        let content = self.0.lock().unwrap().pop_front().ok_or("script exhausted")?;
        Ok(agb_agent::Reply { content, model: "scripted".into(), elapsed_ms: 0 })
    }
}

/// Template song plus a written melody: 4 bars, one note per bar on `lead`.
fn jam_song() -> Value {
    let mut song = agb_song::template("jam");
    song["time"]["lengthTicks"] = json!(4 * 3840);
    song["patches"]["lead"] = json!({ "engine": "synth.waveform", "params": { "osc.wave": "sine" } });
    song["tracks"].as_array_mut().unwrap().push(json!({
        "id": "lead", "patch": "lead",
        "clips": [{ "atTick": 0, "notes": (0..4).map(|b| json!({ "tick": b * 3840, "pitch": 60 + b, "durTicks": 1920 })).collect::<Vec<_>>() }]
    }));
    song
}

struct Jam {
    events: Vec<Value>,
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn run_jam(song: &Value, allow: Option<Vec<String>>, replies: &[Value], say: Option<&str>) -> Jam {
    let d = tempfile::tempdir().unwrap();
    let parsed: agb_song::Song = serde_json::from_value(song.clone()).unwrap();
    let l = agb_song::lower(&parsed, &agb_song::PlayOptions::default()).unwrap();
    let ctx = agb_agent::SongContext { title: "jam".into(), length_ticks: l.length_ticks, allow, ..Default::default() };
    let root = d.path().join("work");
    let cfg = agb_agent::JamConfig { root: root.clone(), seed: 0, max_retries: 2, realtime: false };
    let (tx, rx) = std::sync::mpsc::channel();
    if let Some(text) = say {
        tx.send(agb_agent::Command::Chat(text.into())).unwrap();
    }
    let mut events = vec![];
    agb_agent::jam(&cfg, l.project, &ctx, Some(Box::new(Scripted::new(replies))), None, rx, &mut |e| events.push(e)).unwrap();
    Jam { events, root, _dir: d }
}

fn of_type<'a>(events: &'a [Value], ty: &str) -> Vec<&'a Value> {
    events.iter().filter(|e| e["type"] == ty).collect()
}

/// Notes `[tick, pitch]` of one track across all chunks.
fn played(events: &[Value], track: &str) -> Vec<(u64, u64)> {
    of_type(events, "chunk")
        .iter()
        .flat_map(|c| c["events"].as_array().unwrap().iter())
        .filter(|e| e[1] == track)
        .map(|e| (e[0].as_u64().unwrap(), e[3].as_u64().unwrap()))
        .collect()
}

/// Nobody speaks: the song plays exactly as `song render` renders it.
#[test]
fn jam_without_chat_is_the_song() {
    let j = run_jam(&jam_song(), None, &[], None);
    assert_eq!(of_type(&j.events, "chunk").len(), 4);
    assert_eq!(played(&j.events, "lead"), vec![(0, 60), (3840, 61), (7680, 62), (11520, 63)]);
    let done = of_type(&j.events, "done")[0];
    assert_eq!(done["changes"], json!(0));

    let d = tempfile::tempdir().unwrap();
    let dir = song_dir(d.path(), &jam_song());
    let r = song_render(&dir, &SongRenderArgs { tail_ms: Some(2500), stems: false, ..args(d.path().join("r")) }).unwrap();
    assert_eq!(done["mixSha256"], r["mixSha256"]);
}

/// The listener asks; the performer answers, rewrites the written melody and mutes the drums.
#[test]
fn jam_chat_changes_the_performance() {
    let reply = json!({
        "reply": "好，换个旋律，鼓先停。", "title": "换旋律", "commentary": "新的高音旋律，鼓静音",
        "ops": [
            { "op": "clip.clear", "trackId": "lead" },
            { "op": "clip.schedule", "trackId": "lead", "notes": [{ "tick": 960, "pitch": 84, "durTicks": 960, "velocity": 0.7 }] },
            { "op": "track.upsert", "trackId": "drums", "mute": true }
        ]
    });
    let j = run_jam(&jam_song(), None, &[reply], Some("换个旋律，鼓停一下"));
    let chats = of_type(&j.events, "chat");
    assert_eq!(chats[0]["who"], "listener");
    assert_eq!(chats[1]["who"], "ai");
    assert_eq!(chats[1]["text"], "好，换个旋律，鼓先停。");
    let answer = j.events.iter().find(|e| e["type"] == "change" && e["answer"] == true).expect("an answer change");
    assert_eq!(answer["title"], "换旋律");
    // The written melody is gone from the commit tick on; the new note is there.
    assert_eq!(played(&j.events, "lead"), vec![(960, 84)]);
    assert!(played(&j.events, "drums").is_empty(), "drums were muted");
    assert!(!played(&j.events, "bass").is_empty(), "the rest keeps playing");

    // The take records the transaction and the conversation; the song itself was never written to.
    let take: Value = serde_json::from_slice(&std::fs::read(j.root.join("take.json")).unwrap()).unwrap();
    // One answer = two transactions: the written notes, then the mix change.
    assert_eq!(take["transactions"].as_array().unwrap().len(), 2);
    assert_eq!(take["dialogue"].as_array().unwrap().len(), 2);
}

/// `clip.clear` only removes notes inside its window, and only earlier revisions' notes.
#[test]
fn jam_clip_clear_window() {
    let reply = json!({ "reply": "", "title": "留白", "ops": [{ "op": "clip.clear", "trackId": "lead", "toTick": 7680 }] });
    let j = run_jam(&jam_song(), None, &[reply], Some("前两小节旋律空出来"));
    assert_eq!(played(&j.events, "lead"), vec![(7680, 62), (11520, 63)]);
}

/// The file's `allow` list is enforced by the player, not left to the model.
#[test]
fn jam_respects_allow() {
    let forbidden = json!({ "reply": "换了", "title": "x", "ops": [{ "op": "clip.clear", "trackId": "lead" }] });
    let polite = json!({ "reply": "这首曲子的作者不让改音符，我把鼓收小一点。", "title": "鼓收小",
                         "ops": [{ "op": "track.upsert", "trackId": "drums", "gainDb": -20 }] });
    let j = run_jam(&jam_song(), Some(vec!["tempo".into()]), &[forbidden, polite], Some("把旋律删掉"));
    let rejected = of_type(&j.events, "txn_rejected");
    assert_eq!(rejected.len(), 1);
    assert!(rejected[0]["message"].as_str().unwrap().contains("does not allow"), "{}", rejected[0]);
    assert_eq!(played(&j.events, "lead").len(), 4, "the melody is untouched");
    let chats = of_type(&j.events, "chat");
    assert_eq!(chats.last().unwrap()["text"], "这首曲子的作者不让改音符，我把鼓收小一点。");
}

/// A model that never produces a usable reply does not stop the music.
#[test]
fn jam_survives_a_useless_model() {
    let junk = json!({ "reply": "x", "ops": [{ "op": "layer.upsert", "trackId": "ghost", "layerId": "l" }] });
    let j = run_jam(&jam_song(), None, &[junk.clone(), junk.clone(), junk], Some("加点东西"));
    assert_eq!(of_type(&j.events, "txn_rejected").len(), 3);
    assert_eq!(of_type(&j.events, "chunk").len(), 4);
    assert!(of_type(&j.events, "chat").last().unwrap()["text"].as_str().unwrap().contains("没改成"));
    assert_eq!(played(&j.events, "lead").len(), 4);
}

/// The listener transposes while the song plays: pitched tracks move from the next bar, drums stay.
#[test]
fn jam_listener_transposes_live() {
    let d = tempfile::tempdir().unwrap();
    let parsed: agb_song::Song = serde_json::from_value(jam_song()).unwrap();
    let l = agb_song::lower(&parsed, &agb_song::PlayOptions::default()).unwrap();
    let ctx = agb_agent::SongContext { title: "jam".into(), length_ticks: l.length_ticks, ..Default::default() };
    let cfg = agb_agent::JamConfig { root: d.path().join("w"), seed: 0, max_retries: 0, realtime: false };
    let (tx, rx) = std::sync::mpsc::channel();
    let mut events: Vec<Value> = vec![];
    let mut sent = false;
    // Batch mode renders one bar per loop turn: send the command once the first bar is out.
    agb_agent::jam(&cfg, l.project, &ctx, None, None, rx, &mut |e| {
        if e["type"] == "chunk" && !sent {
            sent = true;
            tx.send(agb_agent::Command::SetTranspose(3)).unwrap();
        }
        events.push(e);
    })
    .unwrap();
    assert_eq!(played(&events, "lead"), vec![(0, 60), (3840, 64), (7680, 65), (11520, 66)]);
    let kicks: Vec<u64> = played(&events, "drums").iter().map(|n| n.1).collect();
    assert!(kicks.iter().all(|p| *p == 36), "drums are not transposed: {kicks:?}");
    let change = events.iter().find(|e| e["type"] == "human_change").unwrap();
    assert_eq!((change["kind"].as_str(), change["tick"].as_u64()), (Some("transpose"), Some(3840)));
    let last = of_type(&events, "chunk").last().unwrap()["transposeMap"].clone();
    assert_eq!(last, json!([[0, 0], [3840, 3]]));
}

/// The key arrives after playback started: the next message is answered, no reopen needed.
#[test]
fn jam_key_can_arrive_later() {
    let d = tempfile::tempdir().unwrap();
    let parsed: agb_song::Song = serde_json::from_value(jam_song()).unwrap();
    let l = agb_song::lower(&parsed, &agb_song::PlayOptions::default()).unwrap();
    let ctx = agb_agent::SongContext { title: "jam".into(), length_ticks: l.length_ticks, ..Default::default() };
    let cfg = agb_agent::JamConfig { root: d.path().join("w"), seed: 0, max_retries: 0, realtime: false };
    let reply = json!({ "reply": "好的。", "title": "鼓收小", "ops": [{ "op": "track.upsert", "trackId": "drums", "gainDb": -20 }] });
    let connect = |key: &str| -> Box<dyn agb_agent::Llm + Send> {
        assert_eq!(key, "k-123");
        Box::new(Scripted::new(&[reply.clone()]))
    };
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(agb_agent::Command::Chat("安静点".into())).unwrap(); // no key yet
    tx.send(agb_agent::Command::SetKey("k-123".into())).unwrap();
    tx.send(agb_agent::Command::Chat("安静点".into())).unwrap();
    let mut events = vec![];
    agb_agent::jam(&cfg, l.project, &ctx, None, Some(&connect), rx, &mut |e| events.push(e)).unwrap();
    let ai: Vec<&str> = of_type(&events, "chat").iter().filter(|c| c["who"] == "ai").map(|c| c["text"].as_str().unwrap()).collect();
    assert!(ai[0].contains("缺少 API Key"), "{ai:?}");
    assert_eq!(ai[1], "好的。");
    assert!(events.iter().any(|e| e["type"] == "change" && e["answer"] == true));
}

/// A song video must be 16:9 or 4:3; the size is read from the file itself.
#[test]
fn video_aspect_is_checked() {
    let src = example().parent().unwrap().join("when-the-mist-lifts.src");
    let bytes = std::fs::read(src.join("visuals/mist.mp4")).unwrap();
    assert_eq!(agb_song::package::video_dimensions(&bytes), Some((1280, 720)));
    assert!(agb_song::package::video_aspect_ok(1280, 720) && agb_song::package::video_aspect_ok(1440, 1080));
    assert!(!agb_song::package::video_aspect_ok(1080, 1080) && !agb_song::package::video_aspect_ok(1080, 1920));

    // Patch the stored size to a square: the validator must refuse the file.
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("sq.src");
    std::fs::create_dir_all(dir.join("visuals")).unwrap();
    let at = bytes.windows(4).position(|w| w == b"tkhd").unwrap();
    let end = at - 4 + u32::from_be_bytes(bytes[at - 4..at].try_into().unwrap()) as usize;
    let mut square = bytes.clone();
    square[end - 8..end - 4].copy_from_slice(&(720u32 << 16).to_be_bytes());
    std::fs::write(dir.join("visuals/mist.mp4"), &square).unwrap();
    std::fs::copy(src.join("cover.svg"), dir.join("cover.svg")).unwrap();
    let mut song: Value = serde_json::from_slice(&std::fs::read(src.join("song.json")).unwrap()).unwrap();
    song["assets"]["video"]["sha256"] = json!(agb_assets::sha256_bytes(&square));
    std::fs::write(dir.join("song.json"), serde_json::to_vec(&song).unwrap()).unwrap();
    let v = song_validate(&dir).unwrap();
    assert_eq!(codes(&v, "errors"), vec!["VIDEO_ASPECT"], "{v}");
}
