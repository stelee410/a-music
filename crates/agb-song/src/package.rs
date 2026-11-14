//! Package container (FORMAT.md §2): a directory or a zip with the same layout.

use crate::model::Song;
use agb_protocol::{AgbError, ErrorCode};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const MIMETYPE: &str = "application/vnd.agb.song+zip";
pub const SONG_FILE: &str = "song.json";
pub const MAX_SONG_JSON: u64 = 16 * 1024 * 1024;
pub const MAX_PACKAGE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_REF_DEPTH: u32 = 8;

fn err(code: ErrorCode, msg: impl Into<String>) -> AgbError {
    AgbError::new(code, msg)
}

/// A relative, `/`-separated path that cannot leave the package root.
pub fn safe_rel_path(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && !p.contains('\\')
        && !p.contains(':')
        && !p.contains('\0')
        && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
}

fn check_path(p: &str) -> Result<(), AgbError> {
    if safe_rel_path(p) {
        Ok(())
    } else {
        Err(err(ErrorCode::SchemaViolation, format!("unsafe package path {p:?}")))
    }
}

type Zip = zip::ZipArchive<fs::File>;

/// An opened `.agbsong` (read-only).
pub enum Package {
    Dir(PathBuf),
    Zip { path: PathBuf, archive: RefCell<Zip> },
}

impl Package {
    pub fn open(path: &Path) -> Result<Package, AgbError> {
        if path.is_dir() {
            return Ok(Package::Dir(path.to_path_buf()));
        }
        let f = fs::File::open(path)
            .map_err(|e| err(ErrorCode::ProjectNotFound, format!("{}: {e}", path.display())))?;
        let archive = zip::ZipArchive::new(f)
            .map_err(|e| err(ErrorCode::SchemaViolation, format!("{}: not a zip package: {e}", path.display())))?;
        let pkg = Package::Zip { path: path.to_path_buf(), archive: RefCell::new(archive) };
        let mut total = 0u64;
        for (name, size) in pkg.list()? {
            check_path(&name)?;
            total += size;
        }
        if total > MAX_PACKAGE_BYTES {
            return Err(err(ErrorCode::BudgetExceeded, "package exceeds 2 GiB uncompressed"));
        }
        Ok(pkg)
    }

    pub fn is_zip(&self) -> bool {
        matches!(self, Package::Zip { .. })
    }

    pub fn location(&self) -> &Path {
        match self {
            Package::Dir(p) => p,
            Package::Zip { path, .. } => path,
        }
    }

    /// All files as `(relative path, size)`, sorted. Dot files are not part of a package.
    pub fn list(&self) -> Result<Vec<(String, u64)>, AgbError> {
        let mut out = vec![];
        match self {
            Package::Dir(root) => walk(root, "", &mut out)?,
            Package::Zip { archive, .. } => {
                let mut a = archive.borrow_mut();
                for i in 0..a.len() {
                    let f = a.by_index(i).map_err(AgbError::io)?;
                    if !f.is_dir() {
                        out.push((f.name().to_string(), f.size()));
                    }
                }
            }
        }
        out.sort();
        Ok(out)
    }

    pub fn exists(&self, rel: &str) -> bool {
        if !safe_rel_path(rel) {
            return false;
        }
        match self {
            Package::Dir(root) => root.join(rel).is_file(),
            Package::Zip { archive, .. } => archive.borrow_mut().by_name(rel).is_ok(),
        }
    }

    pub fn read(&self, rel: &str) -> Result<Vec<u8>, AgbError> {
        check_path(rel)?;
        let missing = |e: &dyn std::fmt::Display| err(ErrorCode::AssetMissing, format!("{rel}: {e}"));
        match self {
            Package::Dir(root) => fs::read(root.join(rel)).map_err(|e| missing(&e)),
            Package::Zip { archive, .. } => {
                let mut a = archive.borrow_mut();
                let mut f = a.by_name(rel).map_err(|e| missing(&e))?;
                let mut buf = Vec::with_capacity(f.size() as usize);
                f.read_to_end(&mut buf).map_err(AgbError::io)?;
                Ok(buf)
            }
        }
    }

    /// `sha256:<hex>` of a packaged file.
    pub fn sha256(&self, rel: &str) -> Result<String, AgbError> {
        Ok(sha256_tag(&self.read(rel)?))
    }

    /// Read `song.json`, resolve `$ref`s and parse it.
    pub fn load_song(&self) -> Result<Song, AgbError> {
        let v = self.load_song_value()?;
        serde_json::from_value(v).map_err(|e| err(ErrorCode::SchemaViolation, format!("{SONG_FILE}: {e}")))
    }

    /// `song.json` as a JSON value with every `$ref` resolved.
    pub fn load_song_value(&self) -> Result<Value, AgbError> {
        if !self.exists(SONG_FILE) {
            return Err(err(
                ErrorCode::ProjectNotFound,
                format!("{}: no {SONG_FILE} in package", self.location().display()),
            ));
        }
        let mut v = self.read_json(SONG_FILE)?;
        self.resolve_refs(&mut v, 0)?;
        Ok(v)
    }

    fn read_json(&self, rel: &str) -> Result<Value, AgbError> {
        let bytes = self.read(rel)?;
        if bytes.len() as u64 > MAX_SONG_JSON {
            return Err(err(ErrorCode::BudgetExceeded, format!("{rel} exceeds 16 MiB")));
        }
        serde_json::from_slice(&bytes).map_err(|e| err(ErrorCode::InvalidJson, format!("{rel}: {e}")))
    }

