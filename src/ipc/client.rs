use std::path::Path;

use anyhow::{Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::net::unix::OwnedReadHalf;

use super::protocol::{Request, Response, ResponseData, SessionInfo};
use crate::session::mode::SessionMode;

async fn send_request(socket_path: &Path, request: &Request) -> Result<Response> {
    let stream = UnixStream::connect(socket_path)
        .await
        .context("Failed to connect to daemon. Is it running? (ok_claude start)")?;

    let (reader, mut writer) = stream.into_split();

    let json = serde_json::to_string(request)?;
    writer.write_all(json.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.shutdown().await?;

    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    reader.read_line(&mut line).await?;

    let response: Response = serde_json::from_str(line.trim())?;
    Ok(response)
}

fn into_result(response: Response) -> Result<ResponseData> {
    match response {
        Response::Ok { data } => Ok(data),
        Response::Error { message } => Err(anyhow::anyhow!(message)),
    }
}

pub async fn send_stop(socket_path: &Path) -> Result<()> {
    let resp = send_request(socket_path, &Request::Stop).await?;
    into_result(resp)?;
    Ok(())
}

pub async fn send_new_session(
    socket_path: &Path,
    name: Option<String>,
    mode: SessionMode,
    cwd: Option<std::path::PathBuf>,
    command: Option<Vec<String>>,
) -> Result<SessionInfo> {
    let resp = send_request(
        socket_path,
        &Request::New {
            name,
            mode,
            cwd,
            command,
        },
    )
    .await?;
    match into_result(resp)? {
        ResponseData::Session(info) => Ok(info),
        _ => Err(anyhow::anyhow!("unexpected response")),
    }
}

pub async fn send_list(socket_path: &Path) -> Result<Vec<SessionInfo>> {
    let resp = send_request(socket_path, &Request::List).await?;
    match into_result(resp)? {
        ResponseData::Sessions { list } => Ok(list),
        _ => Err(anyhow::anyhow!("unexpected response")),
    }
}

pub async fn send_focus(socket_path: &Path, session: &str) -> Result<()> {
    let resp = send_request(
        socket_path,
        &Request::Focus {
            session: session.into(),
        },
    )
    .await?;
    into_result(resp)?;
    Ok(())
}

pub async fn send_mode(socket_path: &Path, session: &str, mode: SessionMode) -> Result<()> {
    let resp = send_request(
        socket_path,
        &Request::Mode {
            session: session.into(),
            mode,
        },
    )
    .await?;
    into_result(resp)?;
    Ok(())
}

pub async fn send_kill(socket_path: &Path, session: &str) -> Result<()> {
    let resp = send_request(
        socket_path,
        &Request::Kill {
            session: session.into(),
        },
    )
    .await?;
    into_result(resp)?;
    Ok(())
}

pub async fn send_shell(socket_path: &Path, session: &str) -> Result<()> {
    let resp = send_request(
        socket_path,
        &Request::Shell {
            session: session.into(),
        },
    )
    .await?;
    into_result(resp)?;
    Ok(())
}

pub async fn send_ping(socket_path: &Path) -> Result<()> {
    let resp = send_request(socket_path, &Request::Ping).await?;
    into_result(resp)?;
    Ok(())
}

/// Connect to the daemon and subscribe to the event stream.
/// Returns a buffered reader that yields newline-delimited JSON DaemonEvent objects.
#[allow(dead_code)]
pub async fn connect_subscription(socket_path: &Path) -> Result<BufReader<OwnedReadHalf>> {
    let stream = UnixStream::connect(socket_path)
        .await
        .context("Failed to connect to daemon for subscription")?;

    let (reader, mut writer) = stream.into_split();

    // Send Subscribe request
    let json = serde_json::to_string(&Request::Subscribe)?;
    writer.write_all(json.as_bytes()).await?;
    writer.write_all(b"\n").await?;

    let mut reader = BufReader::new(reader);

    // Read the ack response
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let response: Response = serde_json::from_str(line.trim())?;
    into_result(response)?;

    // Return the reader for the caller to consume events from
    Ok(reader)
}
