use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use tokio::signal;
use tokio::sync::{broadcast, Mutex};
use tracing::{info, warn};

use crate::audio::capture::AudioCapture;
use crate::config::Config;
use crate::hotkey::evdev::PttListener;
use crate::ipc::events::DaemonEvent;
use crate::ipc::server::IpcServer;
use crate::ml_bridge::client::{MlBridgeClient, MlServiceConfig, MlServiceProcess, VAD_SPEECH_END};
use crate::session::manager::SessionManager;

pub async fn run() -> Result<()> {
    let config = Config::load()?;
    info!("Starting ok_claude daemon");

    // Write PID file
    let pid_path = config.pid_path();
    if let Some(parent) = pid_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&pid_path, std::process::id().to_string())?;

    // Event broadcast channel
    let (event_tx, _) = broadcast::channel::<DaemonEvent>(256);

    // PTT watch channel (placeholder until real PTT starts)
    let (ptt_tx, ptt_rx) = tokio::sync::watch::channel(false);

    // Bind IPC socket immediately so clients can connect
    let manager = SessionManager::new();
    let server = Arc::new(IpcServer::new(
        config.socket_path(),
        manager,
        event_tx.clone(),
        ptt_rx.clone(),
    )?);
    let session_manager = server.manager();

    // Start IPC accept loop right away (before ML/audio/PTT setup)
    let server_handle = tokio::spawn(Arc::clone(&server).run());

    // ML service socket path
    let ml_socket_path = config
        .socket_path()
        .parent()
        .unwrap_or(&PathBuf::from("/tmp"))
        .join("ok_claude_ml.sock");

    // Start ML service
    let ml_service_dir = find_ml_service_dir()?;

    let ml_log_path = config
        .socket_path()
        .parent()
        .unwrap_or(&PathBuf::from("/tmp"))
        .join("ok_claude_ml.log");

    let ml_config = MlServiceConfig {
        ml_service_dir,
        stt_model: config.stt.model.clone(),
        stt_language: config.stt.language.clone(),
        tts_model: config.tts.model.clone(),
        log_path: Some(ml_log_path),
    };

    let mut ml_process = MlServiceProcess::start(ml_socket_path.clone(), &ml_config).await?;
    let ml_client = Arc::new(Mutex::new(MlBridgeClient::new(ml_socket_path)));

    // Run the main daemon loop; always clean up ML process regardless of outcome
    let result =
        run_with_ml(&config, server_handle, session_manager, &ml_client, &event_tx, ptt_tx).await;

    // Cleanup (runs on both success and error)
    ml_client.lock().await.disconnect();
    if let Err(e) = ml_process.stop().await {
        warn!("Error stopping ML service: {e}");
    }
    let _ = std::fs::remove_file(&pid_path);
    let _ = std::fs::remove_file(config.socket_path());
    info!("Daemon stopped");
    result
}

