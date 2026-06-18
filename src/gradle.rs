//! Gradle wrapper helpers: locate `./gradlew`, run tasks with live output, and
//! (when the static project parse is insufficient) introspect via Gradle itself.

use crate::error::{anyhow, Result};
use crate::exec;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;

/// A handle to a project's Gradle wrapper.
pub struct Gradle {
    /// Project root (directory containing `gradlew`).
    pub root: PathBuf,
    gradlew: PathBuf,
}

impl Gradle {
    /// Locate the Gradle wrapper at `root`, preferring `gradlew` (`gradlew.bat`
    /// on Windows). Errors if no wrapper is present.
    pub fn at(root: &Path) -> Result<Self> {
        let candidates = if cfg!(windows) {
            ["gradlew.bat", "gradlew"]
        } else {
            ["gradlew", "gradlew.bat"]
        };
        for name in candidates {
            let path = root.join(name);
            if path.exists() {
                return Ok(Self {
                    root: root.to_path_buf(),
                    gradlew: path,
                });
            }
        }
        Err(anyhow!(
            "No Gradle wrapper (gradlew) found at {}. Is this an Android/Gradle project?",
            root.display()
        ))
    }

    /// Run a task (optionally scoped to a module path like `:app`) with extra
    /// args, streaming output live. Returns the exit status.
    pub fn run_task(&self, task: &str, extra_args: &[&str]) -> Result<ExitStatus> {
        let mut args: Vec<&str> = vec![task];
        args.extend_from_slice(extra_args);
        exec::run_streaming(&self.gradlew, &self.root, &args)
    }

    /// Run an arbitrary set of Gradle args, streaming output live.
    pub fn run(&self, args: &[&str]) -> Result<ExitStatus> {
        exec::run_streaming(&self.gradlew, &self.root, args)
    }

    /// `./gradlew --stop` — stop all Gradle daemons for this project.
    pub fn stop_daemons(&self) -> Result<ExitStatus> {
        self.run(&["--stop"])
    }

    /// Capture `./gradlew :module:tasks --all` for introspection fallback.
    ///
    /// This is the slow path (JVM warmup); the static [`crate::project`] parser
    /// is preferred. Returns raw stdout for the caller to scan.
    pub fn tasks(&self, module: Option<&str>) -> Result<String> {
        let task = match module {
            Some(m) => format!("{}:tasks", m.trim_end_matches(':')),
            None => "tasks".to_string(),
        };
        let output = exec::run_in(&self.gradlew, &self.root, ["-q", &task, "--all"])?;
        Ok(exec::stdout_string(&output))
    }
}
