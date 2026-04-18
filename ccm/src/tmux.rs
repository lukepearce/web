use anyhow::{Context, Result};
use std::process::Command;

pub const PREFIX: &str = "ccm-";

#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub session: String,
    pub index: u32,
    pub name: String,
}

pub fn full_name(name: &str) -> String {
    if name.starts_with(PREFIX) {
        name.to_string()
    } else {
        format!("{PREFIX}{name}")
    }
}

pub fn short_name(full: &str) -> &str {
    full.strip_prefix(PREFIX).unwrap_or(full)
}

pub fn is_installed() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// List ccm-managed tmux sessions.
pub fn list_sessions() -> Result<Vec<String>> {
    let output = match Command::new("tmux")
        .args(["list-sessions", "-F", "#{session_name}"])
        .output()
    {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e).context("spawn tmux list-sessions"),
    };
    if !output.status.success() {
        // "no server running" exits non-zero — treat as empty.
        return Ok(vec![]);
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| l.starts_with(PREFIX))
        .map(|l| l.to_string())
        .collect())
}

/// List all windows across the given sessions.
pub fn list_windows(sessions: &[String]) -> Result<Vec<WindowInfo>> {
    if sessions.is_empty() {
        return Ok(vec![]);
    }
    let output = Command::new("tmux")
        .args([
            "list-windows",
            "-a",
            "-F",
            "#{session_name}|#{window_index}|#{window_name}",
        ])
        .output()
        .context("spawn tmux list-windows")?;
    if !output.status.success() {
        return Ok(vec![]);
    }
    let set: std::collections::HashSet<&str> = sessions.iter().map(|s| s.as_str()).collect();
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '|');
            let session = parts.next()?.to_string();
            let index: u32 = parts.next()?.parse().ok()?;
            let name = parts.next()?.to_string();
            if set.contains(session.as_str()) {
                Some(WindowInfo { session, index, name })
            } else {
                None
            }
        })
        .collect())
}

pub fn has_session(full_name: &str) -> bool {
    Command::new("tmux")
        .args(["has-session", "-t", full_name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn new_session(full_name: &str, path: &str) -> Result<()> {
    let status = Command::new("tmux")
        .args(["new-session", "-d", "-s", full_name, "-c", path])
        .status()
        .context("spawn tmux new-session")?;
    if !status.success() {
        anyhow::bail!("tmux new-session failed for {full_name}");
    }
    // Disable status bar so it doesn't generate idle ticks that fool our
    // status detection.
    let _ = Command::new("tmux")
        .args(["set-option", "-t", full_name, "status", "off"])
        .status();
    Ok(())
}

pub fn new_window(session: &str, name: &str, path: &str) -> Result<()> {
    let status = Command::new("tmux")
        .args(["new-window", "-t", session, "-n", name, "-c", path])
        .status()
        .context("spawn tmux new-window")?;
    if !status.success() {
        anyhow::bail!("tmux new-window failed");
    }
    Ok(())
}

pub fn kill_window(target: &str) -> Result<()> {
    let _ = Command::new("tmux").args(["kill-window", "-t", target]).status();
    Ok(())
}

pub fn rename_window(target: &str, new_name: &str) -> Result<()> {
    let status = Command::new("tmux")
        .args(["rename-window", "-t", target, new_name])
        .status()
        .context("spawn tmux rename-window")?;
    if !status.success() {
        anyhow::bail!("tmux rename-window failed");
    }
    Ok(())
}

/// Capture the visible contents of the given window (plain text, wrapped
/// lines joined). Used for polling status without attaching.
pub fn capture_pane(target: &str) -> Option<String> {
    let out = Command::new("tmux")
        .args(["capture-pane", "-t", target, "-p", "-J"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

/// argv for attaching to a specific window of a session.
pub fn attach_window_command(session: &str, window_index: u32) -> (String, Vec<String>) {
    (
        "tmux".to_string(),
        vec![
            "-u".into(),
            "-2".into(),
            "attach-session".into(),
            "-t".into(),
            format!("{session}:{window_index}"),
        ],
    )
}

/// Working directory of the given window's active pane.
pub fn session_path(full_name: &str) -> Option<String> {
    let out = Command::new("tmux")
        .args([
            "display-message",
            "-p",
            "-t",
            full_name,
            "#{pane_current_path}",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if path.is_empty() { None } else { Some(path) }
}