async fn run_with_ml(
    config: &Config,
    server_handle: tokio::task::JoinHandle<Result<()>>,
    session_manager: Arc<Mutex<SessionManager>>,
    ml_client: &Arc<Mutex<MlBridgeClient>>,
    event_tx: &broadcast::Sender<DaemonEvent>,
    ptt_placeholder_tx: tokio::sync::watch::Sender<bool>,
) -> Result<()> {
    // Verify ML service is reachable
    ml_client.lock().await.ping().await?;
    info!("ML service connected");

    // Start audio capture
    let capture = AudioCapture::new(config.audio.sample_rate, 512);
    let (mut audio_rx, _stream_handle) = capture.start()?;

    // Start PTT listener
    let ptt = PttListener::new(&config.hotkey.ptt_key, config.hotkey.device.clone())?;
    let (ptt_rx, _ptt_handle) = ptt.run().await?;

    // Forward real PTT state to the placeholder watch channel used by IPC server
    let ptt_fwd = ptt_rx.clone();
    let ptt_event_tx = event_tx.clone();
    tokio::spawn(async move {
        let mut rx = ptt_fwd.clone();
        let mut prev = false;
        loop {
            if rx.changed().await.is_err() {
                break;
            }
            let active = *rx.borrow();
            if active != prev {
                prev = active;
                let _ = ptt_placeholder_tx.send(active);
                let _ = ptt_event_tx.send(DaemonEvent::PttState { active });
            }
        }
    });

    // Audio pipeline task: PTT -> VAD -> STT -> session dispatch
    let pipeline_ml = Arc::clone(ml_client);
    let pipeline_mgr = Arc::clone(&session_manager);
    let pipeline_ptt = ptt_rx.clone();
    let pipeline_event_tx = event_tx.clone();

    let pipeline_handle = tokio::spawn(async move {
        let mut speech_buffer: Vec<i16> = Vec::new();
        let mut was_pressed = false;
        let mut was_speaking = false;

        loop {
            let chunk = match audio_rx.recv().await {
                Some(c) => c,
                None => break,
            };

            let ptt_active = *pipeline_ptt.borrow();

            if !ptt_active {
                if was_pressed && !speech_buffer.is_empty() {
                    if was_speaking {
                        let _ = pipeline_event_tx.send(DaemonEvent::VadState { speaking: false });
                        was_speaking = false;
                    }
                    // PTT just released -- flush whatever we have
                    let audio = std::mem::take(&mut speech_buffer);
                    if let Err(e) =
                        handle_utterance(&audio, &pipeline_ml, &pipeline_mgr, &pipeline_event_tx)
                            .await
                    {
                        warn!("Utterance handling error: {e}");
                    }
                }
                was_pressed = false;
                continue;
            }

            if !was_pressed {
                // PTT just pressed -- start fresh
                was_speaking = false;
            }
            was_pressed = true;

            // Feed chunk to VAD
            let pcm_bytes = samples_to_bytes(&chunk);
            let mut ml = pipeline_ml.lock().await;
            match ml.vad_feed(&pcm_bytes).await {
                Ok(status) => {
                    speech_buffer.extend_from_slice(&chunk);

                    // Track VAD speaking state
                    if status == crate::ml_bridge::client::VAD_SPEECH_START && !was_speaking {
                        was_speaking = true;
                        let _ = pipeline_event_tx.send(DaemonEvent::VadState { speaking: true });
                    }

                    if status == VAD_SPEECH_END && !speech_buffer.is_empty() {
                        was_speaking = false;
                        let _ = pipeline_event_tx.send(DaemonEvent::VadState { speaking: false });
                        let audio = std::mem::take(&mut speech_buffer);
                        drop(ml); // Release lock before handling
                        if let Err(e) = handle_utterance(
                            &audio,
                            &pipeline_ml,
                            &pipeline_mgr,
                            &pipeline_event_tx,
                        )
                        .await
                        {
                            warn!("Utterance handling error: {e}");
                        }
                    }
                }
                Err(e) => {
                    warn!("VAD error: {e}");
                    speech_buffer.extend_from_slice(&chunk);
                }
            }
        }
    });

    // Wait for IPC server or shutdown signal
    tokio::select! {
        result = server_handle => {
            result??;
        }
        _ = signal::ctrl_c() => {
            info!("Received SIGINT, shutting down");
        }
    }

    pipeline_handle.abort();
    Ok(())
}

/// Transcribe audio and route to the focused session.
async fn handle_utterance(
    audio: &[i16],
    ml: &Arc<Mutex<MlBridgeClient>>,
    manager: &Arc<Mutex<SessionManager>>,
    event_tx: &broadcast::Sender<DaemonEvent>,
) -> Result<()> {
    let pcm_bytes = samples_to_bytes(audio);
    let text = ml.lock().await.transcribe(&pcm_bytes).await?;
    let text = text.trim().to_string();

    if text.is_empty() {
        info!("STT returned empty text, skipping");
        return Ok(());
    }

    info!("Transcribed: {text}");

    // Get session info and send input under a short lock
    let mgr = manager.lock().await;
    let session = match mgr.focused_session() {
        Some(s) => s,
        None => {
            warn!("No focused session to route transcription to");
            return Ok(());
        }
    };

    let session_id = session.id.clone();
    info!("Routing to '{}' (mode: {})", session.name, session.mode);

    let _ = event_tx.send(DaemonEvent::Transcription {
        session_id: session_id.clone(),
        text: text.clone(),
    });

    Ok(())
}

fn samples_to_bytes(samples: &[i16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for &sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

pub fn find_ml_service_dir() -> Result<PathBuf> {
    // Try relative to cwd first, then relative to binary
    let candidates = [
        PathBuf::from("ml_service"),
        std::env::current_dir()?.join("ml_service"),
    ];

    for candidate in &candidates {
        if candidate.join("pyproject.toml").exists() {
            return Ok(candidate.clone());
        }
    }

    anyhow::bail!(
        "Could not find ml_service directory. Run claudio from the project root."
    )
}
