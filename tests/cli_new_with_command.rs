//! Integration test: `claudio new --cwd <dir> -- <argv>`
//!
//! Verifies the daemon-side manager records cwd + command on the
//! Session, and that the IPC client round-trips both fields through
//! a Unix socket using the new Request::New schema.

use std::path::PathBuf;
use std::time::Duration;

use ok_claude::ipc::client::send_new_session;
use ok_claude::session::manager::SessionManager;
use ok_claude::session::mode::SessionMode;

#[tokio::test]
async fn create_session_with_cwd_and_command_via_manager() {
    let mut mgr = SessionManager::new();
    let session = mgr.create_session(
        Some("orch-feature".to_string()),
        SessionMode::Listening,
        Some(PathBuf::from("/tmp")),
        Some(vec!["echo".to_string(), "hello".to_string()]),
    );
    assert_eq!(session.cwd.as_deref(), Some(std::path::Path::new("/tmp")));
    assert_eq!(
        session.command.as_deref(),
        Some(["echo".to_string(), "hello".to_string()].as_slice())
    );
    assert!(matches!(session.mode, SessionMode::Listening));
}

#[tokio::test]
async fn send_new_session_round_trip_through_unix_socket() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;

    let dir = tempfile::tempdir().expect("tempdir");
    let socket_path = dir.path().join("test.sock");

    let listener = UnixListener::bind(&socket_path).unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let req: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        let cwd = req["cwd"].as_str().unwrap_or("").to_string();
        let cmd = req["command"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join("|")
            })
            .unwrap_or_default();
        let resp = serde_json::json!({
            "status": "ok",
            "data": {
                "type": "session",
                "id": "abcd1234",
                "name": format!("{cwd}::{cmd}"),
                "mode": "listening",
                "focused": true,
                "busy": false,
            }
        });
        let s = serde_json::to_string(&resp).unwrap();
        writer.write_all(s.as_bytes()).await.unwrap();
        writer.write_all(b"\n").await.unwrap();
    });

    tokio::time::sleep(Duration::from_millis(50)).await;

    let info = send_new_session(
        &socket_path,
        Some("orch".to_string()),
        SessionMode::Listening,
        Some(PathBuf::from("/tmp/wt")),
        Some(vec!["claude".to_string(), "Read starter.md".to_string()]),
    )
    .await
    .expect("send_new_session");

    server.await.unwrap();

    assert_eq!(info.name, "/tmp/wt::claude|Read starter.md");
    assert_eq!(info.mode, "listening");
}
