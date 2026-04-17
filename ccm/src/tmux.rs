use anyhow::{Context, Result};
use std::process::Command;

pub const PREFIX: &str = "ccm-";

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

/// List existing ccm-managed tmux sessions. Returns (full_name, short_name) pairs.
pub fn list_sessions() -> Result<Vec<String>> {
    let output = Command::new("tmux")
        .args(["list-sessions", "-F", "#{session_name}"])
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            // tmux server not running is not an error for us
            if e.kind() == std::io::ErrorKind::NotFound {
                return Ok(vec![]);
            }
            return Err(e).context("failed to spawn tmux");
        }
    };

    if !output.status.success() {
        // "no server running" exits non-zero — treat as empty list
        return Ok(vec![]);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .filter(|l| l.starts_with(PREFIX))
        .map(|l| l.to_string())
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
        .context("failed to spawn tmux new-session")?;
    if !status.success() {
        anyhow::bail!("tmux new-session failed for {full_name}");
    }
    Ok(())
}

pub fn kill_session(full_name: &str) -> Result<()> {
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", full_name])
        .status();
    Ok(())
}

pub fn rename_session(old_full: &str, new_full: &str) -> Result<()> {
    let status = Command::new("tmux")
        .args(["rename-session", "-t", old_full, new_full])
        .status()
        .context("failed to spawn tmux rename-session")?;
    if !status.success() {
        anyhow::bail!("tmux rename-session failed");
    }
    Ok(())
}

/// Build the argv for attaching to a tmux session from inside a pty.
/// We use `-u` for unicode, `-2` for 256-color. `new-session -A` attaches if it
/// exists, creates if it doesn't, so repeated attach is safe.
pub fn attach_command(full_name: &str, path: &str) -> (String, Vec<String>) {
    (
        "tmux".to_string(),
        vec![
            "-u".into(),
            "-2".into(),
            "new-session".into(),
            "-A".into(),
            "-s".into(),
            full_name.into(),
            "-c".into(),
            path.into(),
        ],
    )
}

/// Get the working directory of the first pane of a tmux session, used to
/// repopulate `path` for sessions picked up at launch.
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
    if path.is_empty() {
        None
    } else {
        Some(path)
    }
}
