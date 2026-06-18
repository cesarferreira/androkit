# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!-- next-header -->
## [Unreleased] - ReleaseDate

## [0.2.0] - 2026-06-18

### Added
- Detect the application module when the Android plugin is applied via a version-catalog alias (`alias(libs.plugins.android.application)`), resolving aliases from `gradle/libs.versions.toml`.
- Follow custom `project(":m").projectDir = file("…")` relocations when locating module build files.

### Changed
- **Breaking:** `project::dsl::is_application_module` now takes the resolved application-plugin aliases.
- Bumped the discovery cache schema so previously cached results are recomputed.

## [0.1.0] - 2026-06-18

### Added
- Initial release: `adb`, `apk`, `manifest`, `gradle`, `project`, `model`, and `exec` modules extracted from `dab`.

<!-- next-url -->
[Unreleased]: https://github.com/cesarferreira/androkit/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/cesarferreira/androkit/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/cesarferreira/androkit/releases/tag/v0.1.0
