//! `song.json` data model (FORMAT.md §4–§12).
//!
//! Unknown fields are ignored on purpose (FORMAT.md §3.2): no type here uses
//! `deny_unknown_fields`.

use agb_score::{MasterLimiter, ParamValue, Pitch};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

fn d_requires() -> Vec<String> {
    vec!["core".into()]
}
fn d_ppqn() -> u32 {
    960
}
fn d_bpm() -> f64 {
    120.0
}
fn d_meter() -> (u32, u32) {
    (4, 4)
}
fn d_spq() -> u32 {
    4
}
fn d_a4() -> f64 {
    440.0
}
fn d_one() -> u32 {
    1
}
fn d_vel() -> f64 {
    0.8
}
fn d_true() -> bool {
    true
}
fn d_master_gain() -> f64 {
    -6.0
}
fn d_limiter() -> MasterLimiter {
    MasterLimiter::Soft
}

/// `[tick, value]`
pub type Pt = (u64, f64);

/// The entry document of an `.agbsong` package.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Song {
    /// Always `"agbsong"`.
    pub format: String,
    /// Semver of the format this file was written for.
    pub format_version: String,
    /// Features a player must support to play this file at all.
    #[serde(default = "d_requires")]
    pub requires: Vec<String>,
    /// Features used that a player may skip (and must report).
    #[serde(default)]
    pub uses: Vec<String>,
    pub meta: Meta,
    #[serde(default)]
    pub time: Time,
    #[serde(default)]
    pub tuning: Tuning,
    #[serde(default)]
    pub seed: u64,
    #[serde(default)]
    pub assets: BTreeMap<String, Asset>,
    #[serde(default)]
    pub patches: BTreeMap<String, Patch>,
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub master: Master,
    #[serde(default)]
    pub sections: Vec<Section>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay: Option<Overlay>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visuals: Option<Visuals>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub performance: Option<Performance>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extensions: BTreeMap<String, Value>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub title: String,
    #[serde(default)]
    pub artists: Vec<Artist>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// ISO 8601.
    #[serde(default)]
    pub created: Option<String>,
    /// License of the work: SPDX id or free text.
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub generator: Option<Generator>,
    /// Asset id of the cover image.
    #[serde(default)]
    pub cover: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Artist {
    pub name: String,
    /// `ai` | `human`
    #[serde(default)]
    pub kind: Option<String>,
    /// Model name for AI authors.
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Generator {
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
}

/// FORMAT.md §5. All positions are integer ticks from 0.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Time {
    #[serde(default = "d_ppqn")]
    pub ppqn: u32,
    /// Starting tempo, 20..=400.
    #[serde(default = "d_bpm")]
    pub bpm: f64,
    #[serde(default)]
    pub tempo_map: Vec<TempoPoint>,
    #[serde(default = "d_meter")]
    pub meter: (u32, u32),
    /// Step grid used by loops (4 = sixteenth notes).
    #[serde(default = "d_spq")]
    pub steps_per_quarter: u32,
    #[serde(default)]
    pub length_ticks: Option<u64>,
    #[serde(default, rename = "loop")]
    pub loop_range: Option<TickRange>,
}

