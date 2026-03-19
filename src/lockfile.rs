use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct LockInfo {
    pub pid: u32,
    pub port: u16,
}

/// Write a lock file with the current process PID and the given port.
pub fn write_lock(path: &Path, port: u16) -> Result<()> {
    let info = LockInfo {
        pid: std::process::id(),
        port,
    };
    let json = serde_json::to_string_pretty(&info)?;
    std::fs::write(path, json).context("writing serve lock file")?;
    Ok(())
}

/// Read a lock file. Returns None if the file doesn't exist or the PID is stale.
/// Cleans up stale lock files automatically.
pub fn read_lock(path: &Path) -> Option<LockInfo> {
    let content = std::fs::read_to_string(path).ok()?;
    let info: LockInfo = serde_json::from_str(&content).ok()?;

    // Check if the process is still alive
    if is_process_alive(info.pid) {
        Some(info)
    } else {
        // Stale lock file, clean up
        let _ = std::fs::remove_file(path);
        None
    }
}

/// Remove the lock file.
pub fn remove_lock(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Check if a process with the given PID is still running.
fn is_process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
