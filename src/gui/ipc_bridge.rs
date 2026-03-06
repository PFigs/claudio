use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

use anyhow::{Context, Result};

use crate::ipc::events::DaemonEvent;
use crate::ipc::protocol::{Request, Response};

/// Blocking subscription reader. Runs on a background thread.
/// Connects to daemon, subscribes, and forwards events through the channel.
pub fn run_subscription(socket_path: &Path, tx: flume::Sender<DaemonEvent>) {
    let result = run_subscription_inner(socket_path, &tx);
    if let Err(e) = result {
        tracing::warn!("IPC subscription ended: {e}");
    }
}

fn run_subscription_inner(socket_path: &Path, tx: &flume::Sender<DaemonEvent>) -> Result<()> {
    let stream = UnixStream::connect(socket_path).context("Failed to connect to daemon")?;
    let mut writer = stream.try_clone()?;
    let reader = BufReader::new(stream);

    // Send Subscribe request
    let req = serde_json::to_string(&Request::Subscribe)?;
    writer.write_all(req.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;

    // Read lines (first is ack, rest are events)
    let mut first = true;
    for line in reader.lines() {
        let line = line.context("IPC read error")?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if first {
            // Skip the initial OK ack response
            first = false;
            continue;
        }

        if let Ok(event) = serde_json::from_str::<DaemonEvent>(trimmed) {
            if tx.send(event).is_err() {
                break; // GUI closed
            }
        }
    }

    Ok(())
}

/// Send a one-shot command to the daemon (blocking).
pub fn send_command(socket_path: &Path, request: &Request) -> Result<Response> {
    let stream = UnixStream::connect(socket_path).context("Failed to connect to daemon")?;
    let mut writer = stream.try_clone()?;
    let reader = BufReader::new(stream);

    let json = serde_json::to_string(request)?;
    writer.write_all(json.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;

    let mut line = String::new();
    let mut reader = reader;
    reader.read_line(&mut line)?;

    let response: Response = serde_json::from_str(line.trim())?;
    Ok(response)
}
