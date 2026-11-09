//! Asset index, content hashing and license registry (SPEC §6, §7.1, §9.1).
//!
//! P0 engines (`synth.waveform`) need no sample assets, but the project model
//! already carries `assets` + `licensesRegistry`, so this crate provides the
//! types, hashing and the license policy used by `agb asset verify`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

/// Kind of asset referenced by a project.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    #[default]
    Sample,
    Wavetable,
    Sfz,
    Other,
}

/// A file referenced by the project, relative to `projectRoot`.
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetRef {
    pub id: String,
    /// Path relative to the project root (e.g. `assets/kick.wav`).
    pub path: String,
    #[serde(default)]
    pub kind: AssetKind,
    /// `sha256:<hex>` of the file content.
    pub sha256: String,
}

/// License record for one asset id (`licensesRegistry[assetId]`).
#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LicenseRecord {
    /// SPDX identifier, or `LicenseRef-original` for self-made material,
    /// or `LicenseRef-private` for user-owned kits that must never ship.
    pub spdx: String,
    /// Whether the asset may be redistributed (e.g. in a factory kit).
    #[serde(default)]
    pub redistributable: bool,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

/// Licenses accepted for factory content (SPEC §1: original / CC0 / clearly
/// redistributable). GPL is deliberately not on this list.
pub const FACTORY_ALLOWED_LICENSES: &[&str] = &[
    "LicenseRef-original",
    "CC0-1.0",
    "CC-BY-4.0",
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "Unlicense",
];

/// Factory policy: may this record be shipped in `factory/`?
pub fn factory_allowed(rec: &LicenseRecord) -> bool {
    rec.redistributable && FACTORY_ALLOWED_LICENSES.contains(&rec.spdx.as_str())
}

/// `sha256:<hex>` of a byte slice.
pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("sha256:{}", hex::encode(h.finalize()))
}

/// `sha256:<hex>` of a file, streamed.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("sha256:{}", hex::encode(h.finalize())))
}

/// A problem found while verifying project assets.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AssetProblem {
    pub asset_id: String,
    /// `ASSET_MISSING` | `ASSET_HASH_MISMATCH` | `LICENSE_MISSING` | `LICENSE_FORBIDDEN`
    pub code: &'static str,
    pub message: String,
}

/// Verify that every asset exists, matches its hash and has a license record.
/// With `factory = true`, also enforce the factory license policy.
pub fn verify(
    project_root: &Path,
    assets: &[AssetRef],
    registry: &BTreeMap<String, LicenseRecord>,
    factory: bool,
) -> Vec<AssetProblem> {
    let mut out = Vec::new();
    for a in assets {
        let p = project_root.join(&a.path);
        match sha256_file(&p) {
            Err(_) => out.push(AssetProblem {
                asset_id: a.id.clone(),
                code: "ASSET_MISSING",
                message: format!("file not found: {}", a.path),
            }),
            Ok(h) if h != a.sha256 => out.push(AssetProblem {
                asset_id: a.id.clone(),
                code: "ASSET_HASH_MISMATCH",
                message: format!("expected {}, found {}", a.sha256, h),
            }),
            Ok(_) => {}
        }
        match registry.get(&a.id) {
            None => out.push(AssetProblem {
                asset_id: a.id.clone(),
                code: "LICENSE_MISSING",
                message: "no licensesRegistry entry".into(),
            }),
            Some(rec) if factory && !factory_allowed(rec) => out.push(AssetProblem {
                asset_id: a.id.clone(),
                code: "LICENSE_FORBIDDEN",
                message: format!(
                    "license {} (redistributable={}) not allowed in factory content",
                    rec.spdx, rec.redistributable
                ),
            }),
            Some(_) => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_policy() {
        let ok = LicenseRecord { spdx: "CC0-1.0".into(), redistributable: true, source: None, notes: None };
        let gpl = LicenseRecord { spdx: "GPL-3.0-only".into(), redistributable: true, source: None, notes: None };
        let private = LicenseRecord { spdx: "LicenseRef-private".into(), redistributable: false, source: None, notes: None };
        assert!(factory_allowed(&ok));
        assert!(!factory_allowed(&gpl));
        assert!(!factory_allowed(&private));
    }

    #[test]
    fn hash_known_vector() {
        assert_eq!(
            sha256_bytes(b"abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn missing_asset_reported() {
        let dir = std::env::temp_dir().join("agb-assets-test-nonexistent");
        let assets = vec![AssetRef { id: "k".into(), path: "nope.wav".into(), kind: AssetKind::Sample, sha256: "sha256:00".into() }];
        let probs = verify(&dir, &assets, &BTreeMap::new(), false);
        assert!(probs.iter().any(|p| p.code == "ASSET_MISSING"));
        assert!(probs.iter().any(|p| p.code == "LICENSE_MISSING"));
    }
}
