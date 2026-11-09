//! Control-plane protocol (SPEC §8): transaction envelope, ops, responses and
//! stable error codes. Any future HTTP / MCP adapter must reuse these types.

use agb_score::{AutomationMode, ClipNote, LayerPatch, MasterLimiter, ParamValue, PhasePolicy, TrackPatch};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Envelope
// ---------------------------------------------------------------------------

/// §8.2 Transaction envelope.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Transaction {
    /// Idempotency key (UUID or stable string).
    pub command_id: String,
    /// `agent.*` or `human.*`.
    pub actor: String,
    /// When present, must equal the current project revision.
    #[serde(default)]
    pub if_match_revision: Option<u64>,
    #[serde(default)]
    pub dry_run: bool,
    pub quantize: Quantize,
    #[serde(default)]
    pub late_policy: LatePolicy,
    /// Atomic batch: either every op is applied or none.
    pub ops: Vec<Op>,
}

/// §8.2 Quantize — the tick at which the transaction takes effect.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Quantize {
    Tick { tick: u64 },
    NextBar {},
    /// Only for `system.panic*` and read-only ops; score writes are rejected.
    Immediate {},
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LatePolicy {
    #[default]
    Reject,
    ShiftToNextLegal,
    ApplyAsapWithFlag,
}

// ---------------------------------------------------------------------------
// Ops (§8.5)
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(tag = "op")]
pub enum Op {
    // ---- P0 ----
    #[serde(rename = "project.setTempo")]
    ProjectSetTempo(SetTempo),
    #[serde(rename = "project.setMix")]
    ProjectSetMix(SetMix),
    #[serde(rename = "track.upsert")]
    TrackUpsert(TrackPatch),
    #[serde(rename = "layer.upsert")]
    LayerUpsert(LayerPatch),
    #[serde(rename = "scene.define")]
    SceneDefine(SceneDefine),
    #[serde(rename = "scene.launch")]
    SceneLaunch(SceneLaunch),
    #[serde(rename = "render.offline")]
    RenderOffline(RenderOffline),
    #[serde(rename = "system.panic")]
    SystemPanic(Empty),
    #[serde(rename = "system.panic.clear")]
    SystemPanicClear(Empty),

    // ---- P1 ----
    #[serde(rename = "clip.schedule")]
    ClipSchedule(ClipSchedule),
    #[serde(rename = "preset.search")]
    PresetSearch(PresetSearch),
    #[serde(rename = "preset.load")]
    PresetLoad(PresetLoad),
    #[serde(rename = "voice.setParams")]
    VoiceSetParams(VoiceSetParams),
    #[serde(rename = "automation.schedule")]
    AutomationSchedule(AutomationSchedule),
}

