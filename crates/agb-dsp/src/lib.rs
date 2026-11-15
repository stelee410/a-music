//! Voice interface (SPEC §9.3) and P0 synthesis (`synth.waveform`, §9.4).

pub mod common;
pub mod detmath;
pub mod drum808;
pub mod fx;
pub mod modal;
pub mod sampler;
pub mod subtractive;
pub mod waveform;

use schemars::JsonSchema;
use serde::Serialize;

pub use agb_score::ParamValue;
pub use drum808::{DrumParams, DrumVoice};
pub use modal::{ModalParams, ModalVoice};
pub use sampler::{SamplerParams, SamplerVoice};
pub use subtractive::{SubParams, SubVoice};
pub use waveform::{Wave, WaveformParams, WaveformVoice};
use std::collections::BTreeMap;

/// Version tag of the DSP code; part of the render determinism key (§10.2 item 4).
pub const ENGINE_VERSION: &str = "agb-dsp/0.4.0";

/// Parameter metadata (§9.3 ParamMeta).
#[derive(Serialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ParamMeta {
    pub path: &'static str,
    pub unit: &'static str,
    pub kind: ParamKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    pub default: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safe_max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<&'static str>>,
}

#[derive(Serialize, JsonSchema, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParamKind {
    Continuous,
    Discrete,
}

/// Logical voice interface (§9.3). Rendering *adds* into a mono buffer.
pub trait Voice {
    fn note_on(&mut self, pitch: u8, velocity: f32, tick: u64);
    fn note_off(&mut self, pitch: u8, tick: u64);
    fn set_param(&mut self, path: &str, value: &ParamValue) -> Result<(), String>;
    /// Add `out.len()` samples into `out` (mono interleaved = 1 channel).
    fn render(&mut self, out: &mut [f32]);
    fn reset(&mut self);
    fn is_active(&self) -> bool;
    /// Current envelope level × amplitude (used by `quietest` steal).
    fn level(&self) -> f32;
    fn describe_params(&self) -> Vec<ParamMeta>;
    /// Voices in the same choke group on one track can cut each other (hats).
    fn choke_group(&self) -> Option<u8> {
        None
    }
    /// Whether this voice, when started, silences others in its choke group.
    fn chokes_others(&self) -> bool {
        false
    }
    /// The note's own velocity (0…1), given before `note_on`. `note_on` receives the amplitude, which
    /// also carries the track's and the layer's gain; an engine that chooses between samples by how
    /// hard the note is played needs the velocity itself.
    fn set_velocity(&mut self, _velocity: f32) {}
}

/// Resolved parameters of any available engine.
#[derive(Clone, Debug, PartialEq)]
pub enum EngineParams {
    Waveform(WaveformParams),
    Subtractive(SubParams),
    Drum808(DrumParams),
    Modal(ModalParams),
    Sampler(SamplerParams),
}

impl EngineParams {
    pub fn defaults(engine_id: &str) -> Option<Self> {
        Some(match engine_id {
            "synth.waveform" => EngineParams::Waveform(WaveformParams::default()),
            "synth.subtractive" => EngineParams::Subtractive(SubParams::new(false)),
            "synth.moogish" => EngineParams::Subtractive(SubParams::new(true)),
            "drum.analog808style" => EngineParams::Drum808(DrumParams::default()),
            "synth.modal" => EngineParams::Modal(ModalParams::default()),
            "sampler.multi" => EngineParams::Sampler(SamplerParams::default()),
            _ => return None,
        })
    }

    pub fn describe(engine_id: &str) -> Option<Vec<ParamMeta>> {
        Some(match engine_id {
            "synth.waveform" => waveform::describe_params(),
            "synth.subtractive" => SubParams::describe(false),
            "synth.moogish" => SubParams::describe(true),
            "drum.analog808style" => DrumParams::describe(),
            "synth.modal" => ModalParams::describe(),
            "sampler.multi" => SamplerParams::describe(),
            _ => return None,
        })
    }

    pub fn set(&mut self, path: &str, v: &ParamValue) -> Result<(), String> {
        match self {
            EngineParams::Waveform(p) => p.set(path, v),
            EngineParams::Subtractive(p) => p.set(path, v),
            EngineParams::Drum808(p) => p.set(path, v),
            EngineParams::Modal(p) => p.set(path, v),
            EngineParams::Sampler(p) => p.set(path, v),
        }
    }

    /// Engine defaults overlaid by each map in order.
    pub fn resolve(engine_id: &str, layers: &[&BTreeMap<String, ParamValue>]) -> Result<Self, String> {
        let mut p = Self::defaults(engine_id).ok_or_else(|| format!("engine {engine_id} not available"))?;
        for m in layers {
            for (k, v) in *m {
                p.set(k, v)?;
            }
        }
        Ok(p)
    }

