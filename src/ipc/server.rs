use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{broadcast, Mutex};
use tracing::{error, info, warn};

use super::events::DaemonEvent;
use super::protocol::{Request, Response, ResponseData, SessionInfo};
use crate::session::manager::SessionManager;

pub struct IpcServer {
    listener: UnixListener,
    manager: Arc<Mutex<SessionManager>>,
    shutdown: tokio::sync::watch::Sender<bool>,
    event_tx: broadcast::Sender<DaemonEvent>,
    ptt_rx: tokio::sync::watch::Receiver<bool>,
}

impl IpcServer {
    pub fn new(
        socket_path: PathBuf,
        manager: SessionManager,
        event_tx: broadcast::Sender<DaemonEvent>,
        ptt_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<Self> {
        // Clean up stale socket
        if socket_path.exists() {
            std::fs::remove_file(&socket_path)?;
        }
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let listener = UnixListener::bind(&socket_path)?;
        info!("IPC server listening on {:?}", socket_path);

        let (shutdown, _) = tokio::sync::watch::channel(false);
        Ok(Self {
            listener,
            manager: Arc::new(Mutex::new(manager)),
            shutdown,
            event_tx,
            ptt_rx,
        })
    }

    pub fn manager(&self) -> Arc<Mutex<SessionManager>> {
        Arc::clone(&self.manager)
    }

    #[allow(dead_code)]
    pub fn event_tx(&self) -> broadcast::Sender<DaemonEvent> {
        self.event_tx.clone()
    }

    pub async fn run(self: Arc<Self>) -> Result<()> {
        let mut shutdown_rx = self.shutdown.subscribe();

        loop {
            tokio::select! {
                accept = self.listener.accept() => {
                    let (stream, _) = accept?;
                    let manager = Arc::clone(&self.manager);
                    let shutdown_tx = self.shutdown.clone();
                    let event_tx = self.event_tx.clone();
                    let ptt_rx = self.ptt_rx.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(stream, manager, shutdown_tx, event_tx, ptt_rx).await {
                            error!("Connection error: {e}");
                        }
                    });
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("IPC server shutting down");
                        break;
                    }
                }
            }
        }
        Ok(())
    }
}