impl Op {
    pub fn name(&self) -> &'static str {
        match self {
            Op::ProjectSetTempo(_) => "project.setTempo",
            Op::ProjectSetMix(_) => "project.setMix",
            Op::TrackUpsert(_) => "track.upsert",
            Op::LayerUpsert(_) => "layer.upsert",
            Op::SceneDefine(_) => "scene.define",
            Op::SceneLaunch(_) => "scene.launch",
            Op::RenderOffline(_) => "render.offline",
            Op::SystemPanic(_) => "system.panic",
            Op::SystemPanicClear(_) => "system.panic.clear",
            Op::ClipSchedule(_) => "clip.schedule",
            Op::PresetSearch(_) => "preset.search",
            Op::PresetLoad(_) => "preset.load",
            Op::VoiceSetParams(_) => "voice.setParams",
            Op::AutomationSchedule(_) => "automation.schedule",
        }
    }

    /// Phase in which the op becomes available, if not P0.
    pub fn planned_phase(&self) -> Option<&'static str> {
        // All ops of the spec table are implemented as of P1.
        None
    }

    /// Ops that modify the score (subject to quantize / late policy).
    pub fn writes_score(&self) -> bool {
        matches!(
            self,
            Op::ProjectSetTempo(_)
                | Op::ProjectSetMix(_)
                | Op::TrackUpsert(_)
                | Op::LayerUpsert(_)
                | Op::SceneDefine(_)
                | Op::SceneLaunch(_)
                | Op::ClipSchedule(_)
                | Op::PresetLoad(_)
                | Op::VoiceSetParams(_)
                | Op::AutomationSchedule(_)
        )
    }

    pub fn is_panic(&self) -> bool {
        matches!(self, Op::SystemPanic(_) | Op::SystemPanicClear(_))
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetTempo {
    /// 20..=400. Tempo is global (no tempo map).
    pub tempo_bpm: f64,
}

/// Extension: project-level mix settings.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetMix {
    #[serde(default)]
    pub master_gain_db: Option<f64>,
    #[serde(default)]
    pub master_limiter: Option<MasterLimiter>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SceneDefine {
    pub scene_id: String,
    /// Absolute, step-aligned tick. Omit to define without scheduling.
    #[serde(default)]
    pub launch_at_tick: Option<u64>,
    #[serde(default)]
    pub phase_policy: Option<PhasePolicy>,
    /// trackId → presetId, effective from launch.
    #[serde(default)]
    pub track_patches: Option<BTreeMap<String, String>>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SceneLaunch {
    pub scene_id: String,
    /// Defaults to the transaction's effective tick.
    #[serde(default)]
    pub at_tick: Option<u64>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SampleFormat {
    /// 16-bit PCM, round-half-away-from-zero of `x * 32767` after clamping to [-1, 1].
    #[default]
    Pcm16,
    /// 32-bit IEEE float.
    F32,
}

/// §10.1 offline render request (txn op form).
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderOffline {
    #[serde(default)]
    pub from_tick: Option<u64>,
    /// Defaults to one full polymeter cycle (LCM of all layer lengths).
    #[serde(default)]
    pub to_tick: Option<u64>,
    #[serde(default)]
    pub tail_ms: Option<u32>,
    /// Defaults to the project seed.
    #[serde(default)]
    pub seed: Option<u64>,
    #[serde(default)]
    pub stems: bool,
    /// Relative to the project root.
    pub out_dir: String,
    #[serde(default)]
    pub format: Option<SampleFormat>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClipSchedule {
    pub track_id: String,
    pub notes: Vec<ClipNote>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PresetSearch {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub engine: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TailPolicy {
    #[default]
    ReleaseOld,
    CutOld,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PresetLoad {
    pub track_id: String,
    pub preset_id: String,
    #[serde(default)]
    pub tail_policy: TailPolicy,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VoiceSetParams {
    pub track_id: String,
    pub params: BTreeMap<String, ParamValue>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationSchedule {
    pub track_id: String,
    pub param: String,
    pub mode: AutomationMode,
    pub from_tick: u64,
    pub to_tick: u64,
    pub from: f64,
    pub to: f64,
}

// ---------------------------------------------------------------------------
// Responses & errors (§8.3, §8.4)
// ---------------------------------------------------------------------------

/// §8.3 success response.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TxnResponse {
    pub ok: bool,
    pub command_id: String,
    /// Project revision after the transaction (unchanged for dry runs).
    pub revision: u64,
    pub effective_tick: u64,
    pub idempotent_replay: bool,
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dry_run: bool,
    /// Per-op results (e.g. render summaries); omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub results: Vec<serde_json::Value>,
}

/// §8.4 error codes (stable). Codes after `UnsupportedInPhase` are
/// implementation additions for local failure modes.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    InvalidJson,
    SchemaViolation,
    RevisionConflict,
    IdempotencyPayloadMismatch,
    LateRejected,
    UnknownTrack,
    UnknownPreset,
    UnknownEngine,
    AssetMissing,
    LicenseForbidden,
    BudgetExceeded,
    PanicActive,
    UnsupportedInPhase,
    // ---- additions ----
    /// `project.json` missing (for read commands) or unreadable.
    ProjectNotFound,
    /// `specVersion` of the project is not supported by this build (§14).
    SpecVersionUnsupported,
    /// Another process holds the project lock.
    ProjectLocked,
    /// Filesystem error.
    IoError,
}

impl ErrorCode {
    pub fn as_str(&self) -> String {
        serde_json::to_value(self).unwrap().as_str().unwrap().to_string()
    }
}

/// Error carried through all crates and serialised as the error response.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgbError {
    pub code: ErrorCode,
    pub message: String,
    /// Index into `ops` of the offending op, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op_index: Option<usize>,
    /// Current project revision (always present on `REVISION_CONFLICT`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_revision: Option<u64>,
}

impl AgbError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        AgbError { code, message: message.into(), op_index: None, server_revision: None }
    }
    pub fn at_op(mut self, i: usize) -> Self {
        if self.op_index.is_none() {
            self.op_index = Some(i);
        }
        self
    }
    pub fn io(e: impl std::fmt::Display) -> Self {
        AgbError::new(ErrorCode::IoError, e.to_string())
    }
}

impl std::fmt::Display for AgbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for AgbError {}

/// Error response envelope.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ErrorResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_id: Option<String>,
    pub error: AgbError,
    /// Mirrors `error.serverRevision` at top level for `REVISION_CONFLICT` (§8.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_revision: Option<u64>,
}

impl ErrorResponse {
    pub fn new(command_id: Option<String>, error: AgbError) -> Self {
        let server_revision = error.server_revision;
        ErrorResponse { ok: false, command_id, error, server_revision }
    }
}

/// Parse raw JSON into a [`Transaction`], distinguishing `INVALID_JSON` from
/// `SCHEMA_VIOLATION`. Returns the parsed `Value` too (used for payload hashing).
pub fn parse_transaction(raw: &str) -> Result<(serde_json::Value, Transaction), AgbError> {
    let v: serde_json::Value = serde_json::from_str(raw)
        .map_err(|e| AgbError::new(ErrorCode::InvalidJson, format!("invalid JSON: {e}")))?;
    let t = parse_transaction_value(&v)?;
    Ok((v, t))
}

pub fn parse_transaction_value(v: &serde_json::Value) -> Result<Transaction, AgbError> {
    // Locate a failing op for a better message.
    if let Some(ops) = v.get("ops").and_then(|o| o.as_array()) {
        for (i, op) in ops.iter().enumerate() {
            if let Err(e) = serde_json::from_value::<Op>(op.clone()) {
                let name = op.get("op").and_then(|n| n.as_str()).unwrap_or("?");
                return Err(AgbError::new(
                    ErrorCode::SchemaViolation,
                    format!("ops[{i}] ({name}): {e}"),
                )
                .at_op(i));
            }
        }
    }
    serde_json::from_value(v.clone())
        .map_err(|e| AgbError::new(ErrorCode::SchemaViolation, format!("transaction: {e}")))
}

/// JSON Schema of the transaction envelope.
pub fn transaction_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Transaction)).expect("schema serialises")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_serialise() {
        assert_eq!(ErrorCode::RevisionConflict.as_str(), "REVISION_CONFLICT");
        assert_eq!(ErrorCode::UnsupportedInPhase.as_str(), "UNSUPPORTED_IN_PHASE");
        assert_eq!(ErrorCode::IdempotencyPayloadMismatch.as_str(), "IDEMPOTENCY_PAYLOAD_MISMATCH");
    }

    #[test]
    fn invalid_json_vs_schema() {
        assert_eq!(parse_transaction("{").unwrap_err().code, ErrorCode::InvalidJson);
        assert_eq!(parse_transaction("{}").unwrap_err().code, ErrorCode::SchemaViolation);
        let bad_op = r#"{"commandId":"a","actor":"agent.x","quantize":{"type":"tick","tick":0},
            "ops":[{"op":"track.upsert","trackId":"t"},{"op":"track.upsert","trackId":"t","bogus":1}]}"#;
        let e = parse_transaction(bad_op).unwrap_err();
        assert_eq!(e.code, ErrorCode::SchemaViolation);
        assert_eq!(e.op_index, Some(1));
        let unknown = r#"{"commandId":"a","actor":"agent.x","quantize":{"type":"tick","tick":0},
            "ops":[{"op":"nope.nope"}]}"#;
        assert_eq!(parse_transaction(unknown).unwrap_err().code, ErrorCode::SchemaViolation);
    }

    #[test]
    fn quantize_forms() {
        let q: Quantize = serde_json::from_str(r#"{"type":"next_bar"}"#).unwrap();
        assert_eq!(q, Quantize::NextBar {});
        let q: Quantize = serde_json::from_str(r#"{"type":"immediate"}"#).unwrap();
        assert_eq!(q, Quantize::Immediate {});
        let q: Quantize = serde_json::from_str(r#"{"type":"tick","tick":280}"#).unwrap();
        assert_eq!(q, Quantize::Tick { tick: 280 });
    }

    #[test]
    fn panic_op_without_fields() {
        let op: Op = serde_json::from_str(r#"{"op":"system.panic"}"#).unwrap();
        assert!(op.is_panic());
    }
}
