//! On-disk project store (SPEC §6): `project.json`, `.agb-idempotency/`, lock file.
//!
//! Writes are atomic (`write tmp` + `rename`). A lock file serialises writers
//! so the engine stays the single authority for `revision`.

use agb_protocol::{AgbError, ErrorCode, TxnResponse};
use agb_score::Project;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

pub const PROJECT_FILE: &str = "project.json";
pub const IDEMPOTENCY_DIR: &str = ".agb-idempotency";
pub const LOCK_FILE: &str = ".agb.lock";

#[derive(Clone, Debug)]
pub struct ProjectStore {
    pub root: PathBuf,
}

/// Cached first response for a `commandId`.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct IdempotencyRecord {
    pub command_id: String,
    pub payload_sha256: String,
    pub response: TxnResponse,
}

/// Held while writing; removed on drop.
pub struct LockGuard {
    path: PathBuf,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

impl ProjectStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        ProjectStore { root: root.into() }
    }

    pub fn project_path(&self) -> PathBuf {
        self.root.join(PROJECT_FILE)
    }

    pub fn exists(&self) -> bool {
        self.project_path().is_file()
    }

    /// Create the project directory layout with a default `project.json`.
    pub fn init(&self, name: Option<&str>) -> Result<Project, AgbError> {
        if self.exists() {
            return self.load();
        }
        for d in ["assets", "renders", IDEMPOTENCY_DIR] {
            fs::create_dir_all(self.root.join(d)).map_err(AgbError::io)?;
        }
        let mut p = Project::default();
        if let Some(n) = name {
            p.name = n.to_string();
        }
        self.save(&p)?;
        Ok(p)
    }

    pub fn load(&self) -> Result<Project, AgbError> {
        let path = self.project_path();
        let raw = fs::read_to_string(&path).map_err(|e| {
            AgbError::new(ErrorCode::ProjectNotFound, format!("{}: {e}", path.display()))
        })?;
        let p: Project = serde_json::from_str(&raw).map_err(|e| {
            AgbError::new(ErrorCode::SchemaViolation, format!("{}: {e}", path.display()))
        })?;
        check_spec_version(&p.spec_version)?;
        Ok(crate::state::normalize(p))
    }

    pub fn save(&self, p: &Project) -> Result<(), AgbError> {
        let mut bytes = serde_json::to_vec_pretty(p).map_err(AgbError::io)?;
        bytes.push(b'\n');
        atomic_write(&self.project_path(), &bytes)
    }

    fn idem_path(&self, command_id: &str) -> PathBuf {
        self.root.join(IDEMPOTENCY_DIR).join(format!("{}.json", sha256_hex(command_id.as_bytes())))
    }

    pub fn idempotency_get(&self, command_id: &str) -> Result<Option<IdempotencyRecord>, AgbError> {
        let p = self.idem_path(command_id);
        match fs::read_to_string(&p) {
            Ok(s) => serde_json::from_str(&s).map(Some).map_err(AgbError::io),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(AgbError::io(e)),
        }
    }

    pub fn idempotency_put(&self, rec: &IdempotencyRecord) -> Result<(), AgbError> {
        fs::create_dir_all(self.root.join(IDEMPOTENCY_DIR)).map_err(AgbError::io)?;
        let bytes = serde_json::to_vec_pretty(rec).map_err(AgbError::io)?;
        atomic_write(&self.idem_path(&rec.command_id), &bytes)
    }

    /// Acquire the writer lock (waits up to `timeout`; locks older than 60 s are stale).
    pub fn lock(&self, timeout: Duration) -> Result<LockGuard, AgbError> {
        fs::create_dir_all(&self.root).map_err(AgbError::io)?;
        let path = self.root.join(LOCK_FILE);
        let start = Instant::now();
        loop {
            match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Ok(LockGuard { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let stale = fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| SystemTime::now().duration_since(t).ok())
                        .is_some_and(|age| age > Duration::from_secs(60));
                    if stale {
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    if start.elapsed() > timeout {
                        return Err(AgbError::new(
                            ErrorCode::ProjectLocked,
                            format!("{} is held by another writer", path.display()),
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(e) => return Err(AgbError::io(e)),
            }
        }
    }
}

pub fn check_spec_version(v: &str) -> Result<(), AgbError> {
    let mut it = v.split('.');
    let major = it.next().and_then(|s| s.parse::<u64>().ok());
    let minor = it.next().and_then(|s| s.parse::<u64>().ok());
    // 0.x semver: the minor version is the compatibility line.
    if major == Some(0) && minor == Some(1) {
        Ok(())
    } else {
        Err(AgbError::new(
            ErrorCode::SpecVersionUnsupported,
            format!("project specVersion {v:?} is not supported (this build reads 0.1.x); no migration available"),
        ))
    }
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), AgbError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(AgbError::io)?;
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&tmp, bytes).map_err(AgbError::io)?;
    fs::rename(&tmp, path).map_err(AgbError::io)
}
