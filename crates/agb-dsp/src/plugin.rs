//! `plugin.vst3`: a track played by a VST 3 plug-in (FORMAT.md §8.6).
//!
//! This crate only carries the description. The renderer hands the track's notes to the plug-in
//! (crate `agb-plugin`) instead of making voices.

use crate::{ParamKind, ParamMeta, ParamValue, Voice};
use std::collections::BTreeMap;

pub const ENGINE_ID: &str = "plugin.vst3";

/// Which plug-in plays, and the sound it is set to.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct PluginParams {
    /// Bundle name without `.vst3`.
    pub name: String,
    /// Class id (32 hex digits), for bundles that hold several plug-ins.
    pub uid: Option<String>,
    /// Path of a `.vstpreset` file, loaded before `values` are applied.
    pub preset: Option<String>,
    /// Parameter title (or `#id`) → 0…1.
    pub values: BTreeMap<String, f64>,
}

impl PluginParams {
    /// Paths: `plugin`, `uid`, `preset`, and `param.<title>` for the plug-in's own parameters.
    pub fn set(&mut self, path: &str, v: &ParamValue) -> Result<(), String> {
        match (path, v) {
            ("plugin", ParamValue::Str(s)) => self.name = s.clone(),
            ("uid", ParamValue::Str(s)) => self.uid = Some(s.clone()),
            ("preset", ParamValue::Str(s)) => self.preset = Some(s.clone()),
            (p, ParamValue::Num(x)) if p.starts_with("param.") && p.len() > 6 => {
                if !(0.0..=1.0).contains(x) {
                    return Err(format!("{p}: plug-in parameters are 0…1, got {x}"));
                }
                self.values.insert(p[6..].to_string(), *x);
            }
            _ => return Err(format!("{ENGINE_ID} has no parameter {path:?} of that type (it has plugin, uid, preset and param.<title>)")),
        }
        Ok(())
    }

    pub fn describe() -> Vec<ParamMeta> {
        ["plugin", "uid", "preset"]
            .into_iter()
            .map(|path| ParamMeta { path, unit: "", kind: ParamKind::Discrete, min: None, max: None, default: "".into(), safe_max: None, choices: None })
            .collect()
    }

    /// Settings from a JSON object `{ "plugin", "uid", "preset", "values": { title: 0…1 } }` (track effects).
    pub fn from_json(v: &serde_json::Value) -> Option<PluginParams> {
        let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
        Some(PluginParams {
            name: s("plugin")?,
            uid: s("uid"),
            preset: s("preset"),
            values: v
                .get("values")
                .and_then(|x| x.as_object())
                .map(|m| m.iter().filter_map(|(k, x)| Some((k.clone(), x.as_f64()?.clamp(0.0, 1.0)))).collect())
                .unwrap_or_default(),
        })
    }
}

/// Stands in where a voice is asked for; the renderer never plays it.
pub struct SilentVoice;

impl Voice for SilentVoice {
    fn note_on(&mut self, _pitch: u8, _velocity: f32, _tick: u64) {}
    fn note_off(&mut self, _pitch: u8, _tick: u64) {}
    fn set_param(&mut self, _path: &str, _value: &ParamValue) -> Result<(), String> {
        Ok(())
    }
    fn render(&mut self, _out: &mut [f32]) {}
    fn reset(&mut self) {}
    fn is_active(&self) -> bool {
        false
    }
    fn level(&self) -> f32 {
        0.0
    }
    fn describe_params(&self) -> Vec<ParamMeta> {
        PluginParams::describe()
    }
}
