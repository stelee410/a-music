//! Preset catalog (SPEC §9.7). Factory presets are JSON files under
//! `factory/presets/`, embedded at compile time by `build.rs`.

use agb_score::ParamValue;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MacroTarget {
    pub param: String,
    pub min: f64,
    pub max: f64,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PitchRange {
    /// Lowest recommended MIDI note.
    pub low: u8,
    /// Highest recommended MIDI note.
    pub high: u8,
}

/// Classification label, **not** a measured CPU figure.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CpuClass {
    Low,
    Med,
    High,
}

/// Preset metadata (§9.7; all listed fields required, `seed` optional).
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preset {
    /// `preset:{engine}:{role}:{name}@v{major}.{minor}`
    pub id: String,
    pub engine: String,
    pub engine_version: String,
    pub parameters: BTreeMap<String, ParamValue>,
    /// ≤ 8 macros: name → targets.
    pub macros: BTreeMap<String, Vec<MacroTarget>>,
    pub tags: Vec<String>,
    pub role: String,
    pub timbre: Vec<String>,
    pub articulation: String,
    pub pitch_range: PitchRange,
    pub velocity_curve: String,
    /// Output trim in dB applied on top of engine `gainDb`.
    pub gain: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    pub assets: Vec<String>,
    pub license: String,
    pub provenance: String,
    /// `sha256:<hex>` of the canonical JSON of this preset without `contentHash`.
    pub content_hash: String,
    pub cpu_class: CpuClass,
}

/// Parsed preset id.
#[derive(Debug, PartialEq, Eq)]
pub struct PresetIdParts<'a> {
    pub engine: &'a str,
    pub role: &'a str,
    pub name: &'a str,
    pub major: u32,
    pub minor: u32,
}

pub fn parse_preset_id(id: &str) -> Option<PresetIdParts<'_>> {
    let rest = id.strip_prefix("preset:")?;
    let (body, ver) = rest.rsplit_once("@v")?;
    let (maj, min) = ver.split_once('.')?;
    let mut it = body.split(':');
    let engine = it.next()?;
    let role = it.next()?;
    let name = it.next()?;
    if it.next().is_some() || engine.is_empty() || role.is_empty() || name.is_empty() {
        return None;
    }
    Some(PresetIdParts { engine, role, name, major: maj.parse().ok()?, minor: min.parse().ok()? })
}

/// Compute the content hash of a preset (canonical = serde_json with sorted keys).
pub fn content_hash(p: &Preset) -> String {
    let mut v = serde_json::to_value(p).expect("preset serialises");
    v.as_object_mut().unwrap().remove("contentHash");
    let bytes = serde_json::to_vec(&v).unwrap();
    format!("sha256:{}", hex::encode(Sha256::digest(&bytes)))
}

include!(concat!(env!("OUT_DIR"), "/factory.rs"));

/// The built-in catalog (parsed once).
pub fn catalog() -> &'static [Preset] {
    static CAT: OnceLock<Vec<Preset>> = OnceLock::new();
    CAT.get_or_init(|| {
        FACTORY
            .iter()
            .map(|(name, src)| {
                serde_json::from_str(src).unwrap_or_else(|e| panic!("factory preset {name}: {e}"))
            })
            .collect()
    })
}

pub fn get(id: &str) -> Option<&'static Preset> {
    catalog().iter().find(|p| p.id == id)
}

/// Search: all `tags` must match; `engine` / `role` exact if given.
pub fn search(tags: &[String], engine: Option<&str>, role: Option<&str>) -> Vec<&'static Preset> {
    catalog()
        .iter()
        .filter(|p| engine.map_or(true, |e| p.engine == e))
        .filter(|p| role.map_or(true, |r| p.role == r))
        .filter(|p| tags.iter().all(|t| p.tags.iter().any(|pt| pt == t)))
        .collect()
}

pub fn preset_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Preset)).expect("schema serialises")
}

/// Problems with a preset (empty = valid).
pub fn validate(p: &Preset) -> Vec<String> {
    let mut errs = vec![];
    match parse_preset_id(&p.id) {
        None => errs.push(format!("{}: malformed preset id", p.id)),
        Some(parts) => {
            if parts.engine != p.engine {
                errs.push(format!("{}: id engine != engine field {}", p.id, p.engine));
            }
            if parts.role != p.role {
                errs.push(format!("{}: id role != role field {}", p.id, p.role));
            }
        }
    }
    if p.macros.len() > 8 {
        errs.push(format!("{}: more than 8 macros", p.id));
    }
    match agb_dsp::EngineParams::resolve(&p.engine, &[&p.parameters]) {
        Err(e) => errs.push(format!("{}: {e}", p.id)),
        Ok(_) => {
            for (m, targets) in &p.macros {
                for t in targets {
                    if !agb_dsp::is_numeric_param(&p.engine, &t.param) {
                        errs.push(format!("{}: macro {m} targets unknown param {}", p.id, t.param));
                    }
                }
            }
        }
    }
    let h = content_hash(p);
    if h != p.content_hash {
        errs.push(format!("{}: contentHash mismatch, computed {h}", p.id));
    }
    errs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_catalog_valid() {
        let cat = catalog();
        assert!(cat.len() >= 8 && cat.len() <= 120, "P1 design target is ~80-120 presets");
        let mut ids = std::collections::BTreeSet::new();
        for p in cat {
            let errs = validate(p);
            assert!(errs.is_empty(), "{errs:#?}");
            assert!(ids.insert(&p.id), "duplicate id {}", p.id);
        }
        assert!(get("preset:synth.waveform:bass:simple_saw@v1.0").is_some());
    }

    #[test]
    fn id_parsing() {
        let p = parse_preset_id("preset:synth.waveform:bass:simple_saw@v1.0").unwrap();
        assert_eq!(p, PresetIdParts { engine: "synth.waveform", role: "bass", name: "simple_saw", major: 1, minor: 0 });
        assert!(parse_preset_id("preset:a:b@v1.0").is_none());
        assert!(parse_preset_id("synth.waveform:bass:x@v1.0").is_none());
    }

    #[test]
    fn search_works() {
        assert!(!search(&["bass".into()], Some("synth.waveform"), None).is_empty());
        assert!(!search(&["mathrock".into(), "bass".into()], Some("synth.moogish"), None).is_empty());
    }
}

#[cfg(test)]
mod maintenance {
    /// `cargo test -p agb-presets print_content_hashes -- --ignored --nocapture`
    /// prints the hash each factory preset should carry after editing it.
    #[test]
    #[ignore]
    fn print_content_hashes() {
        for p in super::catalog() {
            println!("HASH {} {}", p.id, super::content_hash(p));
        }
    }
}
