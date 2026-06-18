//! Serde-serializable data types shared across androkit modules and emitted as
//! JSON by consuming CLIs. None of these types carry presentation concerns
//! (no colors, no formatting) — callers own how they are rendered.

use serde::{Deserialize, Serialize};

/// Properties of a connected device, parsed from `adb shell getprop`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeviceInfo {
    pub serial: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brand: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub android_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub board: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_abi: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_patch: Option<String>,
}

/// Battery state.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Battery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// `/data` storage usage, in gigabytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Storage {
    pub total_gb: f64,
    pub used_gb: f64,
    pub free_gb: f64,
    pub percent_used: f64,
}

/// RAM totals, in gigabytes.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Ram {
    pub total_gb: f64,
    pub free_gb: f64,
}

/// Network reachability for a device.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Network {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssid: Option<String>,
}

/// A device health snapshot (battery / storage / ram / network).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeviceHealth {
    pub device: String,
    pub battery: Battery,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage: Option<Storage>,
    pub ram: Ram,
    pub network: Network,
}

/// IP addresses + WiFi SSID for a device.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkInfo {
    pub device: String,
    pub ip_addresses: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssid: Option<String>,
}

/// Metadata extracted from an APK / XAPK / APKM file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApkInfo {
    pub file: String,
    pub package_name: String,
    pub app_name: String,
    pub version_code: String,
    pub version_name: String,
    pub permissions: Vec<String>,
    /// Present only for XAPK/APKM bundles: the original archive path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_file: Option<String>,
    /// Present only for XAPK/APKM bundles: number of APKs in the bundle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apk_count: Option<usize>,
}

/// A Gradle module within an Android project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    /// Gradle path, e.g. `:app` or `:feature:home`.
    pub path: String,
    /// Filesystem directory of the module, relative to the project root.
    pub dir: String,
    /// True when this module applies the `com.android.application` plugin.
    pub is_application: bool,
}

/// A build variant (build type optionally combined with product flavors).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variant {
    /// camelCase variant name as Gradle would generate it, e.g. `devDebug`.
    pub name: String,
    pub build_type: String,
    pub flavors: Vec<String>,
}

/// Everything androkit can discover about an Android project without running Gradle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AndroidProject {
    /// Absolute project root (directory containing `settings.gradle[.kts]`).
    pub root: String,
    pub modules: Vec<Module>,
    /// Gradle path of the primary application module, e.g. `:app`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_module: Option<String>,
    pub variants: Vec<Variant>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_variant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,
    /// Fully-qualified launcher activity component (`applicationId/Activity`),
    /// suitable for `adb shell am start -n`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_activity: Option<String>,
}

impl AndroidProject {
    /// The Gradle task that installs the given variant onto a device.
    pub fn install_task(&self, variant: &str) -> String {
        format!("install{}", capitalize(variant))
    }

    /// The Gradle task that runs unit tests for the given variant.
    pub fn unit_test_task(&self, variant: &str) -> String {
        format!("test{}UnitTest", capitalize(variant))
    }

    /// The Gradle task that assembles the APK for the given variant.
    pub fn assemble_task(&self, variant: &str) -> String {
        format!("assemble{}", capitalize(variant))
    }
}

/// Uppercase the first character of `s`, leaving the rest untouched.
/// `"devDebug"` → `"DevDebug"`, `"debug"` → `"Debug"`.
pub fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
