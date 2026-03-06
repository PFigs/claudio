use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::process::{Child, Command};
use tracing::{info, warn};

// Message types matching Python protocol.py
const MSG_VAD_FEED: u8 = 0x01;
const MSG_TRANSCRIBE: u8 = 0x02;
#[allow(dead_code)]
const MSG_SYNTHESIZE: u8 = 0x03;
const MSG_PING: u8 = 0x04;
const MSG_ERROR: u8 = 0xFF;

// VAD result status bytes
#[allow(dead_code)]
pub const VAD_NO_EVENT: u8 = 0x00;
pub const VAD_SPEECH_START: u8 = 0x01;
pub const VAD_SPEECH_END: u8 = 0x02;

const HEADER_SIZE: usize = 5; // 1 byte msg_type + 4 bytes body_len (u32 LE)

/// Client for the Python ML service over Unix socket with binary framed protocol.
pub struct MlBridgeClient {
    socket_path: PathBuf,
    stream: Option<UnixStream>,
}

impl MlBridgeClient {
    pub fn new(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            stream: None,
        }
    }

    #[allow(dead_code)]
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    async fn ensure_connected(&mut self) -> Result<&mut UnixStream> {
        if self.stream.is_none() {
            let stream = UnixStream::connect(&self.socket_path)
                .await
                .context("Failed to connect to ML service")?;
            self.stream = Some(stream);
        }
        Ok(self.stream.as_mut().unwrap())
    }

    async fn send_frame(&mut self, msg_type: u8, body: &[u8]) -> Result<()> {
        let stream = self.ensure_connected().await?;
        let mut header = [0u8; HEADER_SIZE];
        header[0] = msg_type;
        header[1..5].copy_from_slice(&(body.len() as u32).to_le_bytes());
        stream.write_all(&header).await?;
        stream.write_all(body).await?;
        stream.flush().await?;
        Ok(())
    }

    async fn recv_frame(&mut self) -> Result<(u8, Vec<u8>)> {
        let stream = self.stream.as_mut().context("Not connected")?;
        let mut header = [0u8; HEADER_SIZE];
        stream.read_exact(&mut header).await?;
        let msg_type = header[0];
        let body_len = u32::from_le_bytes(header[1..5].try_into().unwrap()) as usize;
        let mut body = vec![0u8; body_len];
        if body_len > 0 {
            stream.read_exact(&mut body).await?;
        }

        if msg_type == MSG_ERROR {
            let msg = String::from_utf8_lossy(&body);
            bail!("ML service error: {msg}");
        }

        Ok((msg_type, body))
    }

    /// Send an audio chunk to VAD. Returns the VAD status byte.
    pub async fn vad_feed(&mut self, pcm_i16: &[u8]) -> Result<u8> {
        self.send_frame(MSG_VAD_FEED, pcm_i16).await?;
        let (_msg_type, body) = self.recv_frame().await?;
        if body.is_empty() {
            bail!("VAD returned empty response");
        }
        Ok(body[0])
    }

    /// Transcribe a full utterance of i16 PCM audio. Returns the text.
    pub async fn transcribe(&mut self, pcm_i16: &[u8]) -> Result<String> {
        self.send_frame(MSG_TRANSCRIBE, pcm_i16).await?;
        let (_msg_type, body) = self.recv_frame().await?;
        Ok(String::from_utf8(body)?)
    }

    /// Synthesize text to audio. Returns (sample_rate, raw_i16_pcm).
    #[allow(dead_code)]
    pub async fn synthesize(&mut self, text: &str) -> Result<(u32, Vec<u8>)> {
        self.send_frame(MSG_SYNTHESIZE, text.as_bytes()).await?;
        let (_msg_type, body) = self.recv_frame().await?;
        if body.len() < 4 {
            bail!("TTS response too short");
        }
        let sample_rate = u32::from_le_bytes(body[0..4].try_into().unwrap());
        let pcm = body[4..].to_vec();
        Ok((sample_rate, pcm))
    }

    /// Ping the ML service.
    pub async fn ping(&mut self) -> Result<()> {
        self.send_frame(MSG_PING, &[]).await?;
        let (_msg_type, _body) = self.recv_frame().await?;
        Ok(())
    }

    /// Disconnect from the ML service.
    pub fn disconnect(&mut self) {
        self.stream = None;
    }
}

/// Manages the Python ML service child process lifecycle.
#[allow(dead_code)]
pub struct MlServiceProcess {
    child: Option<Child>,
    socket_path: PathBuf,
}

impl MlServiceProcess {
    /// Start the Python ML service as a child process.
    pub async fn start(socket_path: PathBuf, config: &MlServiceConfig) -> Result<Self> {
        info!("Starting ML service...");

        // Remove stale socket from a previous crashed run
        if socket_path.exists() {
            info!("Removing stale ML socket: {}", socket_path.display());
            let _ = std::fs::remove_file(&socket_path);
        }

        let mut cmd = Command::new("uv");
        cmd.arg("run")
            .arg("ok-claude-ml")
            .env("OK_CLAUDE_ML_SOCKET", &socket_path)
            .env("OK_CLAUDE_STT_MODEL", &config.stt_model)
            .current_dir(&config.ml_service_dir);

        if let Some(ref lang) = config.stt_language {
            cmd.env("OK_CLAUDE_STT_LANGUAGE", lang);
        }
        if let Some(ref model) = config.tts_model {
            cmd.env("OK_CLAUDE_TTS_MODEL", model);
        }

        if let Some(ref log_path) = config.log_path {
            if let Some(parent) = log_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let log_file = std::fs::File::create(log_path)
                .context("Failed to create ML service log file")?;
            cmd.stdout(std::process::Stdio::from(log_file.try_clone()?));
            cmd.stderr(std::process::Stdio::from(log_file));
        }

        let child = cmd.spawn().context("Failed to start ML service")?;

        // Wait for the socket to appear
        for i in 0..60 {
            if socket_path.exists() {
                info!("ML service ready (waited {}ms)", i * 100);
                return Ok(Self {
                    child: Some(child),
                    socket_path,
                });
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        bail!("ML service did not start within 6 seconds");
    }

    /// Stop the ML service.
    pub async fn stop(&mut self) -> Result<()> {
        if let Some(ref mut child) = self.child {
            info!("Stopping ML service...");

            // Send SIGTERM
            if let Some(pid) = child.id() {
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            }

            // Wait up to 5 seconds, then force kill
            match tokio::time::timeout(
                std::time::Duration::from_secs(5),
                child.wait(),
            )
            .await
            {
                Ok(Ok(status)) => {
                    info!("ML service exited: {status}");
                }
                Ok(Err(e)) => {
                    warn!("Error waiting for ML service: {e}");
                }
                Err(_) => {
                    warn!("ML service didn't exit in 5s, killing");
                    let _ = child.kill().await;
                }
            }
            self.child = None;
        }
        Ok(())
    }
}

impl Drop for MlServiceProcess {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            if let Some(pid) = child.id() {
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            }
        }
    }
}

pub struct MlServiceConfig {
    pub ml_service_dir: PathBuf,
    pub stt_model: String,
    pub stt_language: Option<String>,
    pub tts_model: Option<String>,
    pub log_path: Option<PathBuf>,
}
