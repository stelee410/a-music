//! `agb song web`: an `.agbsong` made into an `.agbw`, the file of the web player (AGBW.md).
//!
//! The music is not changed. What a browser cannot have is taken out — plug-ins installed on a
//! machine give way to their fallbacks, the video to a scene drawn by the player — and what the
//! player needs to draw is worked out here: when every note sounds, the bars, the sections, the
//! lyrics line by line.

use crate::{fail, CmdResult};
use agb_dsp::EngineParams;
use agb_protocol::{AgbError, ErrorCode};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const MIMETYPE: &str = "application/vnd.agb.web+zip";
pub const FORMAT_NAME: &str = "agbw";
pub const FORMAT_VERSION: &str = "0.1.0";
/// The scene drawn when the song names none the player has.
pub const DEFAULT_SCENE: &str = "orbits";
const VST: &str = "plugin.vst3";
const WEB: &str = "plugin.web";

pub struct SongWebArgs {
    pub out: PathBuf,
    /// `track=instrument` or `track=instrument:param=0.4,param=0.7`: what plays a track instead of what the
    /// song names. An engine id (`synth.fm`) or a preset id (`preset:synth.fm:keys:tine_piano@v1.0`) plays
    /// in every player; any other name is a web instrument (AGBW.md §6), which only the web player has.
    pub instruments: Vec<String>,
    pub scene: Option<String>,
    /// Where the lyric lines begin, as the author wrote them (see [`line_starts`]).
    pub lyric_lines: Option<PathBuf>,
}

fn bad(msg: impl Into<String>) -> crate::ErrorResponse {
    fail(AgbError::new(ErrorCode::SchemaViolation, msg.into()))
}

/// A patch without the plug-ins of a machine: the first of the patch and its fallbacks that is not
/// one, with the others after it as its fallbacks. `None` when there is nothing else.
fn without_vst(patch: &Value) -> Option<Value> {
    fn flat(p: &Value, out: &mut Vec<Value>) {
        let mut own = p.clone();
        let rest = own.as_object_mut().and_then(|o| o.remove("fallback"));
        out.push(own);
        for f in rest.as_ref().and_then(|r| r.as_array()).into_iter().flatten() {
            flat(f, out);
        }
    }
    let mut all = vec![];
    flat(patch, &mut all);
    all.retain(|p| p.get("engine").and_then(|e| e.as_str()) != Some(VST));
    let mut it = all.into_iter();
    let mut first = it.next()?;
    let rest: Vec<Value> = it.collect();
    if !rest.is_empty() {
        first["fallback"] = Value::Array(rest);
    }
    Some(first)
}

fn uses_vst(patch: &Value) -> bool {
    patch.get("engine").and_then(|e| e.as_str()) == Some(VST)
        || patch.get("fallback").and_then(|f| f.as_array()).is_some_and(|f| f.iter().any(uses_vst))
}

/// Asset ids a patch (with its fallbacks) needs: samples and wavetables.
fn patch_assets(patch: &Value, out: &mut BTreeSet<String>) {
    for z in patch.get("zones").and_then(|z| z.as_array()).into_iter().flatten() {
        if let Some(id) = z.get("sample").and_then(|s| s.as_str()) {
            out.insert(id.to_string());
        }
    }
    if let Some(id) = patch.get("wavetable").and_then(|w| w.as_str()) {
        out.insert(id.to_string());
    }
    for f in patch.get("fallback").and_then(|f| f.as_array()).into_iter().flatten() {
        patch_assets(f, out);
    }
}

struct Swap {
    track: String,
    instrument: String,
    params: BTreeMap<String, f64>,
}

