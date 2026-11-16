//! `voice.chimera`: a track sung by a simulated voice (FORMAT.md §8.7).
//!
//! This crate only carries the description. The renderer gives the track's notes and their lyrics
//! to the singer (crate `agb-voice`) instead of making voices.

use crate::{ParamKind, ParamMeta, ParamValue};

pub const ENGINE_ID: &str = "voice.chimera";
pub const BODIES: &[&str] = &["male", "female", "child"];
pub const LANGUAGES: &[&str] = &["en", "zh"];

/// Who sings and how the lyrics are read.
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceParams {
    /// `male`, `female` or `child`.
    pub body: String,
    /// `en`: English words (also Korean in Hangul and Russian in Cyrillic); `zh`: Mandarin in pinyin.
    pub language: String,
    /// Vibrato depth on held vowels, in cents.
    pub vibrato_cents: f64,
}

impl Default for VoiceParams {
    fn default() -> Self {
        VoiceParams { body: "female".into(), language: "en".into(), vibrato_cents: 35.0 }
    }
}

impl VoiceParams {
    pub fn set(&mut self, path: &str, v: &ParamValue) -> Result<(), String> {
        match (path, v) {
            ("body", ParamValue::Str(s)) if BODIES.contains(&s.as_str()) => self.body = s.clone(),
            ("body", _) => return Err(format!("body must be one of {BODIES:?}")),
            ("language", ParamValue::Str(s)) if LANGUAGES.contains(&s.as_str()) => self.language = s.clone(),
            ("language", _) => return Err(format!("language must be one of {LANGUAGES:?}")),
            ("vibratoCents", ParamValue::Num(x)) if (0.0..=100.0).contains(x) => self.vibrato_cents = *x,
            ("vibratoCents", _) => return Err("vibratoCents must be a number from 0 to 100".into()),
            _ => return Err(format!("{ENGINE_ID} has no parameter {path:?} (it has body, language and vibratoCents)")),
        }
        Ok(())
    }

    pub fn describe() -> Vec<ParamMeta> {
        vec![
            ParamMeta { path: "body", unit: "", kind: ParamKind::Discrete, min: None, max: None, default: "female".into(), safe_max: None, choices: Some(BODIES.to_vec()) },
            ParamMeta { path: "language", unit: "", kind: ParamKind::Discrete, min: None, max: None, default: "en".into(), safe_max: None, choices: Some(LANGUAGES.to_vec()) },
            ParamMeta { path: "vibratoCents", unit: "cents", kind: ParamKind::Continuous, min: Some(0.0), max: Some(100.0), default: 35.0.into(), safe_max: None, choices: None },
        ]
    }
}
