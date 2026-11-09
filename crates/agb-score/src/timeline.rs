//! Committed score timeline.
//!
//! Every successful write transaction appends one [`TimelineEntry`] holding the
//! score [`Change`]s, keyed by the transaction's effective (quantized) tick.
//! The score in force at tick `T` is obtained by applying all entries with
//! `effectiveTick <= T` in `(effectiveTick, revision)` order onto an empty
//! [`Snapshot`]. Render only ever reads snapshots (SPEC §4 rule 2).

use crate::model::{Layer, Note, ParamValue, Track};
use crate::pitch::Pitch;
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;

/// Distinguish "field absent" (`None`) from explicit `null` (`Some(None)`).
fn double_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

/// Partial track update (payload of `track.upsert`). Absent fields keep their value.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrackPatch {
    pub track_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_id: Option<String>,
    /// Explicit `null` clears the preset.
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Option<String>")]
    pub preset_id: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mute: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gain_db: Option<f64>,
    #[serde(
        default,
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Option<String>")]
    pub owner: Option<Option<String>>,
    /// Extension: engine parameter overrides; merged key by key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<BTreeMap<String, ParamValue>>,
    /// Extension: when true, `params` replaces the whole override map
    /// (an absent `params` clears it). Used by `preset.load`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub replace_params: bool,
}

/// Partial layer update (payload of `layer.upsert`).
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayerPatch {
    pub track_id: String,
    pub layer_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length_steps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_offset_steps: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mute: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gain_db: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<Vec<Note>>,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationMode {
    /// Linear `from`→`to` over `[fromTick, toTick]`, then holds `to`.
    Ramp,
    /// `from` during `[fromTick, toTick)`, then `to`.
    Step,
}

/// One automation lane on a track parameter.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationLane {
    pub track_id: String,
    pub param: String,
    pub mode: AutomationMode,
    pub from_tick: u64,
    pub to_tick: u64,
    pub from: f64,
    pub to: f64,
}

impl AutomationLane {
    /// Value at `tick`, or `None` before the lane starts.
    pub fn value_at(&self, tick: f64) -> Option<f64> {
        let (a, b) = (self.from_tick as f64, self.to_tick as f64);
        if tick < a {
            return None;
        }
        Some(match self.mode {
            AutomationMode::Step => {
                if tick < b {
                    self.from
                } else {
                    self.to
                }
            }
            AutomationMode::Ramp => {
                if tick >= b || b <= a {
                    self.to
                } else {
                    self.from + (self.to - self.from) * (tick - a) / (b - a)
                }
            }
        })
    }
}

/// Note at an absolute tick (`clip.schedule`).
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClipNote {
    pub tick: u64,
    pub pitch: Pitch,
    pub dur_ticks: u64,
    #[serde(default)]
    pub velocity: Option<f64>,
}

/// One score change stored in the timeline.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Change {
    /// Merge into (or create) a track.
    UpsertTrack { patch: TrackPatch },
    /// Merge into (or create) a layer of an existing track.
    UpsertLayer { patch: LayerPatch },
    /// Replace a whole track (used for genesis of hand-written projects).
    PutTrack { track: Track },
    /// Parameter automation lane (carries its own tick range).
    Automation { lane: AutomationLane },
    /// Absolute-tick notes on a track.
    Clip { track_id: String, notes: Vec<ClipNote> },
    /// Hard-stop the track's sounding voices at the entry's effective tick
    /// (`preset.load` with `tailPolicy: cut_old`).
    Cut { track_id: String },
}

/// A committed write transaction's score changes.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEntry {
    pub revision: u64,
    pub command_id: String,
    pub effective_tick: u64,
    pub changes: Vec<Change>,
}

/// Score state in force at some tick.
#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct Snapshot {
    pub tracks: Vec<Track>,
}

impl Snapshot {
    pub fn track(&self, id: &str) -> Option<&Track> {
        self.tracks.iter().find(|t| t.id == id)
    }

