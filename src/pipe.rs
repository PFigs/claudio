use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::audio::capture::AudioCapture;
use crate::config::Config;
use crate::daemon::find_ml_service_dir;
use crate::hotkey::evdev::PttListener;
use crate::ml_bridge::client::{MlBridgeClient, MlServiceConfig, MlServiceProcess, VAD_SPEECH_END, VAD_SPEECH_START};

/// Run claudio in pipe mode: merge keyboard + voice transcription to stdout.
///
/// Keyboard input is read from /dev/tty (bypassing the pipe) and forwarded
/// to stdout. Voice transcription from PTT is also written to stdout.
/// Status messages go to stderr.
pub async fn run() -> Result<()> {
    let config = Config::load()?;

    // ML service setup
    let ml_socket_path = config
        .socket_path()
        .parent()
        .unwrap_or(&PathBuf::from("/tmp"))
        .join("ok_claude_ml.sock");

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

    eprintln!("claudio: starting ML service...");
    let mut ml_process = MlServiceProcess::start(ml_socket_path.clone(), &ml_config).await?;
    let ml_client = Arc::new(Mutex::new(MlBridgeClient::new(ml_socket_path)));

    ml_client.lock().await.ping().await?;
    eprintln!("claudio: ML service ready");

    let result = run_pipe_loop(&config, &ml_client).await;

    // Cleanup
    ml_client.lock().await.disconnect();
    if let Err(e) = ml_process.stop().await {
        warn!("Error stopping ML service: {e}");
    }

    result
}

async fn run_pipe_loop(config: &Config, ml_client: &Arc<Mutex<MlBridgeClient>>) -> Result<()> {
    // Start audio capture
    let capture = AudioCapture::new(config.audio.sample_rate, 512);
    let (mut audio_rx, _stream_handle) = capture.start()?;

    // Start PTT listener
    let ptt = PttListener::new(&config.hotkey.ptt_key, config.hotkey.device.clone())?;
    let (ptt_rx, _ptt_handle) = ptt.run().await?;

    eprintln!("claudio: listening (PTT: {})", config.hotkey.ptt_key);

    // Keyboard passthrough: read lines from /dev/tty, write to stdout
    let (line_tx, mut line_rx) = tokio::sync::mpsc::channel::<String>(64);
    std::thread::spawn(move || {
        let tty = match std::fs::File::open("/dev/tty") {
            Ok(f) => f,
            Err(e) => {
                eprintln!("claudio: failed to open /dev/tty: {e}");
                return;
            }
        };
        let reader = io::BufReader::new(tty);
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    if line_tx.blocking_send(l).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Stdout handle (locked once per write for thread safety)
    let stdout = io::stdout();

    // Voice pipeline
    let pipeline_ml = Arc::clone(ml_client);
    let pipeline_ptt = ptt_rx.clone();

    let mut speech_buffer: Vec<i16> = Vec::new();
    let mut was_pressed = false;
    let mut was_speaking = false;

    loop {
        tokio::select! {
            // Keyboard line from /dev/tty
            line = line_rx.recv() => {
                match line {
                    Some(l) => {
                        let mut out = stdout.lock();
                        writeln!(out, "{l}")?;
                        out.flush()?;
                    }
                    None => break, // tty reader closed
                }
            }

            // Audio chunk from microphone
            chunk = audio_rx.recv() => {
                let chunk = match chunk {
                    Some(c) => c,
                    None => break,
                };

                let ptt_active = *pipeline_ptt.borrow();

                if !ptt_active {
                    if was_pressed && !speech_buffer.is_empty() {
                        if was_speaking {
                            eprintln!("claudio: transcribing...");
                            was_speaking = false;
                        }
                        let audio = std::mem::take(&mut speech_buffer);
                        match transcribe(&audio, &pipeline_ml).await {
                            Ok(text) => {
                                if !text.is_empty() {
                                    let mut out = stdout.lock();
                                    writeln!(out, "{text}")?;
                                    out.flush()?;
                                    eprintln!("claudio: [{text}]");
                                }
                            }
                            Err(e) => warn!("Transcription error: {e}"),
                        }
                    }
                    was_pressed = false;
                    continue;
                }

                if !was_pressed {
                    eprintln!("claudio: recording...");
                    was_speaking = false;
                }
                was_pressed = true;

                // Feed chunk to VAD
                let pcm_bytes = samples_to_bytes(&chunk);
                let mut ml = pipeline_ml.lock().await;
                match ml.vad_feed(&pcm_bytes).await {
                    Ok(status) => {
                        speech_buffer.extend_from_slice(&chunk);

                        if status == VAD_SPEECH_START && !was_speaking {
                            was_speaking = true;
                        }

                        if status == VAD_SPEECH_END && !speech_buffer.is_empty() {
                            was_speaking = false;
                            let audio = std::mem::take(&mut speech_buffer);
                            drop(ml);
                            eprintln!("claudio: transcribing...");
                            match transcribe(&audio, &pipeline_ml).await {
                                Ok(text) => {
                                    if !text.is_empty() {
                                        let mut out = stdout.lock();
                                        writeln!(out, "{text}")?;
                                        out.flush()?;
                                        eprintln!("claudio: [{text}]");
                                    }
                                }
                                Err(e) => warn!("Transcription error: {e}"),
                            }
                        }
                    }
                    Err(e) => {
                        warn!("VAD error: {e}");
                        speech_buffer.extend_from_slice(&chunk);
                    }
                }
            }

            // Ctrl+C
            _ = tokio::signal::ctrl_c() => {
                eprintln!("claudio: shutting down");
                break;
            }
        }
    }

    Ok(())
}

async fn transcribe(audio: &[i16], ml: &Arc<Mutex<MlBridgeClient>>) -> Result<String> {
    let pcm_bytes = samples_to_bytes(audio);
    let text = ml.lock().await.transcribe(&pcm_bytes).await?;
    let text = text.trim();
    let text = format!("{text} ");
    info!("Transcribed: {text}");
    Ok(text)
}

fn samples_to_bytes(samples: &[i16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for &sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}
