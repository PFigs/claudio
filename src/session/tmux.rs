use std::process::Command;

use anyhow::{Context, Result};
use tracing::info;

const TMUX_SESSION: &str = "claudio";

/// Ensure the claudio tmux session exists.
pub fn ensure_session() -> Result<()> {
    let exists = Command::new("tmux")
        .args(["has-session", "-t", TMUX_SESSION])
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !exists {
        Command::new("tmux")
            .args(["new-session", "-d", "-s", TMUX_SESSION])
            .status()
            .context("Failed to create tmux session")?;
        info!("Created tmux session: {TMUX_SESSION}");
    }

    Ok(())
}

/// Create a tmux window running `claude` interactively.
pub fn create_claude_window(window_name: &str) -> Result<()> {
    ensure_session()?;
    Command::new("tmux")
        .args([
            "new-window",
            "-t", TMUX_SESSION,
            "-n", window_name,
            "claude",
        ])
        .status()
        .context("Failed to create tmux window with claude")?;
    info!("Created claude window: {TMUX_SESSION}:{window_name}");
    Ok(())
}

/// Send text as literal input to a tmux window, then press Enter.
pub fn send_keys(window_name: &str, text: &str) -> Result<()> {
    let target = format!("{TMUX_SESSION}:{window_name}");
    // Send text + carriage return as one atomic literal write.
    // This avoids timing issues where Enter arrives before the app processes the text.
    let input = format!("{text}\r");
    Command::new("tmux")
        .args(["send-keys", "-t", &target, "-l", &input])
        .status()
        .context("Failed to send text to tmux window")?;
    Ok(())
}

/// Destroy a tmux window.
pub fn destroy_window(window_name: &str) {
    let target = format!("{TMUX_SESSION}:{window_name}");
    let _ = Command::new("tmux")
        .args(["kill-window", "-t", &target])
        .status();
}

/// Destroy the entire claudio tmux session.
pub fn destroy_session() {
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", TMUX_SESSION])
        .status();
}

/// Capture the current visible content of a tmux pane.
pub fn capture_pane(window_name: &str) -> Result<String> {
    let target = format!("{TMUX_SESSION}:{window_name}");
    let output = Command::new("tmux")
        .args(["capture-pane", "-t", &target, "-p"])
        .output()
        .context("Failed to capture tmux pane")?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