    pub fn make_voice(&self, sample_rate: u32, seed: u64) -> Box<dyn Voice> {
        match self {
            EngineParams::Waveform(p) => Box::new(WaveformVoice::new(p.clone(), sample_rate, seed)),
            EngineParams::Subtractive(p) => Box::new(SubVoice::new(p.clone(), sample_rate, seed)),
            EngineParams::Drum808(p) => Box::new(DrumVoice::new(p.clone(), sample_rate, seed)),
            EngineParams::Modal(p) => Box::new(ModalVoice::new(p.clone(), sample_rate, seed)),
            EngineParams::Sampler(p) => Box::new(SamplerVoice::new(p.clone(), sample_rate, seed)),
        }
    }
}

/// Whether `path` is a numeric (automatable) parameter of `engine_id`.
pub fn is_numeric_param(engine_id: &str, path: &str) -> bool {
    EngineParams::describe(engine_id)
        .is_some_and(|m| m.iter().any(|p| p.path == path && p.kind == ParamKind::Continuous))
}

/// Engine availability in this build (§9.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineStatus {
    Available,
    /// Known engine, planned for a later phase (e.g. "P1").
    Planned(&'static str),
    Unknown,
}

pub fn engine_status(engine_id: &str) -> EngineStatus {
    match engine_id {
        "synth.waveform" | "drum.analog808style" | "synth.subtractive" | "synth.moogish" | "synth.modal" | "sampler.multi" => {
            EngineStatus::Available
        }
        "drum.pcm707style" => EngineStatus::Planned("P1 (needs a licensed PCM kit)"),
        "synth.wavetable" | "synth.fm" | "synth.pluck_mathrock" => EngineStatus::Planned("P2"),
        "synth.granular" => EngineStatus::Planned("P2-P3"),
        _ => EngineStatus::Unknown,
    }
}

/// All engine ids known to the spec, with status.
pub fn known_engines() -> Vec<(&'static str, EngineStatus)> {
    [
        "synth.waveform",
        "drum.analog808style",
        "drum.pcm707style",
        "synth.subtractive",
        "synth.moogish",
        "synth.wavetable",
        "synth.fm",
        "synth.granular",
        "synth.pluck_mathrock",
        "synth.modal",
        "sampler.multi",
    ]
    .into_iter()
    .map(|e| (e, engine_status(e)))
    .collect()
}

#[cfg(test)]
mod engine_tests {
    use super::*;

    fn play(engine: &str, pitch: u8, on: usize, total: usize) -> Vec<f32> {
        let p = EngineParams::defaults(engine).unwrap();
        let mut v = p.make_voice(48000, 3);
        v.note_on(pitch, 0.9, 0);
        let mut b = vec![0.0f32; total];
        v.render(&mut b[..on]);
        v.note_off(pitch, 0);
        v.render(&mut b[on..]);
        assert!(!v.is_active(), "{engine}/{pitch} should finish");
        b
    }

    #[test]
    fn engines_sound_bounded_and_finish() {
        for (engine, pitches) in [
            ("synth.subtractive", vec![36u8, 60, 84]),
            ("synth.moogish", vec![28, 48, 72]),
            ("drum.analog808style", vec![35, 36, 37, 38, 39, 42, 46, 41, 50, 49, 51, 53, 54, 56, 60, 61, 62, 63, 64, 70, 75, 99]),
        ] {
            for p in pitches {
                let b = play(engine, p, 9600, 9600 + 48000 * 6);
                let peak = b.iter().fold(0f32, |a, x| a.max(x.abs()));
                assert!(peak > 0.01 && peak < 1.5, "{engine}/{p} peak {peak}");
                assert!(b.iter().all(|x| x.is_finite()));
            }
        }
    }

    #[test]
    fn resolve_and_validate() {
        let mut m = BTreeMap::new();
        m.insert("filter.cutoffHz".to_string(), ParamValue::Num(400.0));
        assert!(EngineParams::resolve("synth.moogish", &[&m]).is_ok());
        m.insert("filter.type".to_string(), ParamValue::Str("hp".into()));
        assert!(EngineParams::resolve("synth.moogish", &[&m]).is_err(), "moogish has no filter.type");
        assert!(EngineParams::resolve("synth.subtractive", &[&m]).is_ok());
        assert!(is_numeric_param("synth.subtractive", "filter.cutoffHz"));
        assert!(!is_numeric_param("synth.subtractive", "osc.wave"));
        assert!(is_numeric_param("drum.analog808style", "bd.decayMs"));
    }

    #[test]
    fn hats_choke() {
        let p = EngineParams::defaults("drum.analog808style").unwrap();
        let mut ch = p.make_voice(48000, 1);
        ch.note_on(42, 1.0, 0);
        assert!(ch.chokes_others() && ch.choke_group() == Some(1));
        let mut oh = p.make_voice(48000, 1);
        oh.note_on(46, 1.0, 0);
        assert!(!oh.chokes_others() && oh.choke_group() == Some(1));
    }
}
