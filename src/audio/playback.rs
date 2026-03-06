use std::sync::mpsc;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleRate;
use tracing::error;

/// Play raw i16 PCM audio through the default output device.
#[allow(dead_code)]
pub async fn play_audio(pcm: Vec<i16>, sample_rate: u32) -> Result<()> {
    tokio::task::spawn_blocking(move || play_audio_blocking(&pcm, sample_rate)).await?
}

#[allow(dead_code)]
fn play_audio_blocking(pcm: &[i16], sample_rate: u32) -> Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .context("No audio output device found")?;

    let config = cpal::StreamConfig {
        channels: 1,
        sample_rate: SampleRate(sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    let (done_tx, done_rx) = mpsc::channel::<()>();
    let mut pos = 0usize;
    let data = pcm.to_vec();
    let total = data.len();

    let stream = device
        .build_output_stream(
            &config,
            move |output: &mut [i16], _info: &cpal::OutputCallbackInfo| {
                for sample in output.iter_mut() {
                    if pos < total {
                        *sample = data[pos];
                        pos += 1;
                    } else {
                        *sample = 0;
                    }
                }
                if pos >= total {
                    let _ = done_tx.send(());
                }
            },
            move |err| {
                error!("Audio output error: {err}");
            },
            None,
        )
        .context("Failed to build output stream")?;

    stream.play().context("Failed to start audio playback")?;

    // Wait for playback to finish
    let _ = done_rx.recv();

    // Small delay to let the last buffer drain
    std::thread::sleep(std::time::Duration::from_millis(50));

    Ok(())
}
