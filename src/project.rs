//! Project intelligence: walk into an arbitrary Android repo and figure out its
//! modules, build variants, applicationId, and launcher activity **without
//! running Gradle** (the slow path). This is the core that makes a CLI
//! "project-aware".
//!
//! Strategy: a fast static parse of `settings.gradle[.kts]` and each module's
//! `build.gradle[.kts]`, plus a source-manifest scan for the launcher activity.
//! Results are cached on disk keyed by the mtimes of the files we read, so
//! repeated invocations in the inner loop are instant. When static parsing is
//! ambiguous, callers can fall back to [`crate::gradle::Gradle::tasks`].
//!
//! Known limitations of the static parser (documented, not silent): product
//! flavor `applicationId` overrides/suffixes are not applied to the base id, and
//! deeply dynamic Gradle (variables, `buildSrc` conventions) may need the Gradle
//! fallback. Build types `debug` and `release` are always assumed to exist.

mod cache;
mod dsl;

use crate::error::{anyhow, Result};
use crate::manifest;
use crate::model::{capitalize, AndroidProject, Module, Variant};
use std::path::{Path, PathBuf};

pub use cache::clear_cache;

/// Discover the project containing `start`, using the on-disk cache when the
/// relevant build files are unchanged since the last discovery.
pub fn discover(start: &Path) -> Result<AndroidProject> {
    let root = find_root(start)?;
    if let Some(cached) = cache::load(&root) {
        return Ok(cached);
    }
    let project = discover_uncached(&root)?;
    cache::store(&root, &project);
    Ok(project)
}

/// Discover without consulting or writing the cache.
pub fn discover_uncached(root: &Path) -> Result<AndroidProject> {
    let modules = parse_modules(root)?;
    let app_module = modules.iter().find(|m| m.is_application).cloned();

    let mut variants = Vec::new();
    let mut application_id = None;
    let mut launch_activity = None;

    if let Some(app) = &app_module {
        let build_file = module_build_file(root, app);
        if let Some(build_file) = build_file {
            let content = std::fs::read_to_string(&build_file).unwrap_or_default();
            variants = parse_variants(&content);
            application_id = dsl::application_id(&content);
        }
        // Launcher activity from the app module's source manifest.
        let manifest_path = root.join(&app.dir).join("src/main/AndroidManifest.xml");
        if manifest_path.exists() {
            if let Ok(Some(activity)) =
                manifest::launcher_activity_from_file(&manifest_path, application_id.as_deref())
            {
                launch_activity = match &application_id {
                    Some(appid) => Some(format!("{appid}/{activity}")),
                    None => Some(activity),
                };
            }
        }
    }

    let default_variant = resolve_default_variant(&variants);

    Ok(AndroidProject {
        root: root.to_string_lossy().to_string(),
        modules,
        app_module: app_module.map(|m| m.path),
        variants,
        default_variant,
        application_id,
        launch_activity,
    })
}

/// Walk up from `start` looking for a `settings.gradle[.kts]` (the Gradle root).
pub fn find_root(start: &Path) -> Result<PathBuf> {
    let start = if start.is_file() {
        start.parent().unwrap_or(start)
    } else {
        start
    };
    let mut dir = Some(start);
    while let Some(d) = dir {
        if d.join("settings.gradle").exists() || d.join("settings.gradle.kts").exists() {
            return Ok(d.to_path_buf());
        }
        // Also accept a bare app dir with a gradlew but no settings (rare).
        if d.join("gradlew").exists() && d.join("build.gradle").exists() {
            return Ok(d.to_path_buf());
        }
        dir = d.parent();
    }
    Err(anyhow!(
        "No Android/Gradle project found at or above {} (no settings.gradle).",
        start.display()
    ))
}

/// Resolve the default variant per convention: `devDebug` → `debug` → first.
fn resolve_default_variant(variants: &[Variant]) -> Option<String> {
    if variants.is_empty() {
        return None;
    }
    let names: Vec<&str> = variants.iter().map(|v| v.name.as_str()).collect();
    for preferred in ["devDebug", "debug"] {
        if names.contains(&preferred) {
            return Some(preferred.to_string());
        }
    }
    Some(variants[0].name.clone())
}

/// Parse module list from `settings.gradle[.kts]` (`include` directives).
fn parse_modules(root: &Path) -> Result<Vec<Module>> {
    let settings = ["settings.gradle", "settings.gradle.kts"]
        .iter()
        .map(|f| root.join(f))
        .find(|p| p.exists());

    let mut paths: Vec<String> = match settings {
        Some(p) => dsl::included_modules(&std::fs::read_to_string(&p).unwrap_or_default()),
        None => Vec::new(),
    };

    // Single-module projects often only have a root build.gradle; treat ":" as app.
    if paths.is_empty()
        && (root.join("build.gradle").exists() || root.join("build.gradle.kts").exists())
    {
        paths.push(":".to_string());
    }

    // Version-catalog plugin aliases for the application plugin, so modules that
    // apply it via `alias(libs.plugins.…)` are still detected as the app.
    let catalog = read_version_catalog(root);
    let app_aliases = dsl::application_plugin_aliases(&catalog);

    let mut modules = Vec::new();
    for path in paths {
        let dir = gradle_path_to_dir(&path);
        let build_file = ["build.gradle", "build.gradle.kts"]
            .iter()
            .map(|f| root.join(&dir).join(f))
            .find(|p| p.exists());
        let is_application = build_file
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|c| dsl::is_application_module(&c, &app_aliases))
            .unwrap_or(false);
        modules.push(Module {
            path,
            dir,
            is_application,
        });
    }
    Ok(modules)
}