async fn handle_connection(
    stream: tokio::net::UnixStream,
    manager: Arc<Mutex<SessionManager>>,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    event_tx: broadcast::Sender<DaemonEvent>,
    ptt_rx: tokio::sync::watch::Receiver<bool>,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    while reader.read_line(&mut line).await? > 0 {
        let request: Request = match serde_json::from_str(line.trim()) {
            Ok(r) => r,
            Err(e) => {
                let resp = Response::error(format!("invalid request: {e}"));
                let json = serde_json::to_string(&resp)?;
                writer.write_all(json.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                line.clear();
                continue;
            }
        };

        // Subscribe is special: switch to streaming mode
        if matches!(request, Request::Subscribe) {
            let resp = Response::ok_empty();
            let json = serde_json::to_string(&resp)?;
            writer.write_all(json.as_bytes()).await?;
            writer.write_all(b"\n").await?;

            return handle_subscription(writer, manager, event_tx, ptt_rx).await;
        }

        let response = dispatch(request, &manager, &shutdown_tx, &event_tx).await;
        let json = serde_json::to_string(&response)?;
        writer.write_all(json.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        line.clear();
    }

    Ok(())
}

async fn handle_subscription(
    mut writer: tokio::net::unix::OwnedWriteHalf,
    manager: Arc<Mutex<SessionManager>>,
    event_tx: broadcast::Sender<DaemonEvent>,
    ptt_rx: tokio::sync::watch::Receiver<bool>,
) -> Result<()> {
    // Send initial snapshot
    let snapshot = build_snapshot(&manager, &ptt_rx).await;
    send_event(&mut writer, &snapshot).await?;

    // Stream events
    let mut rx = event_tx.subscribe();
    loop {
        match rx.recv().await {
            Ok(event) => {
                if send_event(&mut writer, &event).await.is_err() {
                    break; // client disconnected
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                warn!("Subscription lagged by {n} events, re-sending snapshot");
                let snapshot = build_snapshot(&manager, &ptt_rx).await;
                if send_event(&mut writer, &snapshot).await.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }

    Ok(())
}

async fn build_snapshot(
    manager: &Arc<Mutex<SessionManager>>,
    ptt_rx: &tokio::sync::watch::Receiver<bool>,
) -> DaemonEvent {
    let mgr = manager.lock().await;
    let sessions: Vec<SessionInfo> = mgr
        .list_sessions()
        .into_iter()
        .map(|s| SessionInfo {
            id: s.id.clone(),
            name: s.name.clone(),
            mode: s.mode.to_string(),
            focused: mgr.is_focused(&s.id),
            busy: s.busy,
        })
        .collect();
    let focused_id = sessions.iter().find(|s| s.focused).map(|s| s.id.clone());
    DaemonEvent::Snapshot {
        sessions,
        focused_id,
        ptt_active: *ptt_rx.borrow(),
    }
}

async fn send_event(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    event: &DaemonEvent,
) -> Result<(), std::io::Error> {
    let json = serde_json::to_string(event).unwrap();
    writer.write_all(json.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

async fn dispatch(
    request: Request,
    manager: &Arc<Mutex<SessionManager>>,
    shutdown_tx: &tokio::sync::watch::Sender<bool>,
    event_tx: &broadcast::Sender<DaemonEvent>,
) -> Response {
    match request {
        Request::Ping => Response::ok(ResponseData::Pong {}),
        Request::Stop => {
            let _ = shutdown_tx.send(true);
            Response::ok_empty()
        }
        Request::New {
            name,
            mode,
            cwd,
            command,
        } => {
            let mut mgr = manager.lock().await;
            let session = mgr.create_session(name, mode, cwd.clone(), command.clone());
            let id = session.id.clone();
            let name = session.name.clone();
            let mode_str = session.mode.to_string();
            let focused = mgr.is_focused(&id);
            let info = SessionInfo {
                id,
                name,
                mode: mode_str,
                focused,
                busy: false,
            };
            let _ = event_tx.send(DaemonEvent::SessionCreated {
                session: info.clone(),
                cwd,
                command,
            });
            Response::ok(ResponseData::Session(info))
        }
        Request::List => {
            let mgr = manager.lock().await;
            let sessions = mgr
                .list_sessions()
                .into_iter()
                .map(|s| SessionInfo {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    mode: s.mode.to_string(),
                    focused: mgr.is_focused(&s.id),
                    busy: s.busy,
                })
                .collect();
            Response::ok(ResponseData::Sessions { list: sessions })
        }
        Request::Focus { session } => {
            let mut mgr = manager.lock().await;
            match mgr.set_focus(&session) {
                Ok(()) => {
                    let id = mgr
                        .focused_session()
                        .map(|s| s.id.clone())
                        .unwrap_or_default();
                    let _ = event_tx.send(DaemonEvent::SessionFocused { session_id: id });
                    Response::ok_empty()
                }
                Err(e) => Response::error(e.to_string()),
            }
        }
        Request::Mode { session, mode } => {
            let mut mgr = manager.lock().await;
            match mgr.set_mode(&session, mode) {
                Ok(()) => {
                    let _ = event_tx.send(DaemonEvent::SessionModeChanged {
                        session_id: session,
                        mode: mode.to_string(),
                    });
                    Response::ok_empty()
                }
                Err(e) => Response::error(e.to_string()),
            }
        }
        Request::Kill { session } => {
            let mut mgr = manager.lock().await;
            // Resolve ID before destroying
            let resolved = mgr
                .list_sessions()
                .iter()
                .find(|s| s.name == session || s.id.starts_with(&session))
                .map(|s| s.id.clone());
            match mgr.destroy_session(&session) {
                Ok(()) => {
                    if let Some(id) = resolved {
                        let _ = event_tx.send(DaemonEvent::SessionDestroyed { session_id: id });
                    }
                    Response::ok_empty()
                }
                Err(e) => Response::error(e.to_string()),
            }
        }
        Request::Rename { session, new_name } => {
            let mut mgr = manager.lock().await;
            let resolved = mgr
                .list_sessions()
                .iter()
                .find(|s| s.name == session || s.id.starts_with(&session))
                .map(|s| s.id.clone());
            match mgr.rename_session(&session, new_name.clone()) {
                Ok(()) => {
                    if let Some(id) = resolved {
                        let _ = event_tx.send(DaemonEvent::SessionRenamed {
                            session_id: id,
                            new_name,
                        });
                    }
                    Response::ok_empty()
                }
                Err(e) => Response::error(e.to_string()),
            }
        }
        Request::Shell { session } => {
            let mut mgr = manager.lock().await;
            match mgr.set_shell_mode(&session) {
                Ok(()) => Response::ok_empty(),
                Err(e) => Response::error(e.to_string()),
            }
        }
        Request::Subscribe => {
            // Handled in handle_connection before reaching dispatch
            unreachable!()
        }
    }
}
