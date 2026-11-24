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

/// Every song under `songs/` meets the format bar of AUTHORING.md §8 and matches its source.
#[test]
fn committed_repertoire_meets_the_authoring_standard() {
    let songs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../songs");
    let mut seen = 0;
    for entry in std::fs::read_dir(&songs).unwrap() {
        let packed = entry.unwrap().path();
        if packed.extension().and_then(|e| e.to_str()) != Some("agbsong") {
            continue;
        }
        seen += 1;
        let slug = packed.file_stem().unwrap().to_str().unwrap().to_string();
        assert!(packed.is_file(), "{} must be a single file", packed.display());
        let v = song_validate(&packed).unwrap();
        assert_eq!(v["valid"], json!(true), "{slug}: {v}");
        assert!(v["warnings"].as_array().unwrap().is_empty(), "{slug}: {}", v["warnings"]);
        assert_eq!(v["player"]["playable"], json!(true), "{slug}");
        assert!(v["player"]["skipped"].as_array().unwrap().is_empty(), "{slug}: {}", v["player"]["skipped"]);
        let inside = agb_song::package::Package::open(&packed).unwrap().read("song.json").unwrap();
        let source = std::fs::read(songs.join(&slug).join(format!("{slug}.src/song.json"))).unwrap();
        assert!(inside == source, "songs/{slug}.agbsong is stale: regenerate and pack songs/{slug}/{slug}.src");
        let song: serde_json::Value = serde_json::from_slice(&inside).unwrap();
        for field in ["/meta/cover", "/tuning/key", "/time/lengthTicks", "/performance/agent/brief"] {
            assert!(song.pointer(field).is_some_and(|x| !x.is_null()), "{slug}: {field} is required");
        }
        let (mut at, sections) = (0, song["sections"].as_array().unwrap());
        for s in sections {
            assert_eq!(s["fromTick"].as_u64().unwrap(), at, "{slug}: sections must be contiguous");
            at = s["toTick"].as_u64().unwrap();
        }
        assert_eq!(at, song["time"]["lengthTicks"].as_u64().unwrap(), "{slug}: sections must cover the whole song");
    }
    assert!(seen > 0, "no songs found in {}", songs.display());
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
    song["requires"] = json!(["core", "wavetable"]);
    let dir = song_dir(d.path(), &song);
    let v = song_validate(&dir).unwrap();
    assert_eq!(v["valid"], json!(true));
    assert_eq!(v["player"]["playable"], json!(false));
    let e = song_render(&dir, &args(d.path().join("r2"))).unwrap_err();
    assert_eq!(e.error.code, agb_protocol::ErrorCode::UnsupportedInPhase);
}

