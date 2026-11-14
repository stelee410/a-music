//! `.agbsong` acceptance tests (FORMAT.md) at the CLI-library level.

use agb_cli::{song_pack, song_render, song_unpack, song_validate, SongRenderArgs};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn example() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/first-light.agbsong").canonicalize().unwrap()
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
    let dir = root.join("t.agbsong");
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
    let dir = d.path().join("new.agbsong");
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
