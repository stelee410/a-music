//! SPEC §7 data model. Field names are camelCase in JSON.

use crate::pitch::Pitch;
use crate::timeline::TimelineEntry;
use agb_assets::{AssetRef, LicenseRecord};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn d_spec() -> String {
    crate::SPEC_VERSION.to_string()
}
fn d_name() -> String {
    "untitled".into()
}
fn d_tempo() -> f64 {
    120.0
}
fn d_ppqn() -> u32 {
    960
}
fn d_spq() -> u32 {
    2
}
fn d_sr() -> u32 {
    48000
}
fn d_master() -> f64 {
    -6.0
}
fn d_engine() -> String {
    "synth.waveform".into()
}
fn d_len() -> u32 {
    8
}
fn d_grid() -> String {
    "8th".into()
}
fn d_dur() -> u32 {
    1
}
fn d_vel() -> f64 {
    0.8
}
fn is_false(b: &bool) -> bool {
    !*b
}
fn is_zero(v: &i32) -> bool {
    *v == 0
}

/// Voice steal policy (§7.1).
#[derive(Serialize, Deserialize, JsonSchema, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum StealPolicy {
    #[default]
    Oldest,
    Quietest,
    SamePitch,
}

/// Master bus limiter.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MasterLimiter {
    /// Hard clip at 0 dBFS (P0 behaviour).
    #[default]
    Hard,
    /// Soft knee above −3 dBFS, then hard clip at 0 dBFS.
    Soft,
}

/// Scene phase policy (§5.4).
#[derive(Serialize, Deserialize, JsonSchema, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PhasePolicy {
    #[default]
    HardResetOnScene,
    FreePhase,
}

/// A synth parameter value: number or enum string (e.g. `osc.wave: "saw"`).
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum ParamValue {
    Num(f64),
    Str(String),
}

/// `project.json` (§7.1).
///
/// Implementation extensions (documented in README): `transportTick`, `panic`,
/// `timeline`. `tracks` / `scenes` are the materialised **head** state; the
/// authoritative score history is `timeline` (changes keyed by effective tick).
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    #[serde(default = "d_spec")]
    pub spec_version: String,
    #[serde(default = "d_name")]
    pub name: String,
    #[serde(default = "d_tempo")]
    pub tempo_bpm: f64,
    #[serde(default = "d_ppqn")]
    pub ppqn: u32,
    #[serde(default = "d_spq")]
    pub steps_per_quarter: u32,
    #[serde(default = "d_sr")]
    pub sample_rate: u32,
    #[serde(default)]
    pub origin_tick: u64,
    #[serde(default)]
    pub revision: u64,
    #[serde(default = "d_master")]
    pub master_gain_db: f64,
    #[serde(default)]
    pub steal_policy: StealPolicy,
    /// Extension: tempo changes after the origin (`tempoBpm` is the starting tempo).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tempo_map: Vec<TempoPoint>,
    /// Extension: transposition in semitones in force from the origin, applied
    /// at scheduling time to every transposable track.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub transpose: i32,
    /// Extension: later transposition changes (`project.setTranspose`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transpose_map: Vec<TransposePoint>,
    /// Extension: master bus limiter mode.
    #[serde(default)]
    pub master_limiter: MasterLimiter,
    #[serde(default)]
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub scenes: Vec<Scene>,
    #[serde(default)]
    pub assets: Vec<AssetRef>,
    #[serde(default)]
    pub licenses_registry: BTreeMap<String, LicenseRecord>,
    #[serde(default)]
    pub seed: u64,

    // ---- implementation extensions ----
    /// Logical transport position used for `latePolicy` decisions.
    #[serde(default)]
    pub transport_tick: u64,
    #[serde(default)]
    pub panic: PanicState,
    /// Committed score changes, ordered by `(effectiveTick, revision)` when materialised.
    #[serde(default)]
    pub timeline: Vec<TimelineEntry>,
}

impl Default for Project {
    fn default() -> Self {
        serde_json::from_str("{}").expect("all project fields have defaults")
    }
}

/// Tempo change taking effect at `tick` (bar-aligned by convention).
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TempoPoint {
    pub tick: u64,
    pub bpm: f64,
}

/// Transposition taking effect at `tick`: absolute semitones relative to the written score.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TransposePoint {
    pub tick: u64,
    pub semitones: i32,
}

impl Project {
    /// Transposition (semitones) in force at `tick`.
    pub fn transpose_at(&self, tick: u64) -> i32 {
        self.transpose_map.iter().rev().find(|t| t.tick <= tick).map_or(self.transpose, |t| t.semitones)
    }
}

/// `system.panic` state.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PanicState {
    #[serde(default)]
    pub active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_revision: Option<u64>,
}

/// §7.2
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: String,
    #[serde(default = "d_engine")]
    pub engine_id: String,
    #[serde(default)]
    pub preset_id: Option<String>,
    #[serde(default)]
    pub channel: u8,
    #[serde(default)]
    pub mute: bool,
    #[serde(default)]
    pub gain_db: f64,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub layers: Vec<Layer>,
    /// P0: kept but not processed.
    #[serde(default)]
    pub fx: Vec<serde_json::Value>,
    /// Extension: per-track engine parameter overrides (`osc.wave`, `amp.attackMs`, …).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, ParamValue>,
    /// Extension: follow the project transposition. Default: every engine except `drum.*`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transposable: Option<bool>,
}

impl Track {
    pub fn follows_transpose(&self) -> bool {
        self.transposable.unwrap_or(!self.engine_id.starts_with("drum."))
    }
}

impl Track {
    pub fn new(id: impl Into<String>) -> Self {
        Track {
            id: id.into(),
            engine_id: d_engine(),
            preset_id: None,
            channel: 0,
            mute: false,
            gain_db: 0.0,
            owner: None,
            layers: vec![],
            fx: vec![],
            params: BTreeMap::new(),
            transposable: None,
        }
    }
}

/// §7.3
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Layer {
    pub id: String,
    #[serde(default = "d_len")]
    pub length_steps: u32,
    #[serde(default)]
    pub phase_offset_steps: i32,
    #[serde(default = "d_grid")]
    pub grid: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub mute: bool,
    #[serde(default)]
    pub gain_db: f64,
    /// Drum triggers 0/1, length == `lengthSteps`. Used only when `notes` is empty.
    #[serde(default)]
    pub steps: Option<Vec<u8>>,
    /// Pitched events. Take precedence over `steps` when non-empty.
    #[serde(default)]
    pub notes: Option<Vec<Note>>,
}

impl Layer {
    pub fn new(id: impl Into<String>) -> Self {
        Layer {
            id: id.into(),
            length_steps: d_len(),
            phase_offset_steps: 0,
            grid: d_grid(),
            mute: false,
            gain_db: 0.0,
            steps: None,
            notes: None,
        }
    }
}

/// Note inside a layer.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Note {
    pub step: u32,
    pub pitch: Pitch,
    #[serde(default = "d_dur")]
    pub dur_steps: u32,
    #[serde(default = "d_vel")]
    pub velocity: f64,
    /// What this note sings, on a track with a singing voice (`voice.chimera`): a word or syllable,
    /// `+` for the next syllable of the word before, `-` to carry the vowel on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lyric: Option<String>,
}

/// §7.4
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Scene {
    pub id: String,
    /// Absolute tick at which the scene launches; `null` = defined but not scheduled.
    #[serde(default)]
    pub launch_at_tick: Option<u64>,
    #[serde(default)]
    pub phase_policy: PhasePolicy,
    /// trackId → presetId override, effective from the launch tick.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub track_patches: BTreeMap<String, String>,
}
