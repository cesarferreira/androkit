//! ADB client: a presentation-free port of the reusable half of dab's
//! `adb_client.rs`. Every method returns data ([`crate::model`] structs or
//! plain values) instead of printing — callers own rendering.

use crate::error::{anyhow, Result};
use crate::exec;
use crate::model::{DeviceHealth, DeviceInfo, Network, NetworkInfo, Ram, Storage};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// A thin, structured wrapper around the `adb` executable.
pub struct Adb {
    adb_path: PathBuf,
}

impl Adb {
    /// Locate `adb` on `PATH`.
    pub fn new() -> Result<Self> {
        Ok(Self {
            adb_path: exec::find_program("adb")?,
        })
    }

    /// Use an explicit `adb` path (e.g. from `$ANDROID_HOME/platform-tools`).
    pub fn with_path(adb_path: PathBuf) -> Self {
        Self { adb_path }
    }

    /// Run `adb <args>` and capture output.
    pub fn run(&self, args: &[&str]) -> Result<Output> {
        exec::run(&self.adb_path, args)
    }

    /// Run `adb -s <serial> <args>` and capture output.
    fn run_device(&self, serial: &str, args: &[&str]) -> Result<Output> {
        let mut full = vec!["-s", serial];
        full.extend_from_slice(args);
        self.run(&full)
    }

    // ---- devices -------------------------------------------------------

    /// Serial numbers of connected devices. Errors when none are attached.
    pub fn devices(&self) -> Result<Vec<String>> {
        let output = self.run(&["devices", "-l"])?;
        let stdout = exec::stdout_string(&output);
        let devices: Vec<String> = stdout
            .lines()
            .filter(|line| !line.is_empty())
            .filter(|line| !line.contains("daemon not running"))
            .filter(|line| !line.contains("daemon started"))
            .filter(|line| !line.contains("List of devices attached"))
            .filter_map(|line| line.split_whitespace().next().map(|s| s.to_string()))
            .collect();
        if devices.is_empty() {
            return Err(anyhow!(
                "No connected devices found. Connect an Android device via USB (with USB debugging) or start an emulator."
            ));
        }
        Ok(devices)
    }

    /// Parse `adb -s <serial> shell getprop` into a [`DeviceInfo`].
    pub fn device_info(&self, serial: &str) -> Result<DeviceInfo> {
        let output = self.run_device(serial, &["shell", "getprop"])?;
        let stdout = exec::stdout_string(&output);
        let mut info = DeviceInfo {
            serial: serial.to_string(),
            ..Default::default()
        };
        for line in stdout.lines() {
            if let Some((key, value)) = line.split_once("]: [") {
                let key = key.trim_start_matches('[');
                let value = value.trim_end_matches(']').to_string();
                match key {
                    "ro.product.model" => info.model = Some(value),
                    "ro.product.manufacturer" => info.manufacturer = Some(value),
                    "ro.product.brand" => info.brand = Some(value),
                    "ro.product.device" => info.device = Some(value),
                    "ro.product.name" => info.name = Some(value),
                    "ro.build.version.release" => info.android_version = Some(value),
                    "ro.build.version.sdk" => info.sdk = Some(value),
                    "ro.build.version.codename" => info.codename = Some(value),
                    "ro.product.board" => info.board = Some(value),
                    "ro.product.cpu.abi" => info.cpu_abi = Some(value),
                    "ro.product.locale" => info.locale = Some(value),
                    "ro.build.id" => info.build_id = Some(value),
                    "ro.build.version.security_patch" => info.security_patch = Some(value),
                    _ => {}
                }
            }
        }
        Ok(info)
    }

