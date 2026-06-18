//! APK / XAPK / APKM analysis.
//!
//! Mirrors dab's approach: prefer `aapt`/`aapt2 dump badging` for full metadata,
//! fall back to a ZIP-structure summary when neither is installed. XAPK/APKM
//! bundles are extracted to a temp dir and the base APK is analyzed.

use crate::error::{anyhow, Result};
use crate::model::ApkInfo;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use zip::ZipArchive;

/// Analyze any supported package file (`.apk`, `.xapk`, `.apkm`).
pub fn analyze(path: &Path) -> Result<ApkInfo> {
    match extension(path).as_deref() {
        Some("apk") => analyze_apk(path),
        Some("xapk") | Some("apkm") => analyze_bundle(path),
        _ => Err(anyhow!(
            "Unsupported file type. Only APK, XAPK, and APKM files are supported."
        )),
    }
}

/// Analyze a single APK: aapt first, ZIP fallback.
pub fn analyze_apk(apk: &Path) -> Result<ApkInfo> {
    if let Some(info) = analyze_with_aapt(apk) {
        return Ok(info);
    }
    analyze_apk_basic(apk)
}

/// Run `aapt`/`aapt2 dump badging` and parse it. `None` if no aapt is available
/// or the command fails.
fn analyze_with_aapt(apk: &Path) -> Option<ApkInfo> {
    for cmd in ["aapt", "aapt2"] {
        if let Ok(aapt) = which::which(cmd) {
            if let Ok(output) = Command::new(&aapt)
                .args(["dump", "badging", &apk.to_string_lossy()])
                .output()
            {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    return Some(parse_aapt_badging(&stdout, apk));
                }
            }
        }
    }
    None
}

/// Parse `aapt dump badging` output into [`ApkInfo`]. Public for unit testing.
pub fn parse_aapt_badging(aapt_output: &str, apk: &Path) -> ApkInfo {
    let mut info = ApkInfo {
        file: apk.to_string_lossy().to_string(),
        package_name: "N/A".to_string(),
        app_name: "N/A".to_string(),
        version_code: "N/A".to_string(),
        version_name: "N/A".to_string(),
        ..Default::default()
    };

    for line in aapt_output.lines() {
        let line = line.trim();
        if line.starts_with("package:") {
            if let Some(v) = attr(line, "name") {
                info.package_name = v;
            }
            if let Some(v) = attr(line, "versionCode") {
                info.version_code = v;
            }
            if let Some(v) = attr(line, "versionName") {
                info.version_name = if v.is_empty() {
                    "Not set".to_string()
                } else {
                    v
                };
            }
        } else if line.starts_with("application-label:") {
            let label = unquote(line.trim_start_matches("application-label:").trim());
            if !label.is_empty() {
                info.app_name = label;
            }
        } else if line.starts_with("application-label-") && info.app_name == "N/A" {
            if let Some(rest) = line.split_once(':') {
                let label = unquote(rest.1.trim());
                if !label.is_empty() {
                    info.app_name = label;
                }
            }
        } else if line.starts_with("uses-permission:") {
            if let Some(p) = attr(line, "name") {
                if !info.permissions.contains(&p) {
                    info.permissions.push(p);
                }
            }
        }
    }
    info
}

/// ZIP-structure fallback when aapt is unavailable. Returns minimal info with a
/// note; package metadata is unavailable without aapt.
fn analyze_apk_basic(apk: &Path) -> Result<ApkInfo> {
    let file = fs::File::open(apk)?;
    let mut archive = ZipArchive::new(file)?;
    let mut has_manifest = false;
    for i in 0..archive.len() {
        if let Ok(f) = archive.by_index(i) {
            if f.name() == "AndroidManifest.xml" {
                has_manifest = true;
                break;
            }
        }
    }
    if !has_manifest {
        return Err(anyhow!(
            "{} does not look like an APK (no AndroidManifest.xml)",
            apk.display()
        ));
    }
    Ok(ApkInfo {
        file: apk.to_string_lossy().to_string(),
        package_name: "N/A (install aapt/aapt2 for full info)".to_string(),
        app_name: "N/A".to_string(),
        version_code: "N/A".to_string(),
        version_name: "N/A".to_string(),
        ..Default::default()
    })
}

