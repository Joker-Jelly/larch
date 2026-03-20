use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config;
use crate::lockfile;

// ── Path helpers ────────────────────────────────────────────────────

fn larch_exe() -> Result<PathBuf> {
    std::env::current_exe().context("failed to determine larch binary path")
}

fn logs_dir() -> Result<PathBuf> {
    let dir = config::global_config_dir()?.join("logs");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

#[cfg(target_os = "macos")]
fn plist_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home directory"))?;
    Ok(home.join("Library/LaunchAgents/com.larch.serve.plist"))
}

#[cfg(target_os = "linux")]
fn unit_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home directory"))?;
    let dir = home.join(".config/systemd/user");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("larch.service"))
}

// ── Install ─────────────────────────────────────────────────────────

pub fn install(port: u16) -> Result<()> {
    let exe = larch_exe()?;
    let logs = logs_dir()?;

    #[cfg(target_os = "macos")]
    {
        install_launchd(&exe, port, &logs)?;
    }

    #[cfg(target_os = "linux")]
    {
        install_systemd(&exe, port, &logs)?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        anyhow::bail!("service management is only supported on macOS and Linux");
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn install_launchd(exe: &Path, port: u16, logs: &Path) -> Result<()> {
    let plist = plist_path()?;
    let stdout_log = logs.join("larch-stdout.log");
    let stderr_log = logs.join("larch-stderr.log");

    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.larch.serve</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
        <string>serve</string>
        <string>--port</string>
        <string>{port}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{stdout}</string>
    <key>StandardErrorPath</key>
    <string>{stderr}</string>
</dict>
</plist>
"#,
        exe = exe.display(),
        port = port,
        stdout = stdout_log.display(),
        stderr = stderr_log.display(),
    );

    std::fs::write(&plist, &content)
        .with_context(|| format!("failed to write plist to {}", plist.display()))?;

    let status = Command::new("launchctl")
        .args(["load", "-w"])
        .arg(&plist)
        .status()
        .context("failed to run launchctl load")?;

    if !status.success() {
        anyhow::bail!("launchctl load failed with exit code {:?}", status.code());
    }

    println!("✅ Installed launchd service: {}", plist.display());
    println!("   Logs: {}", logs.display());
    Ok(())
}

#[cfg(target_os = "linux")]
fn install_systemd(exe: &Path, port: u16, logs: &Path) -> Result<()> {
    let unit = unit_path()?;

    let content = format!(
        r#"[Unit]
Description=Larch Markdown Knowledge Engine
After=network.target

[Service]
ExecStart={exe} serve --port {port}
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
"#,
        exe = exe.display(),
        port = port,
    );

    std::fs::write(&unit, &content)
        .with_context(|| format!("failed to write unit file to {}", unit.display()))?;

    let reload = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status()
        .context("failed to run systemctl daemon-reload")?;
    if !reload.success() {
        anyhow::bail!("systemctl daemon-reload failed");
    }

    let enable = Command::new("systemctl")
        .args(["--user", "enable", "--now", "larch"])
        .status()
        .context("failed to run systemctl enable")?;
    if !enable.success() {
        anyhow::bail!("systemctl enable --now larch failed");
    }

    println!("✅ Installed systemd user service: {}", unit.display());
    println!("   Logs: journalctl --user -u larch");
    let _ = logs; // logs dir created but systemd uses journal by default
    Ok(())
}

// ── Uninstall ───────────────────────────────────────────────────────

pub fn uninstall() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        uninstall_launchd()?;
    }

    #[cfg(target_os = "linux")]
    {
        uninstall_systemd()?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        anyhow::bail!("service management is only supported on macOS and Linux");
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn uninstall_launchd() -> Result<()> {
    let plist = plist_path()?;

    if plist.exists() {
        let _ = Command::new("launchctl")
            .args(["unload"])
            .arg(&plist)
            .status();

        std::fs::remove_file(&plist)
            .with_context(|| format!("failed to remove {}", plist.display()))?;

        println!("✅ Uninstalled launchd service and removed {}", plist.display());
    } else {
        println!("No launchd service found at {}", plist.display());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn uninstall_systemd() -> Result<()> {
    let unit = unit_path()?;

    if unit.exists() {
        let _ = Command::new("systemctl")
            .args(["--user", "disable", "--now", "larch"])
            .status();

        std::fs::remove_file(&unit)
            .with_context(|| format!("failed to remove {}", unit.display()))?;

        let _ = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status();

        println!("✅ Uninstalled systemd service and removed {}", unit.display());
    } else {
        println!("No systemd service found at {}", unit.display());
    }
    Ok(())
}

// ── Status ──────────────────────────────────────────────────────────

pub fn status() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        status_launchd()?;
    }

    #[cfg(target_os = "linux")]
    {
        status_systemd()?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        anyhow::bail!("service management is only supported on macOS and Linux");
    }

    // Check lockfile regardless of platform
    check_lockfile();

    Ok(())
}

#[cfg(target_os = "macos")]
fn status_launchd() -> Result<()> {
    let plist = plist_path()?;

    if !plist.exists() {
        println!("Service: not installed (no plist found)");
        return Ok(());
    }

    let output = Command::new("launchctl")
        .args(["list"])
        .output()
        .context("failed to run launchctl list")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let found = stdout.lines().find(|line| line.contains("com.larch.serve"));

    match found {
        Some(line) => {
            let parts: Vec<&str> = line.split_whitespace().collect();
            let pid = parts.first().unwrap_or(&"-");
            let exit_code = parts.get(1).unwrap_or(&"-");
            if *pid == "-" {
                println!("Service: installed but not running (last exit: {})", exit_code);
            } else {
                println!("Service: running (PID: {}, last exit: {})", pid, exit_code);
            }
        }
        None => {
            println!("Service: installed but not loaded");
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn status_systemd() -> Result<()> {
    let unit = unit_path()?;

    if !unit.exists() {
        println!("Service: not installed (no unit file found)");
        return Ok(());
    }

    let output = Command::new("systemctl")
        .args(["--user", "is-active", "larch"])
        .output()
        .context("failed to run systemctl is-active")?;

    let state = String::from_utf8_lossy(&output.stdout).trim().to_string();
    println!("Service: {}", state);

    // Show more detail
    let _ = Command::new("systemctl")
        .args(["--user", "status", "larch", "--no-pager"])
        .status();

    Ok(())
}

fn check_lockfile() {
    let lock_path = match config::global_config_dir() {
        Ok(d) => d.join("serve.lock"),
        Err(_) => return,
    };

    // Try the vault-level lock path first, fall back to global
    // For status we check if any lockfile is present
    if let Some(info) = lockfile::read_lock(&lock_path) {
        println!("Lock file: present (PID: {}, port: {})", info.pid, info.port);
    } else {
        println!("Lock file: not found");
    }
}
