//! Score data model (SPEC §7): Project / Track / Layer / Note / Scene, plus the
//! committed timeline of score changes and its materialisation.
//!
//! This crate is pure data: no I/O, no validation error codes (those live in
//! `agb-engine`), no DSP.

pub mod model;
pub mod pitch;
pub mod timeline;

pub use model::*;
pub use pitch::{parse_pitch, Pitch};
pub use timeline::{AutomationLane, AutomationMode, Change, ClipNote, LayerPatch, Snapshot, TimelineEntry, TrackPatch};

/// `specVersion` written by this implementation.
pub const SPEC_VERSION: &str = "0.1.0";

/// JSON Schema for `project.json`.
pub fn project_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Project)).expect("schema serialises")
}