    fn resolve_refs(&self, v: &mut Value, depth: u32) -> Result<(), AgbError> {
        match v {
            Value::Object(m) => {
                if m.len() == 1 {
                    if let Some(Value::String(target)) = m.get("$ref") {
                        let target = target.clone();
                        if depth >= MAX_REF_DEPTH {
                            return Err(err(
                                ErrorCode::SchemaViolation,
                                format!("$ref {target:?}: nesting deeper than {MAX_REF_DEPTH}"),
                            ));
                        }
                        if !safe_rel_path(&target) || !target.ends_with(".json") {
                            return Err(err(
                                ErrorCode::SchemaViolation,
                                format!("$ref {target:?} must be a .json file inside the package"),
                            ));
                        }
                        let mut inner = self.read_json(&target)?;
                        self.resolve_refs(&mut inner, depth + 1)?;
                        *v = inner;
                        return Ok(());
                    }
                }
                for child in m.values_mut() {
                    self.resolve_refs(child, depth)?;
                }
            }
            Value::Array(a) => {
                for child in a {
                    self.resolve_refs(child, depth)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

/// Display size of the first video track of an MP4 / MOV file, read from its
/// `tkhd` box (width and height are the last two 16.16 fixed-point fields).
pub fn video_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut from = 4;
    while let Some(off) = bytes.get(from..)?.windows(4).position(|w| w == b"tkhd") {
        let at = from + off;
        let size = u32::from_be_bytes(bytes.get(at - 4..at)?.try_into().ok()?) as usize;
        let end = at - 4 + size;
        if size >= 92 && end <= bytes.len() {
            let be = |i: usize| u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap()) >> 16;
            let (w, h) = (be(end - 8), be(end - 4));
            if w > 0 && h > 0 {
                return Some((w, h));
            }
        }
        from = at + 4;
    }
    None
}

/// The aspect ratios a song video may have (FORMAT.md §12.1).
pub fn video_aspect_ok(w: u32, h: u32) -> bool {
    let r = w as f64 / h as f64;
    [16.0 / 9.0, 4.0 / 3.0].iter().any(|a| (r - a).abs() / a < 0.02)
}

pub fn sha256_tag(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

fn walk(root: &Path, prefix: &str, out: &mut Vec<(String, u64)>) -> Result<(), AgbError> {
    let dir = if prefix.is_empty() { root.to_path_buf() } else { root.join(prefix) };
    for entry in fs::read_dir(&dir).map_err(AgbError::io)? {
        let entry = entry.map_err(AgbError::io)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        let ty = entry.file_type().map_err(AgbError::io)?;
        if ty.is_dir() {
            walk(root, &rel, out)?;
        } else if ty.is_file() {
            out.push((rel, entry.metadata().map_err(AgbError::io)?.len()));
        }
        // Symlinks are not followed: a package cannot point outside itself.
    }
    Ok(())
}

/// Pack a directory into a zip. Output bytes depend only on the file contents
/// (sorted entries, fixed timestamps), so packing twice gives the same file.
pub fn pack(dir: &Path, out: &Path) -> Result<Vec<String>, AgbError> {
    let pkg = Package::open(dir)?;
    if pkg.is_zip() {
        return Err(err(ErrorCode::SchemaViolation, format!("{} is not a directory", dir.display())));
    }
    if !pkg.exists(SONG_FILE) {
        return Err(err(ErrorCode::ProjectNotFound, format!("{}: no {SONG_FILE}", dir.display())));
    }
    let files = pkg.list()?;
    if files.iter().map(|f| f.1).sum::<u64>() > MAX_PACKAGE_BYTES {
        return Err(err(ErrorCode::BudgetExceeded, "package exceeds 2 GiB"));
    }
    let tmp = out.with_extension(format!("tmp.{}", std::process::id()));
    let f = fs::File::create(&tmp).map_err(AgbError::io)?;
    let mut zw = zip::ZipWriter::new(f);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut names = vec!["mimetype".to_string()];
    zw.start_file("mimetype", stored).map_err(AgbError::io)?;
    zw.write_all(MIMETYPE.as_bytes()).map_err(AgbError::io)?;
    for (rel, _) in &files {
        if rel == "mimetype" {
            continue;
        }
        check_path(rel)?;
        zw.start_file(rel.as_str(), deflated).map_err(AgbError::io)?;
        zw.write_all(&pkg.read(rel)?).map_err(AgbError::io)?;
        names.push(rel.clone());
    }
    zw.finish().map_err(AgbError::io)?;
    fs::rename(&tmp, out).map_err(AgbError::io)?;
    Ok(names)
}

/// Unpack a zip package into a new (or empty) directory.
pub fn unpack(zip_path: &Path, out_dir: &Path) -> Result<Vec<String>, AgbError> {
    let pkg = Package::open(zip_path)?;
    if !pkg.is_zip() {
        return Err(err(ErrorCode::SchemaViolation, format!("{} is not a zip package", zip_path.display())));
    }
    if out_dir.exists() && fs::read_dir(out_dir).map_err(AgbError::io)?.next().is_some() {
        return Err(err(ErrorCode::IoError, format!("{} exists and is not empty", out_dir.display())));
    }
    let mut names = vec![];
    for (rel, _) in pkg.list()? {
        // `Package::open` already rejected unsafe entry names.
        let dest = out_dir.join(&rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(AgbError::io)?;
        }
        fs::write(&dest, pkg.read(&rel)?).map_err(AgbError::io)?;
        names.push(rel);
    }
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_safety() {
        for ok in ["song.json", "samples/kick.wav", "a/b/c.json"] {
            assert!(safe_rel_path(ok), "{ok}");
        }
        for bad in ["", "/etc/passwd", "../x", "a/../b", "a//b", "./a", "a\\b", "C:/x", "a/"] {
            assert!(!safe_rel_path(bad), "{bad}");
        }
    }
}