impl Default for Time {
    fn default() -> Self {
        serde_json::from_str("{}").expect("all Time fields have defaults")
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TempoPoint {
    pub tick: u64,
    pub bpm: f64,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TickRange {
    pub from_tick: u64,
    pub to_tick: u64,
}

/// FORMAT.md §6.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Tuning {
    #[serde(default = "d_a4")]
    pub a4_hz: f64,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub scale: Option<String>,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning { a4_hz: d_a4(), key: None, scale: None }
    }
}

/// FORMAT.md §8.5.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    /// Path relative to the package root.
    pub path: String,
    /// `sample` | `wavetable` | `image` | `midi` | `shader` | `audio` | `other` (open set).
    #[serde(default)]
    pub kind: Option<String>,
    /// `sha256:<hex>`; verified when present.
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub license: Option<License>,
    #[serde(default)]
    pub root_pitch: Option<Pitch>,
    /// Wavetables: samples per frame (one cycle).
    #[serde(default)]
    pub frame_size: Option<u32>,
    /// Wavetables cut from arbitrary samples: `auto`.
    #[serde(default)]
    pub slice: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct License {
    pub spdx: String,
    #[serde(default)]
    pub redistributable: bool,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// FORMAT.md §8.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Patch {
    /// Engine id; may be omitted when `preset` is given.
    #[serde(default)]
    pub engine: Option<String>,
    #[serde(default)]
    pub engine_version: Option<String>,
    #[serde(default)]
    pub preset: Option<String>,
    /// Parameter path → number or enum string. Order: engine defaults ← preset ← params.
    #[serde(default)]
    pub params: BTreeMap<String, ParamValue>,
    #[serde(default)]
    pub macros: BTreeMap<String, Vec<MacroTarget>>,
    #[serde(default)]
    pub modulation: Vec<Modulation>,
    /// `sampler.multi` zones.
    #[serde(default)]
    pub zones: Vec<Zone>,
    /// `synth.wavetable`: asset id.
    #[serde(default)]
    pub wavetable: Option<String>,
    /// Alternatives tried in order when this patch cannot be played.
    #[serde(default)]
    pub fallback: Vec<Patch>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MacroTarget {
    pub param: String,
    pub min: f64,
    pub max: f64,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Modulation {
    pub source: String,
    pub param: String,
    /// In the parameter's own unit.
    pub amount: f64,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Zone {
    /// Asset id.
    pub sample: String,
    #[serde(default)]
    pub key_range: Option<(u8, u8)>,
    #[serde(default)]
    pub vel_range: Option<(f64, f64)>,
    #[serde(default)]
    pub root_pitch: Option<Pitch>,
    #[serde(default)]
    pub tune_cents: Option<f64>,
    #[serde(default)]
    pub gain_db: Option<f64>,
    #[serde(default)]
    pub pan: Option<f64>,
    #[serde(default)]
    pub one_shot: bool,
    #[serde(default, rename = "loop")]
    pub loop_points: Option<ZoneLoop>,
    #[serde(default)]
    pub start_frame: Option<u64>,
    #[serde(default)]
    pub end_frame: Option<u64>,
    #[serde(default)]
    pub choke_group: Option<String>,
    #[serde(default)]
    pub round_robin_group: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ZoneLoop {
    pub start_frame: u64,
    pub end_frame: u64,
    #[serde(default)]
    pub crossfade_frames: Option<u64>,
}

/// FORMAT.md §7.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    /// Patch id.
    pub patch: String,
    #[serde(default)]
    pub gain_db: f64,
    /// -1 (left) … 1 (right).
    #[serde(default)]
    pub pan: f64,
    #[serde(default)]
    pub mute: bool,
    /// Follow global transposition. Default: false for `drum.*` engines, true otherwise.
    #[serde(default)]
    pub transposable: Option<bool>,
    #[serde(default)]
    pub loops: Vec<Loop>,
    #[serde(default)]
    pub clips: Vec<Clip>,
    #[serde(default)]
    pub automation: Vec<Automation>,
    #[serde(default)]
    pub controllers: Vec<Controller>,
    #[serde(default)]
    pub fx: Vec<Fx>,
}

/// FORMAT.md §7.1 — a pattern cycling on the global step grid.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Loop {
    pub id: String,
    pub length_steps: u32,
    #[serde(default)]
    pub phase_offset_steps: i32,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub gain_db: f64,
    /// Drum shorthand: 0/1 per step, each 1 triggers MIDI 36.
    #[serde(default)]
    pub steps: Option<Vec<u8>>,
    #[serde(default)]
    pub notes: Vec<LoopNote>,
    /// The loop sounds from here (default: song start). Phase stays anchored at tick 0.
    #[serde(default)]
    pub from_tick: Option<u64>,
    /// The loop stops triggering here (default: never).
    #[serde(default)]
    pub to_tick: Option<u64>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LoopNote {
    pub step: u32,
    pub pitch: Pitch,
    #[serde(default = "d_one")]
    pub dur_steps: u32,
    #[serde(default = "d_vel")]
    pub velocity: f64,
    #[serde(default)]
    pub release_velocity: Option<f64>,
    #[serde(default)]
    pub detune_cents: Option<f64>,
    #[serde(default)]
    pub expression: Option<Expression>,
}

/// FORMAT.md §7.2 — notes at ticks relative to `atTick`.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    #[serde(default)]
    pub at_tick: u64,
    #[serde(default)]
    pub length_ticks: Option<u64>,
    #[serde(default = "d_one")]
    pub repeat: u32,
    #[serde(default)]
    pub notes: Vec<ClipNote>,
    /// Take the notes from a Standard MIDI File in the package instead.
    #[serde(default)]
    pub midi: Option<MidiRef>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClipNote {
    pub tick: u64,
    pub pitch: Pitch,
    pub dur_ticks: u64,
    #[serde(default = "d_vel")]
    pub velocity: f64,
    #[serde(default)]
    pub release_velocity: Option<f64>,
    #[serde(default)]
    pub detune_cents: Option<f64>,
    #[serde(default)]
    pub expression: Option<Expression>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MidiRef {
    pub asset: String,
    #[serde(default)]
    pub track: Option<u32>,
}

/// FORMAT.md §7.4 — per-note curves; ticks are relative to the note start.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Expression {
    /// Semitones.
    #[serde(default)]
    pub bend: Vec<Pt>,
    /// 0..=1
    #[serde(default)]
    pub pressure: Vec<Pt>,
    /// 0..=1
    #[serde(default)]
    pub timbre: Vec<Pt>,
}

/// Track-wide controller lane (absolute ticks).
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Controller {
    /// `pitchBend` | `modWheel` | `expression` | `sustain` | `aftertouch`
    #[serde(default)]
    pub name: Option<String>,
    /// MIDI controller number, as an alternative to `name`.
    #[serde(default)]
    pub cc: Option<u8>,
    pub points: Vec<Pt>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Curve {
    #[default]
    Linear,
    Step,
}

/// `[tick, value]` or `[tick, value, curve]`; the curve describes the segment
/// arriving at this point.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum Point {
    Curved(u64, f64, Curve),
    Plain(u64, f64),
}

impl Point {
    pub fn tick(&self) -> u64 {
        match self {
            Point::Curved(t, _, _) | Point::Plain(t, _) => *t,
        }
    }
    pub fn value(&self) -> f64 {
        match self {
            Point::Curved(_, v, _) | Point::Plain(_, v) => *v,
        }
    }
    pub fn curve(&self) -> Curve {
        match self {
            Point::Curved(_, _, c) => *c,
            Point::Plain(..) => Curve::Linear,
        }
    }
}

/// FORMAT.md §7.5.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Automation {
    /// `patch.<param>` | `track.gainDb` | `track.pan` | `fx.<fxId>.<param>` | `master.gainDb`
    pub target: String,
    pub points: Vec<Point>,
}

/// FORMAT.md §9.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Fx {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub fx_type: String,
    #[serde(default)]
    pub params: BTreeMap<String, Value>,
    #[serde(default)]
    pub bypass: bool,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Master {
    #[serde(default = "d_master_gain")]
    pub gain_db: f64,
    #[serde(default = "d_limiter")]
    pub limiter: MasterLimiter,
    #[serde(default)]
    pub fx: Vec<Fx>,
    #[serde(default)]
    pub automation: Vec<Automation>,
}

impl Default for Master {
    fn default() -> Self {
        Master { gain_db: d_master_gain(), limiter: d_limiter(), fx: vec![], automation: vec![] }
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub from_tick: u64,
    pub to_tick: u64,
    /// One sentence for people and AI performers: what this section is for.
    #[serde(default)]
    pub intent: Option<String>,
    /// Per-section overrides of the top-level `visuals`.
    #[serde(default)]
    pub visuals: Option<Value>,
}

/// FORMAT.md §10 — frequency layers on top of the music.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Overlay {
    #[serde(default)]
    pub layers: Vec<OverlayLayer>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OverlayLayer {
    pub id: String,
    /// `tone` | `binaural` | `isochronic` | `noise` (open set).
    pub kind: String,
    #[serde(default)]
    pub hz: Option<f64>,
    #[serde(default)]
    pub wave: Option<String>,
    #[serde(default)]
    pub carrier_hz: Option<f64>,
    #[serde(default)]
    pub beat_hz: Option<f64>,
    #[serde(default)]
    pub pulse_hz: Option<f64>,
    /// `white` | `pink` | `brown`
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub gain_db: Option<f64>,
    #[serde(default = "d_true")]
    pub enabled: bool,
    #[serde(default)]
    pub pan: Option<f64>,
    #[serde(default)]
    pub fade_in_ms: Option<f64>,
    #[serde(default)]
    pub fade_out_ms: Option<f64>,
    /// `[tick, gainDb]` level changes over the song.
    #[serde(default)]
    pub schedule: Vec<Pt>,
}

/// FORMAT.md §12.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Visuals {
    #[serde(default)]
    pub palette: Vec<String>,
    #[serde(default)]
    pub scene: Option<String>,
    #[serde(default)]
    pub params: BTreeMap<String, Value>,
    #[serde(default)]
    pub bindings: Vec<Binding>,
    /// Asset id of an optional fragment shader.
    #[serde(default)]
    pub shader: Option<String>,
    /// A video shipped with the song (e.g. rendered by the authoring agent).
    #[serde(default)]
    pub video: Option<Video>,
}

/// FORMAT.md §12.1 — a pre-rendered video. Its own audio track is never played.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Video {
    /// Asset id (`kind: "video"`).
    pub asset: String,
    /// `loop` (default): play in a loop, independent of the song position.
    /// `song`: video time follows song time (bar 1 = start of the video).
    #[serde(default)]
    pub sync: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub scale: Option<f64>,
}

/// FORMAT.md §11.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Performance {
    /// Suggested transposition range in semitones.
    #[serde(default)]
    pub transpose: Option<IntRange>,
    #[serde(default)]
    pub bpm: Option<NumRange>,
    #[serde(default)]
    pub controls: Vec<Control>,
    #[serde(default)]
    pub agent: Option<Agent>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
pub struct IntRange {
    pub min: i32,
    pub max: i32,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
pub struct NumRange {
    pub min: f64,
    pub max: f64,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Control {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    /// 0..=1
    #[serde(default)]
    pub default: Option<f64>,
    pub targets: Vec<ControlTarget>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ControlTarget {
    /// `patch.<patchId>.<param>` | `track.<trackId>.gainDb` |
    /// `fx.<trackId|master>.<fxId>.<param>` | `overlay.<layerId>.gainDb`
    pub target: String,
    pub min: f64,
    pub max: f64,
}

/// Brief for an AI performer taking over the piece. Untrusted text.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    #[serde(default)]
    pub brief: Option<String>,
    #[serde(default)]
    pub persona: Option<String>,
    #[serde(default)]
    pub rules: Vec<String>,
    /// What the AI may change: `notes`, `patches`, `fx`, `tempo`, `overlay`, …
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub sections_are_fixed: bool,
}
