//! Format-level validation (FORMAT.md). Independent of what this build can play:
//! a file using `sampler.multi` is valid even though the engine cannot render it.

use crate::model::*;
use crate::package::{safe_rel_path, Package};
use crate::{FORMAT_NAME, FORMAT_VERSION};
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub code: &'static str,
    /// JSON-ish path of the offending element, e.g. `tracks[1].loops[0]`.
    pub path: String,
    pub message: String,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub errors: Vec<Issue>,
    pub warnings: Vec<Issue>,
}

impl Report {
    fn err(&mut self, code: &'static str, path: impl Into<String>, message: impl Into<String>) {
        self.errors.push(Issue { code, path: path.into(), message: message.into() });
    }
    fn warn(&mut self, code: &'static str, path: impl Into<String>, message: impl Into<String>) {
        self.warnings.push(Issue { code, path: path.into(), message: message.into() });
    }
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

pub const KNOWN_FEATURES: &[&str] =
    &["core", "automation", "expression", "sampler", "wavetable", "fx", "overlay", "visuals", "agent"];
const OVERLAY_KINDS: &[&str] = &["tone", "binaural", "isochronic", "noise"];
const FX_TYPES: &[&str] = &["eq", "filter", "delay", "reverb", "chorus", "drive", "compressor", "amp"];
const SCENES: &[&str] = &["orbits", "river", "field", "spectrum"];
const AUDIO_KINDS: &[&str] = &["sample", "wavetable", "audio"];

pub fn valid_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn is_extension_id(s: &str) -> bool {
    s.starts_with("x-") || s.contains('.')
}

/// `Ok(())` when a file of this version can be read by this build (FORMAT.md §3.1).
pub fn check_version(v: &str) -> Result<(), String> {
    let parts: Vec<Option<u64>> = v.split('.').map(|s| s.parse().ok()).collect();
    let ours: Vec<u64> = FORMAT_VERSION.split('.').map(|s| s.parse().unwrap()).collect();
    match parts.as_slice() {
        [Some(maj), Some(min), Some(_)] => {
            let compatible = if *maj == 0 { ours[0] == 0 && *min == ours[1] } else { *maj == ours[0] };
            if compatible {
                Ok(())
            } else {
                Err(format!("formatVersion {v} is not readable by this build ({FORMAT_VERSION})"))
            }
        }
        _ => Err(format!("formatVersion {v:?} is not MAJOR.MINOR.PATCH")),
    }
}

/// Features the file actually touches, derived from its content.
pub fn features_used(s: &Song) -> BTreeSet<&'static str> {
    let mut f = BTreeSet::from(["core"]);
    fn patch_features(p: &Patch, f: &mut BTreeSet<&'static str>) {
        match p.engine.as_deref() {
            Some("sampler.multi") => {
                f.insert("sampler");
            }
            Some("synth.wavetable") => {
                f.insert("wavetable");
            }
            _ => {}
        }
        if !p.modulation.is_empty() {
            f.insert("expression");
        }
    }
    for p in s.patches.values() {
        patch_features(p, &mut f);
    }
    for t in &s.tracks {
        if !t.automation.is_empty() {
            f.insert("automation");
        }
        if !t.fx.is_empty() {
            f.insert("fx");
        }
        let expr = !t.controllers.is_empty()
            || t.loops.iter().any(|l| l.notes.iter().any(|n| n.expression.is_some()))
            || t.clips.iter().any(|c| c.notes.iter().any(|n| n.expression.is_some()));
        if expr {
            f.insert("expression");
        }
    }
    if !s.master.fx.is_empty() {
        f.insert("fx");
    }
    if !s.master.automation.is_empty() {
        f.insert("automation");
    }
    if s.overlay.as_ref().is_some_and(|o| !o.layers.is_empty()) {
        f.insert("overlay");
    }
    if s.visuals.is_some() {
        f.insert("visuals");
    }
    if s.performance.as_ref().is_some_and(|p| p.agent.is_some()) {
        f.insert("agent");
    }
    f
}