/// Extract an XAPK/APKM, find the base APK, analyze it, and tag the result with
/// the bundle source + APK count.
fn analyze_bundle(bundle: &Path) -> Result<ApkInfo> {
    let temp = std::env::temp_dir().join(format!("androkit_bundle_{}", std::process::id()));
    fs::create_dir_all(&temp)?;
    let result = (|| {
        extract_zip(bundle, &temp)?;
        let mut apks = Vec::new();
        find_apks(&temp, &mut apks)?;
        if apks.is_empty() {
            return Err(anyhow!("No APK files found in bundle"));
        }
        let count = apks.len();
        let base = find_base_apk(&apks);
        let mut info = analyze_apk(&base)?;
        info.source_file = Some(bundle.to_string_lossy().to_string());
        info.apk_count = Some(count);
        Ok(info)
    })();
    let _ = fs::remove_dir_all(&temp);
    result
}

/// Extract every supported APK out of an XAPK/APKM into `dest`, returning their paths.
pub fn extract_apks(bundle: &Path, dest: &Path) -> Result<Vec<PathBuf>> {
    extract_zip(bundle, dest)?;
    let mut apks = Vec::new();
    find_apks(dest, &mut apks)?;
    Ok(apks)
}

fn extract_zip(archive_path: &Path, dest: &Path) -> Result<()> {
    let file = fs::File::open(archive_path)?;
    let mut archive = ZipArchive::new(file)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let outpath = match entry.enclosed_name() {
            Some(p) => dest.join(p),
            None => continue,
        };
        if entry.name().ends_with('/') {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut out = fs::File::create(&outpath)?;
            std::io::copy(&mut entry, &mut out)?;
        }
    }
    Ok(())
}

fn find_apks(dir: &Path, apks: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            find_apks(&path, apks)?;
        } else if extension(&path).as_deref() == Some("apk") {
            apks.push(path);
        }
    }
    Ok(())
}

/// Pick the base APK from a split set: `base.apk`, then any name containing
/// "base", then the largest file. Public for unit testing.
pub fn find_base_apk(apks: &[PathBuf]) -> PathBuf {
    for apk in apks {
        if apk.file_name().map(|n| n.to_string_lossy().to_lowercase()) == Some("base.apk".into()) {
            return apk.clone();
        }
    }
    for apk in apks {
        if apk
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase().contains("base"))
            .unwrap_or(false)
        {
            return apk.clone();
        }
    }
    apks.iter()
        .max_by_key(|p| fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .cloned()
        .unwrap_or_else(|| apks[0].clone())
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
}

/// Extract `attr='value'` or `attr="value"` from an aapt badging line.
fn attr(line: &str, key: &str) -> Option<String> {
    for q in ['\'', '"'] {
        let needle = format!("{key}={q}");
        if let Some(start) = line.find(&needle) {
            let rest = &line[start + needle.len()..];
            if let Some(end) = rest.find(q) {
                return Some(rest[..end].to_string());
            }
        }
    }
    None
}

fn unquote(s: &str) -> String {
    s.trim_matches('\'').trim_matches('"').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_aapt_badging() {
        let sample = r#"package: name='com.example.app' versionCode='42' versionName='1.2.3' platformBuildVersionName=''
application-label:'Example App'
application-label-en:'Example App'
uses-permission: name='android.permission.CAMERA'
uses-permission: name='android.permission.INTERNET'"#;
        let info = parse_aapt_badging(sample, Path::new("/tmp/x.apk"));
        assert_eq!(info.package_name, "com.example.app");
        assert_eq!(info.version_code, "42");
        assert_eq!(info.version_name, "1.2.3");
        assert_eq!(info.app_name, "Example App");
        assert_eq!(
            info.permissions,
            vec![
                "android.permission.CAMERA".to_string(),
                "android.permission.INTERNET".to_string()
            ]
        );
    }

    #[test]
    fn empty_version_name_becomes_not_set() {
        let sample = "package: name='c.x' versionCode='1' versionName=''";
        let info = parse_aapt_badging(sample, Path::new("/tmp/x.apk"));
        assert_eq!(info.version_name, "Not set");
    }

    #[test]
    fn base_apk_prefers_exact_name() {
        let apks = vec![
            PathBuf::from("/t/split_config.arm64.apk"),
            PathBuf::from("/t/base.apk"),
        ];
        assert_eq!(find_base_apk(&apks), PathBuf::from("/t/base.apk"));
    }
}
