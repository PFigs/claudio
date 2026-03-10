use std::io::IsTerminal;
use std::path::Path;
use std::process::Stdio;

use clap::{Parser, Subcommand};

use crate::session::mode::SessionMode;

#[derive(Parser)]
#[command(name = "claudio", about = "Voice-activated terminal session manager")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run setup checks and download models without starting the daemon
    Setup,
    /// Start the daemon
    Start {
        /// Run in foreground (don't detach)
        #[arg(short, long)]
        foreground: bool,
    },
    /// Stop the daemon
    Stop,
    /// Open the GUI dashboard (daemon must be running)
    Gui,
    /// Create a new session
    New {
        /// Session name
        #[arg(short, long)]
        name: Option<String>,
        /// Session mode
        #[arg(short, long, default_value = "speaking")]
        mode: SessionMode,
    },
    /// List all sessions
    List,
    /// Set voice input focus to a session
    Focus {
        /// Session name or ID
        session: String,
    },
    /// Change a session's mode
    Mode {
        /// Session name or ID
        session: String,
        /// New mode
        mode: SessionMode,
    },
    /// Destroy a session
    Kill {
        /// Session name or ID
        session: String,
    },
    /// Next voice input runs as shell command
    Shell {
        /// Session name or ID
        session: String,
    },
}

pub async fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        None => {
            if std::io::stdout().is_terminal() {
                // Interactive: ensure daemon is running, then launch GUI
                launch().await
            } else {
                // Piped: run as voice-to-text passthrough
                crate::pipe::run().await
            }
        }
        Some(Command::Setup) => {
            crate::setup::run_setup().await?;
            Ok(())
        }
        Some(Command::Start { foreground }) => {
            if foreground {
                crate::daemon::run().await
            } else {
                start_daemon_background()?;
                Ok(())
            }
        }
        Some(Command::Stop) => {
            let config = crate::config::Config::load()?;
            match crate::ipc::client::send_stop(&config.socket_path()).await {
                Ok(()) => {
                    println!("Daemon stopped.");
                    Ok(())
                }
                Err(_) => {
                    // Daemon IPC is dead -- force-clean stale state
                    force_cleanup(&config);
                    Ok(())
                }
            }
        }
        Some(Command::Gui) => {
            let config = crate::config::Config::load()?;
            crate::gui::run(&config.socket_path())
        }
        Some(Command::New { name, mode }) => {
            let config = crate::config::Config::load()?;
            let resp = crate::ipc::client::send_new_session(
                &config.socket_path(),
                name,
                mode,
            )
            .await?;
            println!("Created session: {} ({})", resp.name, resp.id);
            Ok(())
        }
        Some(Command::List) => {
            let config = crate::config::Config::load()?;
            let sessions = crate::ipc::client::send_list(&config.socket_path()).await?;
            if sessions.is_empty() {
                println!("No sessions.");
            } else {
                println!("{:<10} {:<15} {:<10} {:<7}", "ID", "NAME", "MODE", "FOCUS");
                for s in sessions {
                    let focus = if s.focused { "*" } else { "" };
                    println!("{:<10} {:<15} {:<10} {:<7}", s.id, s.name, s.mode, focus);
                }
            }
            Ok(())
        }
        Some(Command::Focus { session }) => {
            let config = crate::config::Config::load()?;
            crate::ipc::client::send_focus(&config.socket_path(), &session).await?;
            println!("Focused: {session}");
            Ok(())
        }
        Some(Command::Mode { session, mode }) => {
            let config = crate::config::Config::load()?;
            crate::ipc::client::send_mode(&config.socket_path(), &session, mode).await?;
            println!("Set {session} to {mode}");
            Ok(())
        }
        Some(Command::Kill { session }) => {
            let config = crate::config::Config::load()?;
            crate::ipc::client::send_kill(&config.socket_path(), &session).await?;
            println!("Killed: {session}");
            Ok(())
        }
        Some(Command::Shell { session }) => {
            let config = crate::config::Config::load()?;
            crate::ipc::client::send_shell(&config.socket_path(), &session).await?;
            println!("Next voice input for {session} will run as shell command.");
            Ok(())
        }
    }
}

/// Ensure daemon is running, then launch TUI.
async fn launch() -> anyhow::Result<()> {
    let config = crate::config::Config::load()?;

    if !daemon_is_running(&config.socket_path()).await {
        // Run setup first
        crate::setup::run_setup().await?;

        println!("[5/6] Starting daemon...");
        start_daemon_background()?;
        wait_for_daemon(&config.socket_path()).await?;

        println!("[6/6] Launching GUI...");
    }

    crate::gui::run(&config.socket_path())
}

async fn daemon_is_running(socket_path: &Path) -> bool {
    crate::ipc::client::send_ping(socket_path).await.is_ok()
}

async fn wait_for_daemon(socket_path: &Path) -> anyhow::Result<()> {
    for _ in 0..60 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if daemon_is_running(socket_path).await {
            return Ok(());
        }
    }
    anyhow::bail!("Daemon did not start within 30 seconds")
}

fn start_daemon_background() -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let config = crate::config::Config::load()?;

    // Determine log file path
    let log_path = config
        .daemon
        .log_file
        .as_ref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            crate::config::Config::runtime_dir().join("claudio.log")
        });

    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let log_file = std::fs::File::create(&log_path)?;

    std::process::Command::new(exe)
        .args(["start", "--foreground"])
        .stdout(Stdio::from(log_file.try_clone()?))
        .stderr(Stdio::from(log_file))
        .stdin(Stdio::null())
        .spawn()?;

    Ok(())
}

/// Kill stale daemon/ML processes and remove socket/pid files when IPC is dead.
fn force_cleanup(config: &crate::config::Config) {
    let pid_path = config.pid_path();

    // Try to kill the daemon process from the PID file
    if let Ok(pid_str) = std::fs::read_to_string(&pid_path) {
        if let Ok(pid) = pid_str.trim().parse::<i32>() {
            unsafe { libc::kill(pid, libc::SIGTERM); }
        }
    }

    // Kill any lingering ok-claude-ml processes
    let _ = std::process::Command::new("pkill")
        .args(["-f", "ok-claude-ml"])
        .status();

    // Clean up stale files
    let _ = std::fs::remove_file(&pid_path);
    let _ = std::fs::remove_file(config.socket_path());
    let ml_sock = config
        .socket_path()
        .parent()
        .unwrap_or(&std::path::PathBuf::from("/tmp"))
        .join("ok_claude_ml.sock");
    let _ = std::fs::remove_file(&ml_sock);

    println!("Cleaned up stale daemon state.");
}