fn check_points(r: &mut Report, path: &str, ticks: impl Iterator<Item = u64>) {
    let mut last = None;
    for t in ticks {
        if last.is_some_and(|l| t < l) {
            r.err("POINTS_UNSORTED", path, "points must be in ascending tick order");
            return;
        }
        last = Some(t);
    }
}

fn check_unit(r: &mut Report, path: &str, name: &str, v: f64) {
    if !(0.0..=1.0).contains(&v) {
        r.err("OUT_OF_RANGE", path, format!("{name} {v} must be within 0..=1"));
    }
}

fn check_expression(r: &mut Report, path: &str, e: &Option<Expression>) {
    let Some(e) = e else { return };
    for (name, pts) in [("bend", &e.bend), ("pressure", &e.pressure), ("timbre", &e.timbre)] {
        check_points(r, &format!("{path}.expression.{name}"), pts.iter().map(|p| p.0));
        if name != "bend" {
            for p in pts {
                check_unit(r, &format!("{path}.expression.{name}"), name, p.1);
            }
        }
    }
}

fn check_asset_ref(r: &mut Report, s: &Song, path: &str, id: &str, kinds: &[&str]) {
    match s.assets.get(id) {
        None => r.err("UNKNOWN_ASSET", path, format!("asset {id:?} is not declared in assets")),
        Some(a) => {
            if let Some(k) = a.kind.as_deref() {
                if !kinds.contains(&k) {
                    r.warn("ASSET_KIND", path, format!("asset {id:?} has kind {k:?}, expected one of {kinds:?}"));
                }
            }
        }
    }
}

fn check_fx(r: &mut Report, path: &str, fx: &[Fx]) {
    let mut ids = BTreeSet::new();
    for (i, f) in fx.iter().enumerate() {
        let p = format!("{path}[{i}]");
        if let Some(id) = &f.id {
            if !valid_id(id) {
                r.err("INVALID_ID", &p, format!("fx id {id:?}"));
            }
            if !ids.insert(id.clone()) {
                r.err("DUPLICATE_ID", &p, format!("fx id {id:?} used twice in this chain"));
            }
        }
        if !FX_TYPES.contains(&f.fx_type.as_str()) && !is_extension_id(&f.fx_type) {
            r.warn("UNKNOWN_FX", &p, format!("fx type {:?} is not defined by format {FORMAT_VERSION}", f.fx_type));
        }
    }
}

fn check_patch(r: &mut Report, s: &Song, path: &str, p: &Patch) {
    if p.engine.is_none() && p.preset.is_none() {
        r.err("PATCH_NO_ENGINE", path, "a patch needs `engine` or `preset`");
    }
    for (i, z) in p.zones.iter().enumerate() {
        let zp = format!("{path}.zones[{i}]");
        check_asset_ref(r, s, &zp, &z.sample, &["sample", "audio"]);
        if let Some((lo, hi)) = z.key_range {
            if lo > hi || hi > 127 {
                r.err("OUT_OF_RANGE", &zp, "keyRange must be [low, high] within 0..=127");
            }
        }
        if let Some(Err(e)) = z.root_pitch.as_ref().map(|p| p.to_midi()) {
            r.err("INVALID_PITCH", &zp, e);
        }
    }
    match p.engine.as_deref() {
        Some("sampler.multi") if p.zones.is_empty() => {
            r.err("PATCH_INCOMPLETE", path, "sampler.multi needs at least one zone")
        }
        Some("synth.wavetable") if p.wavetable.is_none() => {
            r.err("PATCH_INCOMPLETE", path, "synth.wavetable needs `wavetable`")
        }
        _ => {}
    }
    if let Some(w) = &p.wavetable {
        check_asset_ref(r, s, &format!("{path}.wavetable"), w, &["wavetable", "sample", "audio"]);
    }
    for (name, targets) in &p.macros {
        if targets.is_empty() {
            r.warn("EMPTY_MACRO", format!("{path}.macros.{name}"), "macro has no targets");
        }
    }
    for (i, fb) in p.fallback.iter().enumerate() {
        check_patch(r, s, &format!("{path}.fallback[{i}]"), fb);
    }
}