    /// Battery / storage / RAM / network snapshot.
    pub fn device_health(&self, serial: &str) -> Result<DeviceHealth> {
        let mut health = DeviceHealth {
            device: serial.to_string(),
            ..Default::default()
        };

        if let Ok(output) = self.run_device(serial, &["shell", "dumpsys", "battery"]) {
            let stdout = exec::stdout_string(&output);
            for line in stdout.lines() {
                let t = line.trim();
                if let Some(v) = t.strip_prefix("level:") {
                    health.battery.level = Some(v.trim().to_string());
                }
                if let Some(v) = t.strip_prefix("status:") {
                    health.battery.status = Some(v.trim().to_string());
                }
            }
        }

        if let Ok(output) = self.run_device(serial, &["shell", "df", "/data"]) {
            let stdout = exec::stdout_string(&output);
            for line in stdout.lines().skip(1) {
                let cols: Vec<&str> = line.split_whitespace().collect();
                if cols.len() >= 5 {
                    let total_kb = cols[1].replace(',', "").parse::<f64>().unwrap_or(0.0);
                    let used_kb = cols[2].replace(',', "").parse::<f64>().unwrap_or(0.0);
                    let free_kb = cols[3].replace(',', "").parse::<f64>().unwrap_or(0.0);
                    health.storage = Some(Storage {
                        total_gb: round2(total_kb / 1024.0 / 1024.0),
                        used_gb: round2(used_kb / 1024.0 / 1024.0),
                        free_gb: round2(free_kb / 1024.0 / 1024.0),
                        percent_used: if total_kb > 0.0 {
                            round1(used_kb / total_kb * 100.0)
                        } else {
                            0.0
                        },
                    });
                    break;
                }
            }
        }

        if let Ok(output) = self.run_device(serial, &["shell", "cat", "/proc/meminfo"]) {
            let stdout = exec::stdout_string(&output);
            let mut total_kb = None;
            let mut free_kb = None;
            for line in stdout.lines() {
                if let Some(v) = line.strip_prefix("MemTotal:") {
                    total_kb = v
                        .split_whitespace()
                        .next()
                        .and_then(|x| x.parse::<f64>().ok());
                }
                if let Some(v) = line.strip_prefix("MemAvailable:") {
                    free_kb = v
                        .split_whitespace()
                        .next()
                        .and_then(|x| x.parse::<f64>().ok());
                }
            }
            if let (Some(t), Some(f)) = (total_kb, free_kb) {
                health.ram = Ram {
                    total_gb: round2(t / 1024.0 / 1024.0),
                    free_gb: round2(f / 1024.0 / 1024.0),
                };
            }
        }

        let net = self.network_info(serial).unwrap_or_default();
        health.network = Network {
            ip: net.ip_addresses.into_iter().find(|ip| ip != "127.0.0.1"),
            ssid: net.ssid,
        };

        Ok(health)
    }

    /// IP addresses and WiFi SSID for a device.
    pub fn network_info(&self, serial: &str) -> Result<NetworkInfo> {
        let mut info = NetworkInfo {
            device: serial.to_string(),
            ..Default::default()
        };
        if let Ok(output) = self.run_device(serial, &["shell", "ip", "-4", "addr", "show"]) {
            let stdout = exec::stdout_string(&output);
            for line in stdout.lines() {
                if let Some(rest) = line.trim().strip_prefix("inet ") {
                    let ip = rest
                        .split('/')
                        .next()
                        .unwrap_or("")
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_string();
                    if !ip.is_empty() {
                        info.ip_addresses.push(ip);
                    }
                }
            }
        }
        if let Ok(output) = self.run_device(serial, &["shell", "dumpsys", "wifi"]) {
            info.ssid = parse_ssid(&exec::stdout_string(&output));
        }
        Ok(info)
    }

    // ---- packages ------------------------------------------------------

