//! Command-execution helpers.
//!
//! Two flavours:
//! - [`run`] captures stdout/stderr (for parsing tool output).
//! - [`run_streaming`] inherits the parent's stdio (for live output like
//!   `logcat` or a Gradle build), and lets the terminal forward Ctrl+C to the
//!   child naturally.

use crate::error::{anyhow, Result};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output};

/// Locate an executable on `PATH`, returning a friendly error if missing.
pub fn find_program(name: &str) -> Result<PathBuf> {
    which::which(name).map_err(|_| {
        anyhow!(
            "`{name}` not found in PATH. Please install the Android SDK and ensure `{name}` is available."
        )
    })
}

/// Run `program` with `args`, capturing its output.
pub fn run<I, S>(program: &Path, args: I) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new(program).args(args).output()?;
    Ok(output)
}

/// Run `program` with `args` in `dir`, capturing its output.
pub fn run_in<I, S>(program: &Path, dir: &Path, args: I) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new(program).current_dir(dir).args(args).output()?;
    Ok(output)
}

/// Run `program` with `args` in `dir`, inheriting stdio so output streams live.
///
/// Returns the child's [`ExitStatus`]. Ctrl+C reaches the child directly because
/// it shares the terminal's process group.
pub fn run_streaming<I, S>(program: &Path, dir: &Path, args: I) -> Result<ExitStatus>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let status = Command::new(program).current_dir(dir).args(args).status()?;
    Ok(status)
}

/// Convenience: the trimmed UTF-8 stdout of an [`Output`].
pub fn stdout_string(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Convenience: the trimmed UTF-8 stderr of an [`Output`].
pub fn stderr_string(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}