fn check_automation(r: &mut Report, path: &str, lanes: &[Automation], prefixes: &[&str]) {
    for (i, a) in lanes.iter().enumerate() {
        let p = format!("{path}[{i}]");
        if !prefixes.iter().any(|pre| a.target.starts_with(pre)) {
            r.err("INVALID_TARGET", &p, format!("target {:?} must start with one of {prefixes:?}", a.target));
        }
        if a.points.is_empty() {
            r.err("EMPTY_POINTS", &p, "automation needs at least one point");
        }
        check_points(r, &p, a.points.iter().map(|x| x.tick()));
        if a.points.iter().any(|x| !x.value().is_finite()) {
            r.err("OUT_OF_RANGE", &p, "values must be finite");
        }
    }
}

/// Validate the document itself. `pkg` adds checks that need the files
/// (existence, hashes).
pub fn validate(s: &Song, pkg: Option<&Package>) -> Report {
    let mut r = Report::default();

    // ---- header ----
    if s.format != FORMAT_NAME {
        r.err("FORMAT", "format", format!("expected {FORMAT_NAME:?}, found {:?}", s.format));
    }
    if let Err(e) = check_version(&s.format_version) {
        r.err("FORMAT_VERSION", "formatVersion", e);
    }
    for (field, list) in [("requires", &s.requires), ("uses", &s.uses)] {
        for f in list {
            if !KNOWN_FEATURES.contains(&f.as_str()) && !is_extension_id(f) {
                r.warn(
                    "UNKNOWN_FEATURE",
                    field,
                    format!("feature {f:?} is not defined by format {FORMAT_VERSION}; third-party ids need an `x-` prefix or a dot"),
                );
            }
        }
    }
    let declared: BTreeSet<&str> = s.requires.iter().chain(&s.uses).map(String::as_str).collect();
    for f in features_used(s) {
        if !declared.contains(f) {
            r.warn("UNDECLARED_FEATURE", "uses", format!("the file uses {f:?} but lists it in neither `requires` nor `uses`"));
        }
    }

    // ---- meta ----
    let n = s.meta.title.chars().count();
    if n == 0 || n > 128 {
        r.err("OUT_OF_RANGE", "meta.title", "title must be 1-128 characters");
    }
    if let Some(c) = &s.meta.cover {
        check_asset_ref(&mut r, s, "meta.cover", c, &["image"]);
    }

    // ---- time ----
    let t = &s.time;
    let bpm_ok = |b: f64| b.is_finite() && (20.0..=400.0).contains(&b);
    if !bpm_ok(t.bpm) {
        r.err("OUT_OF_RANGE", "time.bpm", "bpm must be within 20..=400");
    }
    if t.ppqn == 0 || t.steps_per_quarter == 0 || t.ppqn % t.steps_per_quarter.max(1) != 0 {
        r.err("TIME_GRID", "time", "ppqn must be a positive multiple of stepsPerQuarter");
    }
    check_points(&mut r, "time.tempoMap", t.tempo_map.iter().map(|p| p.tick));
    if t.tempo_map.iter().any(|p| !bpm_ok(p.bpm)) {
        r.err("OUT_OF_RANGE", "time.tempoMap", "bpm must be within 20..=400");
    }
    if t.meter.0 == 0 || !matches!(t.meter.1, 1 | 2 | 4 | 8 | 16) {
        r.err("OUT_OF_RANGE", "time.meter", "meter must be [beats, 1|2|4|8|16]");
    }
    if let Some(l) = &t.loop_range {
        if l.to_tick <= l.from_tick {
            r.err("OUT_OF_RANGE", "time.loop", "toTick must be greater than fromTick");
        }
    }
    if !(s.tuning.a4_hz.is_finite() && (300.0..=600.0).contains(&s.tuning.a4_hz)) {
        r.err("OUT_OF_RANGE", "tuning.a4Hz", "a4Hz must be within 300..=600");
    }

    // ---- assets ----
    for (id, a) in &s.assets {
        let p = format!("assets.{id}");
        if !valid_id(id) {
            r.err("INVALID_ID", &p, "ids must match [a-zA-Z0-9_-]{1,64}");
        }
        if !safe_rel_path(&a.path) {
            r.err("UNSAFE_PATH", &p, format!("path {:?} must be relative and stay inside the package", a.path));
            continue;
        }
        let audio = a.kind.as_deref().is_some_and(|k| AUDIO_KINDS.contains(&k));
        if audio && a.license.is_none() {
            r.warn("LICENSE_MISSING", &p, "audio assets should carry a license record");
        }
        if a.sha256.is_none() {
            r.warn("HASH_MISSING", &p, "no sha256; the file cannot be verified");
        }
        if let Some(pkg) = pkg {
            match pkg.sha256(&a.path) {
                Err(_) => r.err("ASSET_MISSING", &p, format!("file not found in package: {}", a.path)),
                Ok(h) => {
                    if a.sha256.as_ref().is_some_and(|want| *want != h) {
                        r.err("ASSET_HASH_MISMATCH", &p, format!("expected {}, found {h}", a.sha256.as_ref().unwrap()));
                    }
                }
            }
        }
    }

    // ---- patches ----
    for (id, p) in &s.patches {
        let path = format!("patches.{id}");
        if !valid_id(id) {
            r.err("INVALID_ID", &path, "ids must match [a-zA-Z0-9_-]{1,64}");
        }
        check_patch(&mut r, s, &path, p);
    }

    // ---- tracks ----
    if s.tracks.is_empty() {
        r.warn("NO_TRACKS", "tracks", "the song has no tracks");
    }
    let mut track_ids = BTreeSet::new();
    for (ti, tr) in s.tracks.iter().enumerate() {
        let tp = format!("tracks[{ti}]");
        if !valid_id(&tr.id) {
            r.err("INVALID_ID", &tp, format!("track id {:?}", tr.id));
        }
        if !track_ids.insert(tr.id.as_str()) {
            r.err("DUPLICATE_ID", &tp, format!("track id {:?} used twice", tr.id));
        }
        if !s.patches.contains_key(&tr.patch) {
            r.err("UNKNOWN_PATCH", &tp, format!("patch {:?} is not defined in patches", tr.patch));
        }
        if !(-60.0..=12.0).contains(&tr.gain_db) {
            r.err("OUT_OF_RANGE", &tp, "gainDb must be within -60..=12");
        }
        if !(-1.0..=1.0).contains(&tr.pan) {
            r.err("OUT_OF_RANGE", &tp, "pan must be within -1..=1");
        }
        let mut loop_ids = BTreeSet::new();
        for (li, l) in tr.loops.iter().enumerate() {
            let lp = format!("{tp}.loops[{li}]");
            if !valid_id(&l.id) {
                r.err("INVALID_ID", &lp, format!("loop id {:?}", l.id));
            }
            if !loop_ids.insert(l.id.as_str()) {
                r.err("DUPLICATE_ID", &lp, format!("loop id {:?} used twice in track", l.id));
            }
            if l.length_steps == 0 {
                r.err("OUT_OF_RANGE", &lp, "lengthSteps must be >= 1");
            }
            if let Some(st) = &l.steps {
                if st.len() != l.length_steps as usize || st.iter().any(|x| *x > 1) {
                    r.err("STEPS", &lp, "steps must hold lengthSteps values of 0 or 1");
                }
            }
            if let (Some(a), Some(b)) = (l.from_tick, l.to_tick) {
                if b <= a {
                    r.err("OUT_OF_RANGE", &lp, "toTick must be greater than fromTick");
                }
            }
            for (ni, n) in l.notes.iter().enumerate() {
                let np = format!("{lp}.notes[{ni}]");
                if n.step >= l.length_steps {
                    r.err("OUT_OF_RANGE", &np, format!("step {} is outside 0..{}", n.step, l.length_steps));
                }
                if n.dur_steps == 0 {
                    r.err("OUT_OF_RANGE", &np, "durSteps must be >= 1");
                }
                if let Err(e) = n.pitch.to_midi() {
                    r.err("INVALID_PITCH", &np, e);
                }
                check_unit(&mut r, &np, "velocity", n.velocity);
                check_expression(&mut r, &np, &n.expression);
            }
        }
        for (ci, c) in tr.clips.iter().enumerate() {
            let cp = format!("{tp}.clips[{ci}]");
            if c.repeat == 0 {
                r.err("OUT_OF_RANGE", &cp, "repeat must be >= 1");
            }
            if c.repeat > 1 && c.length_ticks.is_none() {
                r.err("CLIP_LENGTH", &cp, "repeat > 1 needs lengthTicks");
            }
            if c.length_ticks == Some(0) {
                r.err("OUT_OF_RANGE", &cp, "lengthTicks must be >= 1");
            }
            if let Some(m) = &c.midi {
                check_asset_ref(&mut r, s, &format!("{cp}.midi"), &m.asset, &["midi"]);
            } else if c.notes.is_empty() {
                r.warn("EMPTY_CLIP", &cp, "clip has neither notes nor midi");
            }
            for (ni, n) in c.notes.iter().enumerate() {
                let np = format!("{cp}.notes[{ni}]");
                if n.dur_ticks == 0 {
                    r.err("OUT_OF_RANGE", &np, "durTicks must be >= 1");
                }
                if let Err(e) = n.pitch.to_midi() {
                    r.err("INVALID_PITCH", &np, e);
                }
                check_unit(&mut r, &np, "velocity", n.velocity);
                check_expression(&mut r, &np, &n.expression);
            }
        }
        check_automation(&mut r, &format!("{tp}.automation"), &tr.automation, &["patch.", "track.", "fx."]);
        for (i, c) in tr.controllers.iter().enumerate() {
            let cp = format!("{tp}.controllers[{i}]");
            if c.name.is_some() == c.cc.is_some() {
                r.err("CONTROLLER", &cp, "give exactly one of `name` or `cc`");
            }
            if c.cc.is_some_and(|n| n > 127) {
                r.err("OUT_OF_RANGE", &cp, "cc must be within 0..=127");
            }
            check_points(&mut r, &cp, c.points.iter().map(|p| p.0));
        }
        check_fx(&mut r, &format!("{tp}.fx"), &tr.fx);
    }

    // ---- master / sections ----
    if !(-60.0..=0.0).contains(&s.master.gain_db) {
        r.err("OUT_OF_RANGE", "master.gainDb", "gainDb must be within -60..=0");
    }
    check_fx(&mut r, "master.fx", &s.master.fx);
    check_automation(&mut r, "master.automation", &s.master.automation, &["master.", "fx."]);
    let mut section_ids = BTreeSet::new();
    for (i, sec) in s.sections.iter().enumerate() {
        let p = format!("sections[{i}]");
        if !valid_id(&sec.id) || !section_ids.insert(sec.id.as_str()) {
            r.err("INVALID_ID", &p, format!("section id {:?} is invalid or duplicated", sec.id));
        }
        if sec.to_tick <= sec.from_tick {
            r.err("OUT_OF_RANGE", &p, "toTick must be greater than fromTick");
        }
    }

    // ---- overlay ----
    if let Some(o) = &s.overlay {
        let mut ids = BTreeSet::new();
        for (i, l) in o.layers.iter().enumerate() {
            let p = format!("overlay.layers[{i}]");
            if !valid_id(&l.id) || !ids.insert(l.id.as_str()) {
                r.err("INVALID_ID", &p, format!("overlay layer id {:?} is invalid or duplicated", l.id));
            }
            let audible = |v: Option<f64>| v.is_some_and(|h| h.is_finite() && h > 0.0 && h <= 20000.0);
            match l.kind.as_str() {
                "tone" if !audible(l.hz) => r.err("OVERLAY", &p, "tone needs hz within 0..=20000"),
                "binaural" if !audible(l.carrier_hz) || !l.beat_hz.is_some_and(|b| b > 0.0 && b <= 100.0) => {
                    r.err("OVERLAY", &p, "binaural needs carrierHz and beatHz (0..=100)")
                }
                "isochronic" if !audible(l.hz) || !l.pulse_hz.is_some_and(|b| b > 0.0 && b <= 100.0) => {
                    r.err("OVERLAY", &p, "isochronic needs hz and pulseHz (0..=100)")
                }
                "noise" if !matches!(l.color.as_deref(), None | Some("white" | "pink" | "brown")) => {
                    r.err("OVERLAY", &p, "noise color must be white, pink or brown")
                }
                k if !OVERLAY_KINDS.contains(&k) && !is_extension_id(k) => {
                    r.warn("UNKNOWN_OVERLAY", &p, format!("overlay kind {k:?} is not defined by format {FORMAT_VERSION}"))
                }
                _ => {}
            }
            if l.gain_db.is_some_and(|g| g > -6.0) {
                r.err("OUT_OF_RANGE", &p, "overlay gainDb must not exceed -6");
            }
            check_points(&mut r, &p, l.schedule.iter().map(|x| x.0));
        }
    }

    // ---- visuals ----
    if let Some(v) = &s.visuals {
        if let Some(sc) = &v.scene {
            if !SCENES.contains(&sc.as_str()) && !is_extension_id(sc) {
                r.warn("UNKNOWN_SCENE", "visuals.scene", format!("scene {sc:?} is not defined by format {FORMAT_VERSION}"));
            }
        }
        for (i, c) in v.palette.iter().enumerate() {
            let hex = c.strip_prefix('#').unwrap_or("");
            if !(matches!(hex.len(), 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit())) {
                r.err("COLOR", format!("visuals.palette[{i}]"), format!("{c:?} must be #rrggbb or #rrggbbaa"));
            }
        }
        if let Some(sh) = &v.shader {
            check_asset_ref(&mut r, s, "visuals.shader", sh, &["shader"]);
        }
        if let Some(video) = &v.video {
            check_asset_ref(&mut r, s, "visuals.video", &video.asset, &["video"]);
            if !matches!(video.sync.as_deref(), None | Some("loop" | "song")) {
                r.err("VIDEO_SYNC", "visuals.video.sync", "sync must be \"loop\" or \"song\"");
            }
            if let Some(a) = s.assets.get(&video.asset) {
                let ext = a.path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
                if !matches!(ext.as_str(), "mp4" | "m4v" | "mov") {
                    r.warn("VIDEO_FORMAT", "visuals.video", format!("{} is not MP4/MOV; players may not be able to show it", a.path));
                }
                if let Some(bytes) = pkg.and_then(|p| p.read(&a.path).ok()) {
                    match crate::package::video_dimensions(&bytes) {
                        Some((w, h)) if !crate::package::video_aspect_ok(w, h) => {
                            r.err("VIDEO_ASPECT", "visuals.video", format!("the video is {w}×{h}; it must be 16:9 or 4:3"))
                        }
                        Some(_) => {}
                        None => r.warn("VIDEO_FORMAT", "visuals.video", "could not read the video size; it must be 16:9 or 4:3"),
                    }
                }
            }
        }
    }

    // ---- performance ----
    if let Some(pf) = &s.performance {
        if pf.transpose.as_ref().is_some_and(|x| x.min > x.max) || pf.bpm.as_ref().is_some_and(|x| x.min > x.max) {
            r.err("OUT_OF_RANGE", "performance", "range min must not exceed max");
        }
        let mut ids = BTreeSet::new();
        for (i, c) in pf.controls.iter().enumerate() {
            let p = format!("performance.controls[{i}]");
            if !valid_id(&c.id) || !ids.insert(c.id.as_str()) {
                r.err("INVALID_ID", &p, format!("control id {:?} is invalid or duplicated", c.id));
            }
            for (j, tg) in c.targets.iter().enumerate() {
                let parts: Vec<&str> = tg.target.splitn(3, '.').collect();
                let known = match parts.as_slice() {
                    ["patch", id, _] => s.patches.contains_key(*id),
                    ["track", id, _] => track_ids.contains(id),
                    ["fx", owner, _] => *owner == "master" || track_ids.contains(owner),
                    ["overlay", id, _] => s.overlay.as_ref().is_some_and(|o| o.layers.iter().any(|l| l.id == *id)),
                    _ => false,
                };
                if !known {
                    r.err("INVALID_TARGET", format!("{p}.targets[{j}]"), format!("target {:?} does not resolve", tg.target));
                }
            }
        }
    }
    r
}
