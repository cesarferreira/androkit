//! Lightweight `AndroidManifest.xml` parsing for **source** (text) manifests.
//!
//! This targets the human-readable manifests in a project's `src/`, not the
//! binary AXML inside a built APK (use [`crate::apk`] + aapt for those). Parsing
//! is intentionally dependency-free string scanning — robust enough to find the
//! launcher activity and declared package without pulling in an XML crate.

use crate::error::Result;
use std::path::Path;

/// Find the launcher activity in a manifest's XML text.
///
/// Returns the activity's `android:name`, resolved to a fully-qualified class
/// when it starts with `.` (relative to `package`, the runtime applicationId or
/// the manifest's own `package` attribute). Handles both `<activity>` and
/// `<activity-alias>` declarations.
pub fn launcher_activity(manifest_xml: &str, package: Option<&str>) -> Option<String> {
    let pkg = package
        .map(|s| s.to_string())
        .or_else(|| manifest_package(manifest_xml));

    for block in activity_blocks(manifest_xml) {
        if is_launcher(&block) {
            // activity-alias is launchable by its own name.
            let name = attr(&block, "android:name")?;
            return Some(resolve(&name, pkg.as_deref()));
        }
    }
    None
}

/// Read the legacy `package="..."` attribute from a manifest, if present.
pub fn manifest_package(manifest_xml: &str) -> Option<String> {
    // Only consider the opening <manifest ...> tag.
    let start = manifest_xml.find("<manifest")?;
    let end = manifest_xml[start..].find('>')? + start;
    attr(&manifest_xml[start..=end], "package")
}

/// Convenience: read + parse a manifest file from disk.
pub fn launcher_activity_from_file(path: &Path, package: Option<&str>) -> Result<Option<String>> {
    let xml = std::fs::read_to_string(path)?;
    Ok(launcher_activity(&xml, package))
}

/// Resolve a possibly-relative activity name against the package.
fn resolve(name: &str, package: Option<&str>) -> String {
    match package {
        Some(pkg) if name.starts_with('.') => format!("{pkg}{name}"),
        Some(pkg) if !name.contains('.') => format!("{pkg}.{name}"),
        _ => name.to_string(),
    }
}

/// Yield each `<activity ...>...</activity>` and `<activity-alias>...` block,
/// correctly handling self-closing tags (`<activity .../>`) and not confusing
/// `<activity` with `<activity-alias`.
fn activity_blocks(xml: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    for tag in ["activity", "activity-alias"] {
        let open = format!("<{tag}");
        let close = format!("</{tag}>");
        let mut pos = 0;
        while let Some(rel) = xml[pos..].find(&open) {
            let start = pos + rel;
            let after_name = start + open.len();
            // The char after the tag name must end the name, so `<activity` does
            // not match `<activity-alias`.
            let exact = matches!(
                xml[after_name..].chars().next(),
                Some(c) if c.is_whitespace() || c == '>' || c == '/'
            );
            if !exact {
                pos = after_name;
                continue;
            }
            let Some(tag_end_rel) = xml[start..].find('>') else {
                break;
            };
            let tag_end = start + tag_end_rel;
            if xml.as_bytes()[tag_end - 1] == b'/' {
                // Self-closing: no body, cannot be a launcher.
                blocks.push(xml[start..=tag_end].to_string());
                pos = tag_end + 1;
            } else if let Some(crel) = xml[tag_end..].find(&close) {
                let end = tag_end + crel + close.len();
                blocks.push(xml[start..end].to_string());
                pos = end;
            } else {
                pos = tag_end + 1;
            }
        }
    }
    blocks
}

/// True when a block declares the MAIN action and LAUNCHER category.
fn is_launcher(block: &str) -> bool {
    block.contains("android.intent.action.MAIN")
        && block.contains("android.intent.category.LAUNCHER")
}

/// Extract `key="value"` or `key='value'` from a tag/block. Returns the first match.
fn attr(block: &str, key: &str) -> Option<String> {
    for q in ['"', '\''] {
        let needle = format!("{key}={q}");
        if let Some(start) = block.find(&needle) {
            let rest = &block[start + needle.len()..];
            if let Some(end) = rest.find(q) {
                return Some(rest[..end].to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="com.example.legacy">
    <application android:label="X">
        <activity android:name=".SplashActivity" android:exported="false" />
        <activity android:name=".MainActivity" android:exported="true">
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
        </activity>
    </application>
</manifest>"#;

    #[test]
    fn finds_relative_launcher_and_resolves_against_application_id() {
        let activity = launcher_activity(MANIFEST, Some("com.example.app"));
        assert_eq!(activity, Some("com.example.app.MainActivity".to_string()));
    }

    #[test]
    fn falls_back_to_manifest_package() {
        let activity = launcher_activity(MANIFEST, None);
        assert_eq!(
            activity,
            Some("com.example.legacy.MainActivity".to_string())
        );
    }

    #[test]
    fn handles_fully_qualified_name() {
        let xml = r#"<activity android:name="com.foo.Bar">
            <intent-filter>
                <action android:name="android.intent.action.MAIN"/>
                <category android:name="android.intent.category.LAUNCHER"/>
            </intent-filter></activity>"#;
        assert_eq!(
            launcher_activity(xml, Some("com.example.app")),
            Some("com.foo.Bar".to_string())
        );
    }

    #[test]
    fn no_launcher_returns_none() {
        let xml = r#"<activity android:name=".Lonely" />"#;
        assert_eq!(launcher_activity(xml, Some("com.x")), None);
    }

    #[test]
    fn finds_launcher_in_activity_alias() {
        let xml = r#"<activity android:name=".RealActivity" />
        <activity-alias android:name=".LauncherAlias" android:targetActivity=".RealActivity">
            <intent-filter>
                <action android:name="android.intent.action.MAIN"/>
                <category android:name="android.intent.category.LAUNCHER"/>
            </intent-filter>
        </activity-alias>"#;
        assert_eq!(
            launcher_activity(xml, Some("com.x")),
            Some("com.x.LauncherAlias".to_string())
        );
    }
}