    /// Sorted list of installed package names.
    pub fn list_packages(&self, serial: &str) -> Result<Vec<String>> {
        let output = self.run_device(serial, &["shell", "pm", "list", "packages"])?;
        let stdout = exec::stdout_string(&output);
        let mut names: Vec<String> = stdout
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.replace("package:", "").trim().to_string())
            .collect();
        names.sort_by_key(|a| a.to_lowercase());
        Ok(names)
    }

    /// Resolve the on-device APK path for a package.
    pub fn apk_path_for(&self, serial: &str, package: &str) -> Result<String> {
        let output = self.run_device(serial, &["shell", "pm", "list", "packages", "-f"])?;
        let stdout = exec::stdout_string(&output);
        stdout
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.replace("package:", ""))
            .find(|l| l.trim().ends_with(package.trim()))
            .map(|l| l.replace(&format!("={}", package), ""))
            .ok_or_else(|| anyhow!("Could not find APK path for {package}"))
    }

    // ---- lifecycle -----------------------------------------------------

    /// Start an activity component (`applicationId/Activity`).
    pub fn start_activity(&self, serial: &str, component: &str) -> Result<()> {
        let output = self.run_device(serial, &["shell", "am", "start", "-n", component])?;
        let stderr = exec::stderr_string(&output);
        // `am start` prints status to stderr; treat an explicit Error line as failure.
        if stderr.contains("Error:") {
            return Err(anyhow!("Failed to start {component}: {}", stderr.trim()));
        }
        Ok(())
    }

    /// Launch via the LAUNCHER intent category (no activity name needed).
    pub fn launch_package(&self, serial: &str, package: &str) -> Result<()> {
        self.run_device(
            serial,
            &[
                "shell",
                "monkey",
                "-p",
                package,
                "-c",
                "android.intent.category.LAUNCHER",
                "1",
            ],
        )?;
        Ok(())
    }

    /// Open a URL / deep link via `am start -a VIEW`.
    pub fn launch_url(&self, serial: &str, url: &str) -> Result<()> {
        let output = self.run_device(
            serial,
            &[
                "shell",
                "am",
                "start",
                "-a",
                "android.intent.action.VIEW",
                "-d",
                url,
            ],
        )?;
        let stderr = exec::stderr_string(&output);
        if stderr.contains("Error:") {
            return Err(anyhow!("Failed to launch {url}: {}", stderr.trim()));
        }
        Ok(())
    }

    /// `am force-stop <package>`.
    pub fn stop_app(&self, serial: &str, package: &str) -> Result<()> {
        self.run_device(serial, &["shell", "am", "force-stop", package])?;
        Ok(())
    }

    /// `pm clear <package>` — wipes app data and cache.
    pub fn clear_data(&self, serial: &str, package: &str) -> Result<()> {
        let output = self.run_device(serial, &["shell", "pm", "clear", package])?;
        if exec::stdout_string(&output).contains("Success") {
            Ok(())
        } else {
            Err(anyhow!("Failed to clear data for {package}"))
        }
    }

    /// `uninstall <package>`.
    pub fn uninstall(&self, serial: &str, package: &str) -> Result<()> {
        let output = self.run_device(serial, &["uninstall", package])?;
        if exec::stdout_string(&output).contains("Success") {
            Ok(())
        } else {
            Err(anyhow!(
                "Failed to uninstall {package}: {}",
                exec::stdout_string(&output).trim()
            ))
        }
    }

    /// The PID of a running package, if any (`pidof`).
    pub fn pid_of(&self, serial: &str, package: &str) -> Result<Option<String>> {
        let output = self.run_device(serial, &["shell", "pidof", package])?;
        let pid = exec::stdout_string(&output).trim().to_string();
        Ok(if pid.is_empty() { None } else { Some(pid) })
    }

    // ---- install -------------------------------------------------------

    /// Install a single APK (`install -d`).
    pub fn install_apk(&self, serial: &str, apk: &Path) -> Result<()> {
        if !apk.exists() {
            return Err(anyhow!("File does not exist: {}", apk.display()));
        }
        let output = self.run_device(serial, &["install", "-d", &apk.to_string_lossy()])?;
        if exec::stdout_string(&output).contains("Success") {
            Ok(())
        } else {
            Err(anyhow!(
                "Failed to install APK: {}",
                exec::stderr_string(&output).trim()
            ))
        }
    }

    /// Install a split set (`install-multiple -d`).
    pub fn install_multiple(&self, serial: &str, apks: &[PathBuf]) -> Result<()> {
        if apks.is_empty() {
            return Err(anyhow!("No APKs to install"));
        }
        let paths: Vec<String> = apks
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        let mut args = vec!["-s", serial, "install-multiple", "-d"];
        args.extend(paths.iter().map(|s| s.as_str()));
        let output = self.run(&args)?;
        if exec::stdout_string(&output).contains("Success") {
            Ok(())
        } else {
            Err(anyhow!(
                "Failed to install APKs: {}",
                exec::stderr_string(&output).trim()
            ))
        }
    }

    /// Pull an installed app's APK to `dest` (a file or directory).
    pub fn download_apk(
        &self,
        serial: &str,
        package: &str,
        dest: Option<PathBuf>,
    ) -> Result<PathBuf> {
        let apk_path = self.apk_path_for(serial, package)?;
        let out = resolve_output(dest, &format!("{package}.apk"))?;
        self.run_device(serial, &["pull", &apk_path, &out.to_string_lossy()])?;
        Ok(out)
    }

    // ---- permissions ---------------------------------------------------

    /// Grant runtime permissions to a package.
    pub fn grant(&self, serial: &str, package: &str, permissions: &[&str]) -> Result<()> {
        for perm in permissions {
            self.run_device(serial, &["shell", "pm", "grant", package, perm])?;
        }
        Ok(())
    }

    /// Revoke runtime permissions from a package.
    pub fn revoke(&self, serial: &str, package: &str, permissions: &[&str]) -> Result<()> {
        for perm in permissions {
            self.run_device(serial, &["shell", "pm", "revoke", package, perm])?;
        }
        Ok(())
    }

    // ---- connectivity --------------------------------------------------

    /// Enable ADB over Wi-Fi (TCP/IP 5555) and connect to the device's wlan0 IP.
    /// Returns the `ip:port` that was connected.
    pub fn enable_wifi(&self, serial: &str) -> Result<String> {
        let output = self.run_device(serial, &["shell", "ip", "-4", "addr", "show", "wlan0"])?;
        let stdout = exec::stdout_string(&output);
        let ip = stdout
            .lines()
            .find_map(|line| {
                line.trim()
                    .strip_prefix("inet ")
                    .and_then(|r| r.split('/').next())
                    .map(|s| s.split_whitespace().next().unwrap_or("").to_string())
            })
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("Could not determine device Wi-Fi IP. Is Wi-Fi enabled?"))?;
        self.run_device(serial, &["tcpip", "5555"])?;
        let addr = format!("{ip}:5555");
        self.run(&["connect", &addr])?;
        Ok(addr)
    }

    /// Switch ADB back to USB mode.
    pub fn enable_usb(&self, serial: &str) -> Result<()> {
        let _ = self.run(&["disconnect"]);
        self.run_device(serial, &["usb"])?;
        Ok(())
    }

    // ---- media ---------------------------------------------------------

    /// Capture a screenshot to `dest` (file or directory). Returns the saved path.
    pub fn screenshot(&self, serial: &str, dest: Option<PathBuf>) -> Result<PathBuf> {
        let remote = "/sdcard/screen.png";
        let out = resolve_output(dest, "screen.png")?;
        self.run_device(serial, &["shell", "screencap", "-p", remote])?;
        self.run_device(serial, &["pull", remote, &out.to_string_lossy()])?;
        let _ = self.run_device(serial, &["shell", "rm", remote]);
        Ok(out)
    }

    /// Record the screen until Ctrl+C, then pull the MP4 to `dest`.
    pub fn record_screen(&self, serial: &str, dest: Option<PathBuf>) -> Result<PathBuf> {
        let remote = "/sdcard/demo.mp4";
        let pid_file = "/sdcard/screenrecord.pid";
        let out = resolve_output(dest, "demo.mp4")?;

        let running = Arc::new(AtomicBool::new(true));
        let r = running.clone();
        let serial_owned = serial.to_string();
        let adb_path = self.adb_path.clone();
        let start_cmd =
            format!("screenrecord {remote} & echo $! > {pid_file} && wait $(cat {pid_file})");
        let mut child = Command::new(&self.adb_path)
            .args(["-s", serial, "shell", &start_cmd])
            .spawn()?;

        // Best-effort: on Ctrl+C, SIGINT the on-device screenrecord so it
        // finalizes the MP4 before we pull it.
        let _ = ctrlc::set_handler(move || {
            r.store(false, Ordering::SeqCst);
            if let Ok(o) = Command::new(&adb_path)
                .args(["-s", &serial_owned, "shell", "cat", pid_file])
                .output()
            {
                if let Ok(pid) = String::from_utf8(o.stdout) {
                    let pid = pid.trim();
                    if !pid.is_empty() {
                        let _ = Command::new(&adb_path)
                            .args(["-s", &serial_owned, "shell", "kill", "-2", pid])
                            .output();
                    }
                }
            }
        });

        let status: ExitStatus = child.wait()?;
        running.store(false, Ordering::SeqCst);
        let _ = self.run_device(serial, &["pull", remote, &out.to_string_lossy()]);
        let _ = self.run_device(serial, &["shell", "rm", remote]);
        let _ = self.run_device(serial, &["shell", "rm", pid_file]);
        if !status.success() {
            return Err(anyhow!("Screen recording failed or was interrupted"));
        }
        Ok(out)
    }

    // ---- logcat --------------------------------------------------------

    /// Stream `logcat` to the inherited stdout until Ctrl+C.
    ///
    /// When `pid` is `Some`, only that process's logs are shown
    /// (`logcat --pid=<pid>`).
    pub fn logcat(&self, serial: &str, pid: Option<&str>) -> Result<ExitStatus> {
        let mut args = vec!["-s", serial, "logcat"];
        let pid_arg;
        if let Some(pid) = pid {
            pid_arg = format!("--pid={pid}");
            args.push(&pid_arg);
        }
        let status = Command::new(&self.adb_path).args(&args).status()?;
        Ok(status)
    }
}