fn parse_swap(s: &str) -> Result<Swap, String> {
    let (track, rest) = s.split_once('=').ok_or_else(|| format!("--instrument {s:?}: write track=instrument"))?;
    // The parameters come after the last colon (a preset id has colons of its own).
    let (instrument, params) = match rest.rsplit_once(':') {
        Some((name, tail)) if tail.contains('=') => (name, tail),
        _ => (rest, ""),
    };
    let mut map = BTreeMap::new();
    for kv in params.split(',').filter(|x| !x.trim().is_empty()) {
        let (k, v) = kv.split_once('=').ok_or_else(|| format!("--instrument {s:?}: parameters are name=value"))?;
        let v: f64 = v.trim().parse().map_err(|_| format!("--instrument {s:?}: {v:?} is not a number"))?;
        map.insert(k.trim().to_string(), v);
    }
    if track.trim().is_empty() || instrument.trim().is_empty() {
        return Err(format!("--instrument {s:?}: write track=instrument"));
    }
    Ok(Swap { track: track.trim().into(), instrument: instrument.trim().into(), params: map })
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF)
}

/// Words put together as a line: with spaces, except between characters written without them.
fn join_words(words: &[&str]) -> String {
    let mut out = String::new();
    for w in words {
        let glue = match (out.chars().last(), w.chars().next()) {
            (Some(a), Some(b)) => !(is_cjk(a) && is_cjk(b)),
            _ => false,
        };
        if glue {
            out.push(' ');
        }
        out.push_str(w);
    }
    out
}

/// A sung note: (tick, start s, end s, the word shown or none).
type Sung = (u64, f64, f64, Option<String>);

/// The sung words of one track as lines, from notes in the order they are sung.
///
/// With `starts` (ticks where the author's lines begin, `--lyric-lines`) the lines are exactly
/// those. Without, a line ends where the singer rests for a beat or more, or at a bar line once it
/// holds five words — a guess: where a phrase ends cannot be told from the notes alone.
fn lyric_lines(notes: &[Sung], beat: f64, bar_ticks: u64, starts: Option<&BTreeSet<u64>>, track: &str) -> Vec<Value> {
    let mut lines: Vec<Value> = vec![];
    let mut words: Vec<(u64, f64, f64, String)> = vec![];
    let mut last_end = f64::NEG_INFINITY;
    let mut flush = |words: &mut Vec<(u64, f64, f64, String)>| {
        if let (Some(first), Some(last)) = (words.first(), words.last()) {
            let texts: Vec<&str> = words.iter().map(|w| w.3.as_str()).collect();
            lines.push(json!({
                "track": track,
                "from": ms(first.1),
                "to": ms(last.2),
                "text": join_words(&texts),
                "words": words.iter().map(|w| json!([ms(w.1), ms(w.2), w.3])).collect::<Vec<_>>(),
            }));
        }
        words.clear();
    };
    for (tick, on, off, shown) in notes {
        match shown {
            Some(text) => {
                let new_line = match starts {
                    Some(s) => s.contains(tick),
                    None => {
                        let crossed_bar = words.last().is_some_and(|w| w.0 / bar_ticks != tick / bar_ticks);
                        on - last_end >= beat * 0.98 || (crossed_bar && words.len() >= 5) || words.len() >= 14
                    }
                };
                if new_line {
                    flush(&mut words);
                }
                words.push((*tick, *on, *off, text.clone()));
            }
            // A note that carries the word on: the word lasts until it ends.
            None => {
                if let Some(w) = words.last_mut() {
                    if on - w.2 < beat * 0.98 {
                        w.2 = *off;
                    }
                }
            }
        }
        last_end = last_end.max(*off);
    }
    flush(&mut words);
    lines
}

