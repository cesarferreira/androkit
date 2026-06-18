//! On-disk discovery cache.
//!
//! Discovery is cheap but not free, and the inner loop hits it constantly. We
//! cache the [`AndroidProject`] keyed by the project root, and invalidate it
//! whenever any build file it depends on (settings script, each module's build
//! script, the app manifest) changes mtime. A new module shows up as a changed
//! `settings.gradle` mtime, so structural changes invalidate too.

use crate::model::AndroidProject;
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    /// (file path, mtime seconds) for every file the project depends on.
    signature: Vec<(String, u64)>,
    project: AndroidProject,
}

/// Load a cached project for `root` if present and still valid.
pub fn load(root: &Path) -> Option<AndroidProject> {
    let path = cache_file(root)?;
    let raw = std::fs::read_to_string(&path).ok()?;
    let entry: CacheEntry = serde_json::from_str(&raw).ok()?;
    if entry.signature == signature(root, &entry.project) {
        Some(entry.project)
    } else {
        None
    }
}

/// Persist `project` for `root`. Best-effort: cache failures are non-fatal.
pub fn store(root: &Path, project: &AndroidProject) {
    let Some(path) = cache_file(root) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let entry = CacheEntry {
        signature: signature(root, project),
        project: project.clone(),
    };
    if let Ok(json) = serde_json::to_string(&entry) {
        let _ = std::fs::write(&path, json);
    }
}

/// Remove all cached discovery data.
pub fn clear_cache() -> std::io::Result<()> {
    if let Some(dir) = cache_dir() {
        if dir.exists() {
            return std::fs::remove_dir_all(dir);
        }
    }
    Ok(())
}

/// The files whose mtimes determine whether the cached project is still valid.
fn signature(root: &Path, project: &AndroidProject) -> Vec<(String, u64)> {
    let mut files: Vec<PathBuf> = Vec::new();

    for settings in ["settings.gradle", "settings.gradle.kts"] {
        files.push(root.join(settings));
    }
    for module in &project.modules {
        for build in ["build.gradle", "build.gradle.kts"] {
            files.push(root.join(&module.dir).join(build));
        }
    }
    if let Some(app_path) = &project.app_module {
        if let Some(app) = project.modules.iter().find(|m| &m.path == app_path) {
            files.push(root.join(&app.dir).join("src/main/AndroidManifest.xml"));
        }
    }

    let mut sig: Vec<(String, u64)> = files
        .into_iter()
        .filter(|p| p.exists())
        .map(|p| {
            let mtime = std::fs::metadata(&p)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            (p.to_string_lossy().to_string(), mtime)
        })
        .collect();
    sig.sort();
    sig
}

fn cache_dir() -> Option<PathBuf> {
    BaseDirs::new().map(|d| d.cache_dir().join("androkit"))
}

fn cache_file(root: &Path) -> Option<PathBuf> {
    let mut hasher = DefaultHasher::new();
    root.to_string_lossy().hash(&mut hasher);
    let key = hasher.finish();
    cache_dir().map(|d| d.join(format!("{key:x}.json")))
}