/// Resolve an optional output path into a concrete file path, defaulting the
/// filename when `dest` is a directory or absent.
fn resolve_output(dest: Option<PathBuf>, default_name: &str) -> Result<PathBuf> {
    Ok(match dest {
        Some(p) if p.is_dir() => p.join(default_name),
        Some(p) => p,
        None => std::env::current_dir()?.join(default_name),
    })
}

/// Extract the active WiFi SSID from `dumpsys wifi` output, ignoring placeholders.
fn parse_ssid(dumpsys: &str) -> Option<String> {
    for line in dumpsys.lines() {
        if let Some(idx) = line.find("SSID:") {
            let after = &line[idx + 5..];
            let mut ssid = after
                .trim()
                .split(',')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            while ssid.starts_with('"') || ssid.ends_with('"') {
                ssid = ssid.trim_matches('"').to_string();
            }
            ssid = ssid.trim().to_string();
            if !ssid.is_empty() && ssid != "<unknown ssid>" && ssid != "0x0" {
                return Some(ssid);
            }
        }
    }
    None
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ssid_stripping_quotes() {
        let sample = r#"  mWifiInfo SSID: "HomeNet", BSSID: aa:bb"#;
        assert_eq!(parse_ssid(sample), Some("HomeNet".to_string()));
    }

    #[test]
    fn ignores_unknown_ssid() {
        assert_eq!(parse_ssid("SSID: <unknown ssid>"), None);
        assert_eq!(parse_ssid("SSID: 0x0"), None);
    }
}