    /// Apply one change. `UpsertLayer` on a missing track is ignored (the
    /// engine rejects such transactions with `UNKNOWN_TRACK` before commit).
    pub fn apply(&mut self, change: &Change) {
        match change {
            Change::PutTrack { track } => {
                if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track.id) {
                    *t = track.clone();
                } else {
                    self.tracks.push(track.clone());
                }
            }
            Change::UpsertTrack { patch } => {
                let idx = match self.tracks.iter().position(|t| t.id == patch.track_id) {
                    Some(i) => i,
                    None => {
                        self.tracks.push(Track::new(patch.track_id.clone()));
                        self.tracks.len() - 1
                    }
                };
                let t = &mut self.tracks[idx];
                if let Some(v) = &patch.engine_id {
                    t.engine_id = v.clone();
                }
                if let Some(v) = &patch.preset_id {
                    t.preset_id = v.clone();
                }
                if let Some(v) = patch.channel {
                    t.channel = v;
                }
                if let Some(v) = patch.mute {
                    t.mute = v;
                }
                if let Some(v) = patch.gain_db {
                    t.gain_db = v;
                }
                if let Some(v) = &patch.owner {
                    t.owner = v.clone();
                }
                if patch.replace_params {
                    t.params = patch.params.clone().unwrap_or_default();
                } else if let Some(p) = &patch.params {
                    for (k, v) in p {
                        t.params.insert(k.clone(), v.clone());
                    }
                }
            }
            Change::UpsertLayer { patch } => {
                let Some(t) = self.tracks.iter_mut().find(|t| t.id == patch.track_id) else {
                    return;
                };
                let idx = match t.layers.iter().position(|l| l.id == patch.layer_id) {
                    Some(i) => i,
                    None => {
                        t.layers.push(Layer::new(patch.layer_id.clone()));
                        t.layers.len() - 1
                    }
                };
                let l = &mut t.layers[idx];
                if let Some(v) = patch.length_steps {
                    l.length_steps = v;
                }
                if let Some(v) = patch.phase_offset_steps {
                    l.phase_offset_steps = v;
                }
                if let Some(v) = &patch.grid {
                    l.grid = v.clone();
                }
                if let Some(v) = patch.mute {
                    l.mute = v;
                }
                if let Some(v) = patch.gain_db {
                    l.gain_db = v;
                }
                if let Some(v) = &patch.steps {
                    l.steps = Some(v.clone());
                }
                if let Some(v) = &patch.notes {
                    l.notes = Some(v.clone());
                }
            }
            // Time-addressed changes do not alter track state.
            Change::Automation { .. } | Change::Clip { .. } | Change::Cut { .. } => {}
        }
    }
}

/// Timeline entries sorted in application order `(effectiveTick, revision)`.
pub fn ordered(entries: &[TimelineEntry]) -> Vec<&TimelineEntry> {
    let mut v: Vec<&TimelineEntry> = entries.iter().collect();
    v.sort_by_key(|e| (e.effective_tick, e.revision));
    v
}

/// Score state in force at `tick` (inclusive).
pub fn snapshot_at(entries: &[TimelineEntry], tick: u64) -> Snapshot {
    let mut s = Snapshot::default();
    for e in ordered(entries) {
        if e.effective_tick > tick {
            break;
        }
        for c in &e.changes {
            s.apply(c);
        }
    }
    s
}

/// Final state after all entries.
pub fn head(entries: &[TimelineEntry]) -> Snapshot {
    snapshot_at(entries, u64::MAX)
}

/// Incremental cursor: walks forward in time applying entries as their tick is reached.
pub struct Cursor<'a> {
    entries: Vec<&'a TimelineEntry>,
    next: usize,
    pub snapshot: Snapshot,
}

impl<'a> Cursor<'a> {
    pub fn new(entries: &'a [TimelineEntry]) -> Self {
        Cursor { entries: ordered(entries), next: 0, snapshot: Snapshot::default() }
    }

    /// Advance to `tick` (monotonic). Returns true if the snapshot changed.
    pub fn advance_to(&mut self, tick: u64) -> bool {
        let mut changed = false;
        while self.next < self.entries.len() && self.entries[self.next].effective_tick <= tick {
            for c in &self.entries[self.next].changes {
                self.snapshot.apply(c);
            }
            self.next += 1;
            changed = true;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer_len(s: &Snapshot, t: &str, l: &str) -> Option<u32> {
        s.track(t)?.layers.iter().find(|x| x.id == l).map(|x| x.length_steps)
    }

    #[test]
    fn time_ordered_materialisation() {
        let mk = |rev, tick, len| TimelineEntry {
            revision: rev,
            command_id: format!("c{rev}"),
            effective_tick: tick,
            changes: vec![
                Change::UpsertTrack { patch: TrackPatch { track_id: "a".into(), ..Default::default() } },
                Change::UpsertLayer {
                    patch: LayerPatch {
                        track_id: "a".into(),
                        layer_id: "l".into(),
                        length_steps: Some(len),
                        ..Default::default()
                    },
                },
            ],
        };
        // rev 1 at tick 1000 (len 5), rev 2 at tick 0 (len 7): time order wins.
        let tl = vec![mk(1, 1000, 5), mk(2, 0, 7)];
        assert_eq!(layer_len(&snapshot_at(&tl, 0), "a", "l"), Some(7));
        assert_eq!(layer_len(&snapshot_at(&tl, 999), "a", "l"), Some(7));
        assert_eq!(layer_len(&snapshot_at(&tl, 1000), "a", "l"), Some(5));
        assert_eq!(layer_len(&head(&tl), "a", "l"), Some(5));

        let mut c = Cursor::new(&tl);
        assert!(c.advance_to(0));
        assert_eq!(layer_len(&c.snapshot, "a", "l"), Some(7));
        assert!(!c.advance_to(500));
        assert!(c.advance_to(1000));
        assert_eq!(layer_len(&c.snapshot, "a", "l"), Some(5));
    }

    #[test]
    fn preset_null_clears() {
        let p: TrackPatch = serde_json::from_str(r#"{"trackId":"a","presetId":null}"#).unwrap();
        assert_eq!(p.preset_id, Some(None));
        let p: TrackPatch = serde_json::from_str(r#"{"trackId":"a"}"#).unwrap();
        assert_eq!(p.preset_id, None);
    }
}
