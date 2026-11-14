//! AGB Song (`.agbsong`) — the interchange file format (see `FORMAT.md`).
//!
//! * [`model`]    — `song.json` types (tolerant of unknown fields)
//! * [`package`]  — directory / zip container, `$ref`, pack / unpack
//! * [`validate`] — format-level validation, independent of this player
//! * [`lower`]    — conversion to an engine [`agb_score::Project`] for the part
//!   of the format this build can play, with a report of what was skipped

pub mod lower;
pub mod model;
pub mod package;
pub mod validate;

pub use lower::{lower, Lowered, PlayOptions, PLAYER_FEATURES};
pub use model::Song;
pub use package::{pack, unpack, Package, MIMETYPE, SONG_FILE};
pub use validate::{validate, Issue, Report};

/// Value of `format` in `song.json`.
pub const FORMAT_NAME: &str = "agbsong";
/// Format version written (and read) by this implementation.
pub const FORMAT_VERSION: &str = "0.1.0";

/// JSON Schema for `song.json`.
pub fn song_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Song)).expect("schema serialises")
}

/// A small playable song used by `agb song new`.
pub fn template(title: &str) -> serde_json::Value {
    serde_json::json!({
        "format": FORMAT_NAME,
        "formatVersion": FORMAT_VERSION,
        "requires": ["core"],
        "uses": [],
        "meta": {
            "title": title,
            "artists": [],
            "generator": { "name": "agb", "version": env!("CARGO_PKG_VERSION") }
        },
        "time": { "ppqn": 960, "bpm": 100, "stepsPerQuarter": 4 },
        "tuning": { "a4Hz": 440, "key": "A", "scale": "minor" },
        "patches": {
            "kit": { "preset": "preset:drum.analog808style:drums:classic_kit@v1.0" },
            "bass": { "engine": "synth.waveform", "params": { "osc.wave": "saw", "amp.releaseMs": 120 } }
        },
        "tracks": [
            { "id": "drums", "role": "drums", "patch": "kit",
              "loops": [{ "id": "kick", "lengthSteps": 16,
                          "steps": [1,0,0,0, 1,0,0,0, 1,0,0,0, 1,0,0,0] }] },
            { "id": "bass", "role": "bass", "patch": "bass", "gainDb": -6,
              "loops": [{ "id": "riff", "lengthSteps": 12, "notes": [
                  { "step": 0, "pitch": "A1", "durSteps": 3 },
                  { "step": 6, "pitch": "E2", "durSteps": 2 },
                  { "step": 9, "pitch": "G1", "durSteps": 2 }
              ] }] }
        ]
    })
}
