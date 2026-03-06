use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleRate, Stream};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{error, info};

/// Audio capture from the default input device at 16kHz mono i16 PCM.
pub struct AudioCapture {
    sample_rate: u32,
    chunk_size: usize,
}

impl AudioCapture {
    pub fn new(sample_rate: u32, chunk_size: usize) -> Self {
        Self {
            sample_rate,
            chunk_size,
        }
    }

    /// Start capturing audio. Returns a receiver for i16 PCM chunks and the
    /// cpal Stream handle (must be kept alive).
    pub fn start(&self) -> Result<(tokio_mpsc::Receiver<Vec<i16>>, Stream)> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("No audio input device found")?;

        info!("Using input device: {}", device.name().unwrap_or_default());

        let config = cpal::StreamConfig {
            channels: 1,
            sample_rate: SampleRate(self.sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let (tx, rx) = tokio_mpsc::channel::<Vec<i16>>(64);
        let chunk_size = self.chunk_size;
        let mut buffer: Vec<i16> = Vec::with_capacity(chunk_size);

        let stream = device
            .build_input_stream(
                &config,
                move |data: &[i16], _info: &cpal::InputCallbackInfo| {
                    buffer.extend_from_slice(data);
                    while buffer.len() >= chunk_size {
                        let chunk: Vec<i16> = buffer.drain(..chunk_size).collect();
                        if tx.try_send(chunk).is_err() {
                            // Channel full or closed -- drop the chunk
                        }
                    }
                },
                move |err| {
                    error!("Audio input error: {err}");
                },
                None,
            )
            .context("Failed to build input stream")?;

        stream.play().context("Failed to start audio capture")?;
        info!("Audio capture started ({}Hz, {} samples/chunk)", self.sample_rate, self.chunk_size);

        Ok((rx, stream))
    }
}