/// A 16-bit mono WAV of a sine, for sampler tests.
fn write_sine(path: &Path, frames: usize, hz: f64) {
    let mut b = Vec::with_capacity(44 + frames * 2);
    let data = (frames * 2) as u32;
    b.extend(b"RIFF"); b.extend((36 + data).to_le_bytes()); b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes()); b.extend(1u16.to_le_bytes()); b.extend(1u16.to_le_bytes());
    b.extend(48000u32.to_le_bytes()); b.extend(96000u32.to_le_bytes()); b.extend(2u16.to_le_bytes()); b.extend(16u16.to_le_bytes());
    b.extend(b"data"); b.extend(data.to_le_bytes());
    for i in 0..frames {
        let v = (i as f64 * hz / 48000.0 * std::f64::consts::TAU).sin();
        b.extend(((v * 16000.0) as i16).to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}

/// FORMAT.md §8.3: a `sampler.multi` patch plays its samples, from a directory and from a packed file;
/// without the audio file the fallback is used and reported.
#[test]
fn sampler_plays_samples() {
    let d = tempfile::tempdir().unwrap();
    let mut song = agb_song::template("t");
    song["uses"] = json!(["sampler"]);
    song["assets"] = json!({ "tone": { "path": "samples/tone.wav", "kind": "sample" } });
    song["patches"]["smp"] = json!({
        "engine": "sampler.multi",
        "zones": [{ "sample": "tone", "keyRange": [48, 72], "rootPitch": 60, "oneShot": true }],
        "fallback": [{ "preset": "preset:synth.waveform:keys:sine_keys@v1.0" }]
    });
    song["tracks"] = json!([{ "id": "s", "patch": "smp", "clips": [{ "atTick": 0, "lengthTicks": 3840,
        "notes": [{ "tick": 0, "pitch": 60, "durTicks": 240, "velocity": 0.9 }] }] }]);
    song["time"]["lengthTicks"] = json!(3840);
    let dir = song_dir(d.path(), &song);
    std::fs::create_dir_all(dir.join("samples")).unwrap();
    write_sine(&dir.join("samples/tone.wav"), 24000, 440.0);

    let v = song_validate(&dir).unwrap();
    assert_eq!(v["valid"], json!(true), "{v}");
    assert!(codes(&v["player"], "skipped").is_empty(), "{}", v["player"]);
    let r = song_render(&dir, &args(d.path().join("r"))).unwrap();
    assert_eq!(r["nearSilent"], json!(false), "{r}");
    // A one-shot plays the whole half second although the note is a sixteenth long.
    let mix = samples(&d.path().join("r/mix.wav"));
    assert!(peak(&mix[20000..23000]) > 1000, "the sample should still be sounding at 0.45 s");
    assert!(peak(&mix[26000..40000]) < 50, "and be over after 0.5 s");

    // The packed file plays the same.
    let packed = d.path().join("t.agbsong");
    song_pack(&dir, &packed).unwrap();
    let r2 = song_render(&packed, &args(d.path().join("r2"))).unwrap();
    assert_eq!(r2["mixSha256"], r["mixSha256"]);

    // Without the audio file the patch falls back.
    std::fs::remove_file(dir.join("samples/tone.wav")).unwrap();
    let mut no_asset = song.clone();
    no_asset["assets"] = json!({});
    let dir2 = song_dir(&d.path().join("b"), &no_asset);
    let v = song_validate(&dir2).unwrap();
    if v["valid"] == json!(true) {
        assert_eq!(codes(&v["player"], "skipped"), vec!["PATCH_FALLBACK"]);
    }
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

/// A performance saved as a song plays back as the performance did.
#[test]
fn export_reproduces_the_performance() {
    let d = tempfile::tempdir().unwrap();
    let project = d.path().join("perf");
    let code = agb_cli::perform_cmd(agb_cli::PerformArgs {
        project: project.clone(),
        brief: "export test".into(),
        phrases: 3,
        bars: 4,
        realtime: false,
        seed: 7,
        models: vec![],
        base_url: None,
        api_key_file: None,
        max_retries: 1,
        continue_existing: false,
        mock: true,
    });
    assert_eq!(code, 0);
    let out = d.path().join("saved.agbsong");
    let r = agb_cli::song_export(&project, &out, Some("saved")).unwrap();
    assert_eq!(r["ok"], true);
    assert_eq!(r["bars"], 12);
    assert!(r["notes"].as_u64().unwrap() > 0);
    let v = agb_cli::song_validate(&out).unwrap();
    assert_eq!(v["valid"], true, "{v}");

    // Same notes → same audio (up to the length of the tail).
    let render_dir = d.path().join("render");
    agb_cli::song_render(
        &out,
        &agb_cli::SongRenderArgs {
            out_dir: render_dir.clone(),
            transpose: 0,
            bpm: None,
            from_tick: None,
            to_tick: None,
            seed: None,
            stems: false,
            tail_ms: None,
            format: agb_protocol::SampleFormat::Pcm16,
        },
    )
    .unwrap();
    let a = samples(&project.join("renders/full/mix.wav"));
    let b = samples(&render_dir.join("mix.wav"));
    let n = a.len().min(b.len());
    assert!(n > 48000);
    let worst = a[..n].iter().zip(&b[..n]).map(|(x, y)| (*x as i32 - *y as i32).abs()).max().unwrap();
    assert!(worst <= 2, "the exported song differs from the performance by {worst} (of 32768)");
}

fn plugin_song(name: &str) -> Value {
    let mut song = agb_song::template("t");
    song["uses"] = json!(["plugin", "fx"]);
    song["patches"]["keys"] = json!({
        "engine": "plugin.vst3",
        "plugin": { "name": name, "params": { "ALGORITHM": 0.13, "OP2 OUTPUT LEVEL": 0.8 } },
        "fallback": [{ "preset": "preset:synth.waveform:keys:sine_keys@v1.0" }]
    });
    // The room is worked out again from a few seconds before each bar: it must hear what the plug-in
    // played then.
    song["tracks"] = json!([{ "id": "k", "patch": "keys", "gainDb": -6, "fx": [{ "type": "reverb", "params": { "decaySec": 1.5, "mix": 0.3 } }], "clips": [{ "atTick": 0, "lengthTicks": 7680, "notes": [
        { "tick": 0, "pitch": 60, "durTicks": 2400, "velocity": 0.9 },
        { "tick": 1920, "pitch": 64, "durTicks": 2880, "velocity": 0.7 },
        { "tick": 5760, "pitch": 67, "durTicks": 960, "velocity": 0.8 }] }] }]);
    song["time"]["lengthTicks"] = json!(7680);
    song
}

/// FORMAT.md §8.6: a patch played by a plug-in the listener has not installed plays its fallback, and
/// the file says what it needs.
#[test]
fn a_missing_plugin_plays_the_fallback() {
    let d = tempfile::tempdir().unwrap();
    let song = plugin_song("No Such Plug-in 4711");
    let dir = song_dir(d.path(), &song);
    let v = song_validate(&dir).unwrap();
    assert_eq!(v["valid"], json!(true), "{v}");
    assert_eq!(codes(&v["player"], "skipped"), vec!["PATCH_FALLBACK"]);
    let r = song_render(&dir, &args(d.path().join("r"))).unwrap();
    assert_eq!(r["nearSilent"], json!(false), "{r}");

    // Without a fallback the author is warned; out-of-range values and a missing name are errors.
    let mut bare = song.clone();
    bare["patches"]["keys"].as_object_mut().unwrap().remove("fallback");
    let v = song_validate(&song_dir(&d.path().join("b"), &bare)).unwrap();
    assert!(codes(&v, "warnings").contains(&"PLUGIN_NO_FALLBACK".to_string()), "{v}");
    let mut bad = song.clone();
    bad["patches"]["keys"]["plugin"]["params"]["ALGORITHM"] = json!(32);
    let v = song_validate(&song_dir(&d.path().join("c"), &bad)).unwrap();
    assert!(codes(&v, "errors").contains(&"OUT_OF_RANGE".to_string()), "{v}");
    let mut none = song.clone();
    none["patches"]["keys"].as_object_mut().unwrap().remove("plugin");
    let v = song_validate(&song_dir(&d.path().join("e"), &none)).unwrap();
    assert!(codes(&v, "errors").contains(&"PATCH_INCOMPLETE".to_string()), "{v}");
}

/// With the plug-in installed (Dexed, a free FM synthesizer; the test passes without doing anything
/// where it is not) the track is played by it: its parameters change the sound, a preset file carried
/// by the song is loaded, and a piece played bar by bar is the same as the piece rendered whole.
#[test]
fn an_installed_plugin_plays_the_track() {
    if !agb_plugin::available("Dexed", None) {
        eprintln!("Dexed.vst3 is not installed: skipped");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let song = plugin_song("Dexed");
    let dir = song_dir(d.path(), &song);
    let v = song_validate(&dir).unwrap();
    assert_eq!(v["valid"], json!(true), "{v}");
    assert!(codes(&v["player"], "skipped").is_empty(), "{}", v["player"]);
    let r = song_render(&dir, &args(d.path().join("r"))).unwrap();
    assert_eq!(r["nearSilent"], json!(false), "{r}");
    let a = samples(&d.path().join("r/mix.wav"));

    // Not the fallback, and the same when rendered again.
    let fb = song_dir(&d.path().join("fb"), &plugin_song("No Such Plug-in 4711"));
    song_render(&fb, &args(d.path().join("rf"))).unwrap();
    assert_ne!(a, samples(&d.path().join("rf/mix.wav")));
    song_render(&dir, &args(d.path().join("r1"))).unwrap();
    assert_eq!(a, samples(&d.path().join("r1/mix.wav")), "a render starts the plug-in afresh");

    // Another parameter value, another sound; an unknown parameter is reported by name and the fallback plays.
    let mut other = song.clone();
    other["patches"]["keys"]["plugin"]["params"]["OP2 OUTPUT LEVEL"] = json!(0.2);
    song_render(&song_dir(&d.path().join("o"), &other), &args(d.path().join("ro"))).unwrap();
    assert_ne!(a, samples(&d.path().join("ro/mix.wav")));
    let mut wrong = song.clone();
    wrong["patches"]["keys"]["plugin"]["params"]["NO SUCH KNOB"] = json!(0.5);
    let v = song_validate(&song_dir(&d.path().join("w"), &wrong)).unwrap();
    assert_eq!(codes(&v["player"], "skipped"), vec!["PATCH_FALLBACK"]);
    assert!(v["player"]["skipped"][0]["message"].as_str().unwrap().contains("NO SUCH KNOB"), "{v}");

    // The same sound from a preset file inside the package.
    let preset = d.path().join("p.vstpreset");
    agb_cli::plugin_preset("Dexed", None, None, &["ALGORITHM=0.13".into(), "OP2 OUTPUT LEVEL=0.8".into()], &preset).unwrap();
    let mut with = song.clone();
    with["assets"] = json!({ "pre": { "path": "presets/p.vstpreset", "kind": "preset" } });
    with["patches"]["keys"]["plugin"] = json!({ "name": "Dexed", "preset": "pre" });
    let pdir = song_dir(&d.path().join("p"), &with);
    std::fs::create_dir_all(pdir.join("presets")).unwrap();
    std::fs::copy(&preset, pdir.join("presets/p.vstpreset")).unwrap();
    let v = song_validate(&pdir).unwrap();
    assert_eq!(v["valid"], json!(true), "{v}");
    song_render(&pdir, &args(d.path().join("rp"))).unwrap();
    assert_eq!(a, samples(&d.path().join("rp/mix.wav")), "the preset holds the same settings");
    let packed = d.path().join("p.agbsong");
    song_pack(&pdir, &packed).unwrap();
    song_render(&packed, &args(d.path().join("rz"))).unwrap();
    assert_eq!(a, samples(&d.path().join("rz/mix.wav")), "and travels inside the packed file");
}

/// Played bar by bar, a plug-in goes on from where the last bar stopped instead of being run again
/// from a few seconds earlier: the bars put together are the piece rendered whole, held notes and
/// ringing tails included.
#[test]
fn a_plugin_track_is_seamless_bar_by_bar() {
    if !agb_plugin::available("Dexed", None) {
        eprintln!("Dexed.vst3 is not installed: skipped");
        return;
    }
    use agb_render::{render, render_tail, RenderRequest};
    let song: agb_song::Song = serde_json::from_value(plugin_song("Dexed")).unwrap();
    let p = agb_song::lower(&song, &Default::default()).unwrap().project;
    let req = |to: u64| RenderRequest { from_tick: 0, to_tick: to, tail_ms: 0, seed: 1, stems: false, format: agb_protocol::SampleFormat::F32 };
    let whole = render(&p, &req(7680)).unwrap().mix;
    let tmap = agb_engine::state::tempo_map(&p);
    let mut joined: Vec<f32> = vec![];
    for bar in 0..4u64 {
        let (from, to) = (bar * 1920, (bar + 1) * 1920);
        let r = render_tail(&p, &req(to), from).unwrap();
        let skip = (tmap.tick_to_sample(from) - tmap.tick_to_sample(r.start_tick)) as usize;
        joined.extend(&r.mix[skip..]);
    }
    assert_eq!(joined.len(), whole.len());
    let worst = joined.iter().zip(&whole).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
    let peak = whole.iter().fold(0f32, |a, x| a.max(x.abs()));
    assert!(peak > 0.01, "the plug-in sounds: {peak}");
    assert!(worst < peak * 1e-3, "bars differ from the whole by {worst} (peak {peak})");

    // Starting in the middle (after a seek, say) still gives sound: the plug-in is run in.
    agb_render::forget_plugins();
    let r = render_tail(&p, &req(5760), 3840).unwrap();
    let skip = (tmap.tick_to_sample(3840) - tmap.tick_to_sample(r.start_tick)) as usize;
    assert!(r.mix[skip..].iter().fold(0f32, |a, x| a.max(x.abs())) > 0.005, "the note held across bar 3 sounds");
}

fn sung_song(language: &str, words: &[Option<&str>]) -> Value {
    let mut song = agb_song::template("t");
    song["uses"] = json!(["voice", "fx"]);
    song["patches"]["singer"] = json!({ "engine": "voice.chimera", "params": { "body": "female", "language": language } });
    let pitches = [69, 74, 76, 78, 74, 76, 78, 76, 74, 71, 73, 74];
    let notes: Vec<Value> = words
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let mut n = json!({ "tick": i as u64 * 900, "pitch": pitches[i % pitches.len()], "durTicks": 800, "velocity": 0.8 });
            if let Some(w) = w {
                n["lyric"] = json!(w);
            }
            n
        })
        .collect();
    song["tracks"] = json!([{ "id": "v", "patch": "singer", "fx": [{ "type": "reverb", "params": { "decaySec": 1.2, "mix": 0.2 } }],
        "clips": [{ "atTick": 0, "lengthTicks": 11520, "notes": notes }] }]);
    song["time"]["lengthTicks"] = json!(11520);
    song
}

/// FORMAT.md §8.7: notes with lyrics on a `voice.chimera` track are sung, in English and in pinyin;
/// lyrics that cannot be read, and lyrics on a track that does not sing, are reported.
#[test]
fn a_voice_sings_the_lyrics() {
    let d = tempfile::tempdir().unwrap();
    let en = sung_song("en", &[Some("I"), Some("am"), Some("here"), Some("now"), Some("ask"), Some("me"), Some("anything"), Some("+"), Some("+"), Some("-")]);
    let dir = song_dir(d.path(), &en);
    let v = song_validate(&dir).unwrap();
    assert_eq!(v["valid"], json!(true), "{v}");
    assert!(codes(&v, "warnings").is_empty(), "{v}");
    assert!(codes(&v["player"], "skipped").is_empty(), "{}", v["player"]);
    let r = song_render(&dir, &args(d.path().join("r"))).unwrap();
    assert_eq!(r["nearSilent"], json!(false), "{r}");
    let a = samples(&d.path().join("r/mix.wav"));
    song_render(&dir, &args(d.path().join("r1"))).unwrap();
    assert_eq!(a, samples(&d.path().join("r1/mix.wav")), "the same every time");

    // Other words, another sound; pinyin is read as Mandarin.
    let zh = sung_song("zh", &[Some("wo"), Some("zai"), Some("zhe"), Some("li"), Some("ting"), Some("ni"), Some("shuo"), Some("hua"), Some("-"), Some("我|wo")]);
    let zdir = song_dir(&d.path().join("zh"), &zh);
    let v = song_validate(&zdir).unwrap();
    assert_eq!(v["valid"], json!(true), "{v}");
    assert!(codes(&v, "warnings").is_empty(), "{v}");
    song_render(&zdir, &args(d.path().join("rz"))).unwrap();
    assert_ne!(a, samples(&d.path().join("rz/mix.wav")));

    // Hanzi alone cannot be read; lyrics on a track that is not a voice are not sung.
    let bad = sung_song("zh", &[Some("我"), Some("wo")]);
    let v = song_validate(&song_dir(&d.path().join("bad"), &bad)).unwrap();
    assert!(codes(&v, "warnings").contains(&"LYRIC_UNREADABLE".to_string()), "{v}");
    let mut wrong = en.clone();
    wrong["patches"]["singer"] = json!({ "preset": "preset:synth.waveform:keys:sine_keys@v1.0" });
    let v = song_validate(&song_dir(&d.path().join("wrong"), &wrong)).unwrap();
    assert!(codes(&v, "warnings").contains(&"LYRICS_NOT_SUNG".to_string()), "{v}");
}

/// A singer is in the middle of a word at a bar line: played bar by bar it goes on from where the
/// last bar stopped, and the bars put together are the piece rendered whole.
#[test]
fn a_sung_track_is_seamless_bar_by_bar() {
    use agb_render::{render, render_tail, RenderRequest};
    let words = [Some("I"), Some("am"), Some("here"), Some("now"), Some("ask"), Some("me"), Some("anything"), Some("+"), Some("+"), Some("-"), Some("la"), Some("la")];
    let song: agb_song::Song = serde_json::from_value(sung_song("en", &words)).unwrap();
    let p = agb_song::lower(&song, &Default::default()).unwrap().project;
    let req = |to: u64| RenderRequest { from_tick: 0, to_tick: to, tail_ms: 0, seed: 1, stems: false, format: agb_protocol::SampleFormat::F32 };
    agb_render::forget_plugins();
    let whole = render(&p, &req(11520)).unwrap().mix;
    let tmap = agb_engine::state::tempo_map(&p);
    let mut joined: Vec<f32> = vec![];
    for bar in 0..3u64 {
        let (from, to) = (bar * 3840, (bar + 1) * 3840);
        let r = render_tail(&p, &req(to), from).unwrap();
        let skip = (tmap.tick_to_sample(from) - tmap.tick_to_sample(r.start_tick)) as usize;
        joined.extend(&r.mix[skip..]);
    }
    assert_eq!(joined.len(), whole.len());
    let worst = joined.iter().zip(&whole).map(|(a, b)| (a - b).abs()).fold(0f32, f32::max);
    let peak = whole.iter().fold(0f32, |a, x| a.max(x.abs()));
    assert!(peak > 0.05, "the voice sounds: {peak}");
    assert!(worst < peak * 1e-3, "bars differ from the whole by {worst} (peak {peak})");
}

fn unzip(path: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    use std::io::Read;
    let mut z = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    (0..z.len())
        .map(|i| {
            let mut f = z.by_index(i).unwrap();
            let mut b = vec![];
            f.read_to_end(&mut b).unwrap();
            (f.name().to_string(), b)
        })
        .collect()
}

/// `agb song web`: the .agbw holds the song without what a browser cannot have, and when every
/// note sounds. The music itself is untouched.
#[test]
fn a_song_becomes_an_agbw() {
    use agb_cli::{song_web, SongWebArgs};
    let d = tempfile::tempdir().unwrap();
    let dir = song_dir(d.path(), &plugin_song("No Such Plugin 4711"));
    let out = d.path().join("s.agbw");
    let r = song_web(&dir, &SongWebArgs { out: out.clone(), instruments: vec![], scene: None, lyric_lines: None }).unwrap();
    assert_eq!(r["ok"], json!(true), "{r}");
    let files = unzip(&out);
    assert_eq!(files["mimetype"], b"application/vnd.agb.web+zip");
    let manifest: Value = serde_json::from_slice(&files["web.json"]).unwrap();
    assert_eq!(manifest["format"], json!("agbw"));
    assert_eq!(manifest["visual"]["scene"], json!("orbits"));
    assert_eq!(manifest["instruments"], json!([]));

    // No VST left: the patch is its fallback, and the song still plays here exactly as before.
    let text = String::from_utf8(files["song.json"].clone()).unwrap();
    assert!(!text.contains("plugin.vst3"), "{text}");
    let web_dir = d.path().join("web");
    std::fs::create_dir_all(&web_dir).unwrap();
    std::fs::write(web_dir.join("song.json"), &files["song.json"]).unwrap();
    let v = song_validate(&web_dir).unwrap();
    assert_eq!(v["valid"], json!(true), "{v}");
    assert!(codes(&v["player"], "skipped").is_empty(), "{}", v["player"]);
    song_render(&dir, &args(d.path().join("a"))).unwrap();
    song_render(&web_dir, &args(d.path().join("b"))).unwrap();
    assert_eq!(samples(&d.path().join("a/mix.wav")), samples(&d.path().join("b/mix.wav")));

    // The timeline: every note of the song, in seconds.
    let timeline: Value = serde_json::from_slice(&files["timeline.json"]).unwrap();
    let notes: usize = timeline["tracks"].as_array().unwrap().iter().map(|t| t["notes"].as_array().unwrap().len()).sum();
    assert_eq!(json!(notes), r["notes"]);
    assert!(notes > 0);
    assert!(timeline["seconds"].as_f64().unwrap() > 0.0);
    assert!(!timeline["bars"].as_array().unwrap().is_empty());
}

/// A track given to a web instrument: `plugin.web` with the old patch as its fallback, so the file
/// is still a song every player can play.
#[test]
fn a_web_instrument_takes_a_track() {
    use agb_cli::{song_web, SongWebArgs};
    let d = tempfile::tempdir().unwrap();
    let song = plugin_song("No Such Plugin 4711");
    let track = song["tracks"][0]["id"].as_str().unwrap().to_string();
    let dir = song_dir(d.path(), &song);
    let out = d.path().join("s.agbw");
    let web = |instruments: Vec<String>| song_web(&dir, &SongWebArgs { out: out.clone(), instruments, scene: Some("spectrum".into()), lyric_lines: None });
    let r = web(vec![format!("{track}=agb.epiano:tone=0.7")]).unwrap();
    assert_eq!(r["webInstruments"], json!(["agb.epiano"]), "{r}");
    let files = unzip(&out);
    let s: Value = serde_json::from_slice(&files["song.json"]).unwrap();
    let patch = &s["patches"][s["tracks"][0]["patch"].as_str().unwrap()];
    assert_eq!(patch["engine"], json!("plugin.web"));
    assert_eq!(patch["plugin"], json!({ "name": "agb.epiano", "params": { "tone": 0.7 } }));
    assert_eq!(patch["fallback"][0]["preset"], json!("preset:synth.waveform:keys:sine_keys@v1.0"), "the old patch, without its VST");
    let manifest: Value = serde_json::from_slice(&files["web.json"]).unwrap();
    assert_eq!(manifest["visual"]["scene"], json!("spectrum"));
    // Here, where there are no web instruments, the fallback plays and the player says so.
    let web_dir = d.path().join("web");
    std::fs::create_dir_all(&web_dir).unwrap();
    std::fs::write(web_dir.join("song.json"), &files["song.json"]).unwrap();
    let v = song_validate(&web_dir).unwrap();
    assert_eq!(v["valid"], json!(true), "{v}");
    assert_eq!(codes(&v["player"], "skipped"), vec!["PATCH_FALLBACK"], "{}", v["player"]);

    assert!(web(vec!["nobody=agb.epiano".into()]).is_err());
    assert!(web(vec![format!("{track}=agb.epiano:tone=7")]).is_err());

    // An engine of the format, or a preset, takes the track in every player: nothing falls back.
    let r = web(vec![format!("{track}=preset:synth.fm:keys:tine_piano@v1.0:op2.level=0.2")]).unwrap();
    assert_eq!(r["webInstruments"], json!([]), "{r}");
    let files = unzip(&out);
    let s: Value = serde_json::from_slice(&files["song.json"]).unwrap();
    let patch = &s["patches"][s["tracks"][0]["patch"].as_str().unwrap()];
    assert_eq!(*patch, json!({ "preset": "preset:synth.fm:keys:tine_piano@v1.0", "params": { "op2.level": 0.2 } }));
    std::fs::write(web_dir.join("song.json"), &files["song.json"]).unwrap();
    let v = song_validate(&web_dir).unwrap();
    assert_eq!(v["valid"], json!(true), "{v}");
    assert!(codes(&v["player"], "skipped").is_empty(), "{}", v["player"]);
    web(vec![format!("{track}=synth.fm:feedback=0.3")]).unwrap();
    let s: Value = serde_json::from_slice(&unzip(&out)["song.json"]).unwrap();
    assert_eq!(s["patches"][s["tracks"][0]["patch"].as_str().unwrap()], json!({ "engine": "synth.fm", "params": { "feedback": 0.3 } }));
    assert!(web(vec![format!("{track}=preset:synth.fm:keys:no_such@v1.0")]).is_err());
    assert!(web(vec![format!("{track}=synth.fm:feedback=7")]).is_err(), "out of the engine's range");
}

/// The lyrics of an .agbw: the words as they are shown, with the time each is sung, in lines.
#[test]
fn an_agbw_carries_the_lyrics() {
    use agb_cli::{song_web, SongWebArgs};
    let d = tempfile::tempdir().unwrap();
    let words = [Some("I"), Some("am"), Some("here"), Some("now"), Some("ask"), Some("me"), Some("anything"), Some("+"), Some("+"), Some("-"), Some("我|wo"), Some("zai|在")];
    let dir = song_dir(d.path(), &sung_song("zh", &words));
    let out = d.path().join("s.agbw");
    let r = song_web(&dir, &SongWebArgs { out: out.clone(), instruments: vec![], scene: None, lyric_lines: None }).unwrap();
    assert_eq!(r["ok"], json!(true), "{r}");
    let timeline: Value = serde_json::from_slice(&unzip(&out)["timeline.json"]).unwrap();
    let lines = timeline["lyrics"].as_array().unwrap();
    let shown: Vec<&str> = lines.iter().flat_map(|l| l["words"].as_array().unwrap()).map(|w| w[2].as_str().unwrap()).collect();
    // `+` and `-` carry a word on and show nothing; of `shown|sung` the shown side, whichever way round.
    assert_eq!(shown, ["I", "am", "here", "now", "ask", "me", "anything", "我", "在"]);
    let anything = lines.iter().flat_map(|l| l["words"].as_array().unwrap()).find(|w| w[2] == json!("anything")).unwrap();
    let me = lines.iter().flat_map(|l| l["words"].as_array().unwrap()).find(|w| w[2] == json!("me")).unwrap();
    assert!(anything[1].as_f64().unwrap() - anything[0].as_f64().unwrap() > 2.5 * (me[1].as_f64().unwrap() - me[0].as_f64().unwrap()), "a word lasts over the notes that carry it on");

    // The author's lines, by the tick of each first word.
    let starts = d.path().join("lines.json");
    std::fs::write(&starts, json!({ "lyrics": [[{ "tick": 0 }], { "words": [{ "tick": 3600 }] }] }).to_string()).unwrap();
    song_web(&dir, &SongWebArgs { out: out.clone(), instruments: vec![], scene: None, lyric_lines: Some(starts) }).unwrap();
    let timeline: Value = serde_json::from_slice(&unzip(&out)["timeline.json"]).unwrap();
    let texts: Vec<&str> = timeline["lyrics"].as_array().unwrap().iter().map(|l| l["text"].as_str().unwrap()).collect();
    assert_eq!(texts.len(), 2, "{texts:?}");
    assert_eq!(texts, ["I am here now", "ask me anything 我在"]);
}