/// Ticks at which lines begin, from a file of the song's author: a JSON array of lines (or an
/// object with such an array under `lyrics`), each line an array of words or an object with
/// `words`, each word an object with `tick`. A bare array of numbers is taken as the ticks.
fn line_starts(path: &Path) -> Result<BTreeSet<u64>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let lines = v.get("lyrics").unwrap_or(&v).as_array().ok_or_else(|| format!("{}: no array of lyric lines", path.display()))?;
    let mut out = BTreeSet::new();
    for l in lines {
        let first = match l {
            Value::Number(n) => n.as_u64(),
            _ => l.get("words").unwrap_or(l).as_array().and_then(|w| w.first()).and_then(|w| w.get("tick")).and_then(|t| t.as_u64()),
        };
        out.insert(first.ok_or_else(|| format!("{}: a line without a first word's `tick`", path.display()))?);
    }
    Ok(out)
}

/// Seconds, to the millisecond.
fn ms(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

pub fn song_web(path: &Path, a: &SongWebArgs) -> CmdResult {
    let pkg = agb_song::Package::open(path).map_err(fail)?;
    let song = pkg.load_song().map_err(fail)?;
    let report = agb_song::validate(&song, Some(&pkg));
    if !report.is_valid() {
        return Ok(json!({ "ok": false, "valid": false, "errors": report.errors, "warnings": report.warnings }));
    }
    let swaps: Vec<Swap> = a.instruments.iter().map(|s| parse_swap(s)).collect::<Result<_, _>>().map_err(bad)?;
    let starts = a.lyric_lines.as_deref().map(line_starts).transpose().map_err(bad)?;
    let mut notes: Vec<String> = vec![];

    // --- song.json for the web ---------------------------------------------------------------
    let mut web = pkg.load_song_value().map_err(fail)?;
    let mut patches: Map<String, Value> = web.get("patches").and_then(|p| p.as_object()).cloned().unwrap_or_default();
    for (id, patch) in patches.iter_mut() {
        if uses_vst(patch) {
            match without_vst(patch) {
                Some(p) => {
                    notes.push(format!("patch {id:?}: the VST plug-in is left out, the web player plays its fallback"));
                    *patch = p;
                }
                None => notes.push(format!("patch {id:?}: only a VST plug-in and no fallback — its tracks are silent on the web")),
            }
        }
    }
    let mut web_instruments = BTreeSet::new();
    fn collect_web(p: &Value, out: &mut BTreeSet<String>) {
        if p.get("engine").and_then(|e| e.as_str()) == Some(WEB) {
            if let Some(n) = p.pointer("/plugin/name").and_then(|n| n.as_str()) {
                out.insert(n.to_string());
            }
        }
        for f in p.get("fallback").and_then(|f| f.as_array()).into_iter().flatten() {
            collect_web(f, out);
        }
    }
    for s in &swaps {
        let Some(track) = web.get_mut("tracks").and_then(|t| t.as_array_mut()).and_then(|t| t.iter_mut().find(|t| t.get("id").and_then(|i| i.as_str()) == Some(&s.track))) else {
            return Err(bad(format!("--instrument: the song has no track {:?}", s.track)));
        };
        let old_id = track.get("patch").and_then(|p| p.as_str()).unwrap_or_default().to_string();
        let old = patches.get(&old_id).cloned().ok_or_else(|| bad(format!("track {:?}: unknown patch {old_id:?}", s.track)))?;
        let mut new_id = format!("{}-web", s.track);
        while patches.contains_key(&new_id) {
            new_id.push('x');
        }
        // An engine of the format or one of its presets plays everywhere (the Mac app, the web, the
        // handheld); anything else names a web instrument, which only the web player has.
        let built_in = |engine: &str| engine != agb_dsp::plugin::ENGINE_ID && engine != WEB && agb_dsp::engine_status(engine) == agb_dsp::EngineStatus::Available;
        let patch = if s.instrument.starts_with("preset:") {
            let preset = agb_presets::get(&s.instrument).ok_or_else(|| bad(format!("--instrument: there is no preset {:?}", s.instrument)))?;
            let given: BTreeMap<String, agb_dsp::ParamValue> = s.params.iter().map(|(k, v)| (k.clone(), agb_dsp::ParamValue::Num(*v))).collect();
            agb_dsp::EngineParams::resolve(&preset.engine, &[&preset.parameters, &given]).map_err(|e| bad(format!("--instrument {:?}: {e}", s.instrument)))?;
            notes.push(format!("track {:?} is played by the preset {:?}", s.track, s.instrument));
            json!({ "preset": s.instrument, "params": s.params })
        } else if built_in(&s.instrument) {
            let given: BTreeMap<String, agb_dsp::ParamValue> = s.params.iter().map(|(k, v)| (k.clone(), agb_dsp::ParamValue::Num(*v))).collect();
            agb_dsp::EngineParams::resolve(&s.instrument, &[&given]).map_err(|e| bad(format!("--instrument {:?}: {e}", s.instrument)))?;
            notes.push(format!("track {:?} is played by the engine {:?}", s.track, s.instrument));
            json!({ "engine": s.instrument, "params": s.params })
        } else {
            if s.params.values().any(|v| !(0.0..=1.0).contains(v)) {
                return Err(bad(format!("--instrument {:?}: the parameters of a web instrument go from 0 to 1", s.instrument)));
            }
            notes.push(format!("track {:?} is played by the web instrument {:?}", s.track, s.instrument));
            json!({ "engine": WEB, "plugin": { "name": s.instrument, "params": s.params }, "fallback": [old] })
        };
        patches.insert(new_id.clone(), patch);
        track["patch"] = json!(new_id);
    }
    // Patches no track uses any more go, with their samples.
    let used: BTreeSet<String> =
        web.get("tracks").and_then(|t| t.as_array()).into_iter().flatten().filter_map(|t| t.get("patch").and_then(|p| p.as_str()).map(str::to_string)).collect();
    patches.retain(|id, _| used.contains(id));
    patches.values().for_each(|p| collect_web(p, &mut web_instruments));

    let mut dropped_fx = 0usize;
    for t in web.get_mut("tracks").and_then(|t| t.as_array_mut()).into_iter().flatten() {
        if let Some(fx) = t.get_mut("fx").and_then(|f| f.as_array_mut()) {
            let before = fx.len();
            fx.retain(|f| f.get("type").and_then(|x| x.as_str()) != Some(VST));
            dropped_fx += before - fx.len();
            for f in fx.iter().filter(|f| f.get("type").and_then(|x| x.as_str()) == Some(WEB)) {
                if let Some(n) = f.pointer("/params/plugin").and_then(|n| n.as_str()) {
                    web_instruments.insert(n.to_string());
                }
            }
        }
    }
    if dropped_fx > 0 {
        notes.push(format!("{dropped_fx} VST effect(s) left out"));
    }

    // Assets: what the patches need and the cover. The video and plug-in presets stay behind.
    let mut keep = BTreeSet::new();
    patches.values().for_each(|p| patch_assets(p, &mut keep));
    if let Some(c) = &song.meta.cover {
        keep.insert(c.clone());
    }
    let assets: Map<String, Value> =
        web.get("assets").and_then(|x| x.as_object()).map(|m| m.iter().filter(|(id, _)| keep.contains(*id)).map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default();
    let had_video = song.visuals.as_ref().is_some_and(|v| v.video.is_some());
    if had_video {
        notes.push("the video is left out: the web player draws a scene instead".into());
    }
    let scene = a.scene.clone().or_else(|| song.visuals.as_ref().and_then(|v| v.scene.clone())).unwrap_or_else(|| DEFAULT_SCENE.to_string());
    let palette: Vec<String> = song.visuals.as_ref().map(|v| v.palette.clone()).unwrap_or_default();
    if let Some(v) = web.get_mut("visuals").and_then(|v| v.as_object_mut()) {
        v.remove("video");
        v.remove("shader");
        v.insert("scene".into(), json!(scene));
    }
    web["patches"] = Value::Object(patches);
    web["assets"] = Value::Object(assets.clone());

    // It must still be a song this build reads.
    let web_song: agb_song::Song = serde_json::from_value(web.clone()).map_err(|e| bad(format!("the converted song does not parse: {e}")))?;
    let web_report = agb_song::validate(&web_song, None);
    if let Some(first) = web_report.errors.first() {
        return Err(bad(format!("the converted song is not valid: {} ({})", first.message, first.path)));
    }

    // --- timeline.json -----------------------------------------------------------------------
    // Lowered here only to learn when the notes sound: which engine plays them does not matter,
    // but a sampler track without its samples would be left out, so the samples are at hand.
    let samples = crate::sample_files(&pkg, &song, &crate::sample_scratch()).map_err(|e| fail(AgbError::io(e)))?;
    let l = agb_song::lower(&web_song, &agb_song::PlayOptions { samples, ..Default::default() }).map_err(fail)?;
    let p = &l.project;
    let sr = p.sample_rate as f64;
    let tmap = agb_engine::state::tempo_map(p);
    let sec = |tick: u64| tmap.tick_to_sample(tick) as f64 / sr;
    let sched = agb_engine::schedule(p, 0, l.length_ticks).map_err(fail)?;
    let length = sec(l.length_ticks);
    let beat = 60.0 / p.tempo_bpm;

    let (num, den) = song.time.meter;
    let bar_ticks = (p.ppqn as u64 * 4 * num.max(1) as u64 / den.max(1) as u64).max(1);
    let mut bars = vec![];
    let mut t = 0u64;
    while t < l.length_ticks {
        bars.push(ms(sec(t)));
        t += bar_ticks;
    }

    let mut tracks = vec![];
    let mut lyrics = vec![];
    for tr in &web_song.tracks {
        let mut evs: Vec<&agb_engine::NoteEvent> = sched.events.iter().filter(|e| e.track_id == tr.id).collect();
        evs.sort_by_key(|e| (e.tick, e.pitch));
        let engine = evs.first().map(|e| e.engine_id.clone());
        let sung = evs.iter().find_map(|e| match &e.params {
            EngineParams::Voice(v) => Some(v.language.clone()),
            _ => None,
        });
        if let Some(language) = &sung {
            let line: Vec<Sung> = evs
                .iter()
                .map(|e| {
                    let shown = e.lyric.as_deref().and_then(|l| agb_voice::shown_part(language, l)).map(|w| w.replace('-', ""));
                    (e.tick, sec(e.tick), sec(e.off_tick), shown.filter(|w| !w.is_empty()))
                })
                .collect();
            lyrics.extend(lyric_lines(&line, beat, bar_ticks, starts.as_ref(), &tr.id));
        }
        let played_by = web_song.patches.get(&tr.patch).and_then(|p| p.engine.clone());
        tracks.push(json!({
            "id": tr.id,
            "name": tr.name.clone().unwrap_or_else(|| tr.id.clone()),
            "role": tr.role,
            "engine": played_by.or(engine),
            "sung": sung.is_some(),
            // [start s, length s, MIDI pitch, velocity 0…1]
            "notes": evs.iter().map(|e| json!([ms(sec(e.tick)), ms(sec(e.off_tick) - sec(e.tick)), e.pitch as i32 + e.transpose, (e.velocity * 100.0).round() / 100.0])).collect::<Vec<_>>(),
        }));
    }
    lyrics.sort_by(|a, b| a["from"].as_f64().partial_cmp(&b["from"].as_f64()).unwrap_or(std::cmp::Ordering::Equal));
    let n_notes: usize = sched.events.len();
    let timeline = json!({
        "seconds": ms(length),
        "bpm": p.tempo_bpm,
        "meter": [num, den],
        "bars": bars,
        "sections": song.sections.iter().map(|s| json!({
            "id": s.id, "name": s.name.clone().unwrap_or_else(|| s.id.clone()),
            "from": ms(sec(s.from_tick)), "to": ms(sec(s.to_tick.min(l.length_ticks))), "intent": s.intent,
        })).collect::<Vec<_>>(),
        "tracks": tracks,
        "lyrics": lyrics,
    });

    // --- web.json ----------------------------------------------------------------------------
    let source = std::fs::read(path).ok().filter(|_| pkg.is_zip()).map(|b| agb_song::package::sha256_tag(&b));
    let cover = song.meta.cover.as_ref().and_then(|c| song.assets.get(c)).map(|c| c.path.clone());
    let manifest = json!({
        "format": FORMAT_NAME,
        "formatVersion": FORMAT_VERSION,
        "title": song.meta.title,
        "artists": song.meta.artists,
        "description": song.meta.description,
        "tags": song.meta.tags,
        "license": song.meta.license,
        "created": song.meta.created,
        "key": song.tuning.key,
        "scale": song.tuning.scale,
        "bpm": song.time.bpm,
        "seconds": ms(length),
        "cover": cover,
        "song": agb_song::SONG_FILE,
        "timeline": "timeline.json",
        "engine": { "version": agb_dsp::ENGINE_VERSION, "sampleRate": p.sample_rate },
        "instruments": web_instruments,
        "visual": { "scene": scene, "palette": palette },
        "source": { "format": agb_song::FORMAT_NAME, "formatVersion": song.format_version, "sha256": source },
        "generator": { "name": "agb", "version": env!("CARGO_PKG_VERSION") },
    });

    // --- the file ----------------------------------------------------------------------------
    let io = |e: std::io::Error| fail(AgbError::io(e));
    let zerr = |e: zip::result::ZipError| fail(AgbError::io(e));
    if let Some(parent) = a.out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    let tmp = a.out.with_extension(format!("tmp.{}", std::process::id()));
    let mut zw = zip::ZipWriter::new(std::fs::File::create(&tmp).map_err(io)?);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut files = vec![];
    let mut put = |zw: &mut zip::ZipWriter<std::fs::File>, name: &str, bytes: &[u8], opt| -> Result<(), crate::ErrorResponse> {
        zw.start_file(name, opt).map_err(zerr)?;
        zw.write_all(bytes).map_err(io)?;
        files.push(name.to_string());
        Ok(())
    };
    let pretty = |v: &Value| {
        let mut b = serde_json::to_vec(v).unwrap();
        b.push(b'\n');
        b
    };
    put(&mut zw, "mimetype", MIMETYPE.as_bytes(), stored)?;
    put(&mut zw, "web.json", &pretty(&manifest), deflated)?;
    put(&mut zw, agb_song::SONG_FILE, &pretty(&web), deflated)?;
    put(&mut zw, "timeline.json", &pretty(&timeline), deflated)?;
    let mut done = BTreeSet::new();
    for asset in assets.values() {
        let Some(rel) = asset.get("path").and_then(|p| p.as_str()) else { continue };
        if !done.insert(rel.to_string()) || !agb_song::package::safe_rel_path(rel) {
            continue;
        }
        put(&mut zw, rel, &pkg.read(rel).map_err(fail)?, deflated)?;
    }
    zw.finish().map_err(zerr)?;
    std::fs::rename(&tmp, &a.out).map_err(io)?;
    let bytes = std::fs::metadata(&a.out).map(|m| m.len()).unwrap_or(0);

    Ok(json!({
        "ok": true,
        "op": "song.web",
        "out": a.out,
        "bytes": bytes,
        "title": song.meta.title,
        "seconds": ms(length),
        "tracks": web_song.tracks.len(),
        "notes": n_notes,
        "lyricLines": timeline["lyrics"].as_array().map_or(0, |l| l.len()),
        "scene": scene,
        "webInstruments": manifest["instruments"],
        "files": files,
        "changes": notes,
        "skippedHere": l.skipped,
    }))
}
