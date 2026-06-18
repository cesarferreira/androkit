# androkit

A reusable Android developer toolkit for Rust — the presentation-free plumbing
that project-aware Android CLIs share.

`androkit` was extracted from [`dab`](https://github.com/cesarferreira/dab) so
that tools like [`adev`](https://github.com/cesarferreira/adev) and `dab`
don't each reimplement ADB wrangling, APK parsing, and Gradle project discovery.

The library **never prints, colors, or prompts**. Every function returns data
([`serde`]-serializable structs or plain values) or a `Result`; the calling CLI
owns all rendering and interaction.

## Modules

| Module | What it does |
|---|---|
| `adb` | Device discovery, app lifecycle (`install`, `launch`, `stop`, `clear`), logcat, screenshots/recording, permissions, Wi-Fi/USB. |
| `apk` | APK / XAPK / APKM metadata extraction (`aapt`/`aapt2` with a ZIP-structure fallback). |
| `manifest` | Source `AndroidManifest.xml` parsing — finds the launcher activity and declared package. |
| `gradle` | Locate and invoke the Gradle wrapper, with a tasks-introspection fallback. |
| `project` | **Static** project discovery: modules, build variants, `applicationId`, launcher activity — without running Gradle. Results are cached and invalidated by build-file mtimes. |
| `model` | `serde`-serializable data types shared across the above. |
| `exec` | Command-execution helpers (captured + streaming). |

## Example

```rust
use androkit::project;

let project = project::discover(std::path::Path::new("."))?;
println!("app id: {:?}", project.application_id);
println!("variants: {:?}", project.variants.iter().map(|v| &v.name).collect::<Vec<_>>());

if let Some(variant) = &project.default_variant {
    println!("install task: {}", project.install_task(variant));     // installDevDebug
    println!("test task:    {}", project.unit_test_task(variant));   // testDevDebugUnitTest
}
# Ok::<(), anyhow::Error>(())
```

## Static discovery: scope & limitations

`project::discover` parses `settings.gradle[.kts]` and each module's
`build.gradle[.kts]` statically (no JVM warmup) so the inner loop stays fast. It
handles the conventional Groovy and Kotlin DSL shapes. Known limitations,
surfaced rather than hidden:

- Build types `debug` and `release` are always assumed to exist.
- Product-flavor `applicationId` overrides/suffixes are not applied to the base id.
- Deeply dynamic Gradle (variables, `buildSrc` conventions) may need the
  `gradle::Gradle::tasks` introspection fallback.

## Requirements

- Rust 1.74+
- `adb` on `PATH` for the `adb` module; `aapt`/`aapt2` for full APK metadata.

## License

MIT