/// Read the project's Gradle version catalog (`gradle/libs.versions.toml`),
/// returning its text or an empty string when absent.
fn read_version_catalog(root: &Path) -> String {
    std::fs::read_to_string(root.join("gradle/libs.versions.toml")).unwrap_or_default()
}

/// `:feature:home` → `feature/home`; `:app` → `app`; `:` → `.`.
fn gradle_path_to_dir(path: &str) -> String {
    let trimmed = path.trim_start_matches(':');
    if trimmed.is_empty() {
        ".".to_string()
    } else {
        trimmed.replace(':', "/")
    }
}

fn module_build_file(root: &Path, module: &Module) -> Option<PathBuf> {
    ["build.gradle", "build.gradle.kts"]
        .iter()
        .map(|f| root.join(&module.dir).join(f))
        .find(|p| p.exists())
}

/// Compute variants from a module's build script: build types × flavor combos.
fn parse_variants(build_script: &str) -> Vec<Variant> {
    let android = dsl::block_body(build_script, "android").unwrap_or_default();

    // Build types: declared names ∪ the always-present {debug, release}.
    let mut build_types =
        dsl::child_block_names(&dsl::block_body(&android, "buildTypes").unwrap_or_default());
    for default in ["debug", "release"] {
        if !build_types.iter().any(|b| b == default) {
            build_types.push(default.to_string());
        }
    }
    // Keep debug first (it's the typical default), then release, then the rest.
    build_types.sort_by_key(|b| match b.as_str() {
        "debug" => 0,
        "release" => 1,
        _ => 2,
    });

    let dimension_groups = dsl::flavor_dimensions(&android);

    if dimension_groups.is_empty() {
        // No flavors → one variant per build type.
        return build_types
            .into_iter()
            .map(|bt| Variant {
                name: bt.clone(),
                build_type: bt,
                flavors: Vec::new(),
            })
            .collect();
    }

    // Cartesian product across dimensions, then × build types.
    let flavor_combos = cartesian(&dimension_groups);
    let mut variants = Vec::new();
    for combo in &flavor_combos {
        for bt in &build_types {
            variants.push(Variant {
                name: variant_name(combo, bt),
                build_type: bt.clone(),
                flavors: combo.clone(),
            });
        }
    }
    variants
}

/// camelCase variant name: `[dev] + debug` → `devDebug`;
/// `[free, blue] + release` → `freeBlueRelease`.
fn variant_name(flavors: &[String], build_type: &str) -> String {
    let mut name = String::new();
    for (i, f) in flavors.iter().enumerate() {
        if i == 0 {
            name.push_str(f);
        } else {
            name.push_str(&capitalize(f));
        }
    }
    name.push_str(&capitalize(build_type));
    name
}

/// Cartesian product of ordered dimension groups, preserving order.
fn cartesian(groups: &[Vec<String>]) -> Vec<Vec<String>> {
    let mut result: Vec<Vec<String>> = vec![Vec::new()];
    for group in groups {
        let mut next = Vec::new();
        for prefix in &result {
            for item in group {
                let mut combo = prefix.clone();
                combo.push(item.clone());
                next.push(combo);
            }
        }
        result = next;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_flavors_yields_build_types_with_debug_first() {
        let script = r#"
            android {
                defaultConfig { applicationId "com.example.app" }
                buildTypes {
                    release { minifyEnabled true }
                }
            }
        "#;
        let variants = parse_variants(script);
        let names: Vec<&str> = variants.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["debug", "release"]);
        assert_eq!(
            resolve_default_variant(&variants),
            Some("debug".to_string())
        );
    }

    #[test]
    fn single_dimension_flavors_combine_with_build_types() {
        let script = r#"
            android {
                flavorDimensions "env"
                productFlavors {
                    dev { dimension "env" }
                    prod { dimension "env" }
                }
                buildTypes {
                    debug {}
                    release {}
                }
            }
        "#;
        let variants = parse_variants(script);
        let names: Vec<String> = variants.iter().map(|v| v.name.clone()).collect();
        assert!(names.contains(&"devDebug".to_string()));
        assert!(names.contains(&"devRelease".to_string()));
        assert!(names.contains(&"prodDebug".to_string()));
        assert!(names.contains(&"prodRelease".to_string()));
        assert_eq!(names.len(), 4);
        assert_eq!(
            resolve_default_variant(&variants),
            Some("devDebug".to_string())
        );
    }

    #[test]
    fn multi_dimension_flavors_produce_full_product() {
        let script = r#"
            android {
                flavorDimensions "tier", "color"
                productFlavors {
                    free { dimension "tier" }
                    paid { dimension "tier" }
                    blue { dimension "color" }
                }
                buildTypes { debug {} release {} }
            }
        "#;
        let variants = parse_variants(script);
        let names: Vec<String> = variants.iter().map(|v| v.name.clone()).collect();
        // 2 (tier) × 1 (color) × 2 (build types) = 4 variants
        assert!(names.contains(&"freeBlueDebug".to_string()));
        assert!(names.contains(&"paidBlueRelease".to_string()));
        assert_eq!(names.len(), 4);
    }

    #[test]
    fn variant_name_camelcases() {
        assert_eq!(variant_name(&["dev".into()], "debug"), "devDebug");
        assert_eq!(
            variant_name(&["free".into(), "blue".into()], "release"),
            "freeBlueRelease"
        );
        assert_eq!(variant_name(&[], "debug"), "Debug");
    }

    #[test]
    fn gradle_path_to_dir_maps_nested_modules() {
        assert_eq!(gradle_path_to_dir(":app"), "app");
        assert_eq!(gradle_path_to_dir(":feature:home"), "feature/home");
        assert_eq!(gradle_path_to_dir(":"), ".");
    }
}
