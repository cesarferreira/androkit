//! # androkit
//!
//! A reusable Android developer toolkit for Rust. It contains the
//! presentation-free plumbing that project-aware Android CLIs share — ADB
//! operations, APK/manifest parsing, Gradle invocation, and static Gradle
//! project discovery — so that tools like `adev` and `dab` don't each
//! reimplement it.
//!
//! The library never prints, colors, or prompts. Every function returns data
//! ([`model`] structs or plain values) or an [`error::Result`]; the calling CLI
//! owns all rendering and interaction.
//!
//! ## Modules
//! - [`adb`] — device discovery, app lifecycle, install, logcat, media, etc.
//! - [`apk`] — APK / XAPK / APKM metadata extraction (aapt + ZIP fallback).
//! - [`manifest`] — source `AndroidManifest.xml` parsing (launcher activity).
//! - [`gradle`] — locate and invoke the Gradle wrapper.
//! - [`project`] — static project discovery (modules, variants, applicationId).
//! - [`model`] — serde-serializable data types shared across the above.
//! - [`exec`] — command-execution helpers.

pub mod adb;
pub mod apk;
pub mod error;
pub mod exec;
pub mod gradle;
pub mod manifest;
pub mod model;
pub mod project;

pub use error::{Error, Result};
