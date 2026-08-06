//! Host audio output. Everything cpal-specific lives here; the emulator only
//! ever pushes samples into a queue.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};

use crate::audio::SharedQueue;

pub struct AudioOut {
    /// Dropping this stops playback, so it has to be kept alive.
    _stream: cpal::Stream,
    pub queue: SharedQueue,
    pub sample_rate: f64,
    pub channels: usize,
    pub device_name: String,
}

impl AudioOut {
    pub fn start() -> Result<AudioOut, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no default audio output device")?;
        let device_name = device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|_| "audio device".into());
        let supported = device
            .default_output_config()
            .map_err(|e| format!("no default output config: {e}"))?;
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.config();
        let channels = config.channels as usize;
        let sample_rate = config.sample_rate as f64;

        let queue: SharedQueue = Arc::new(Mutex::new(VecDeque::with_capacity(
            (sample_rate * 0.5) as usize,
        )));

        // Holding the last sample through an underrun is quieter than jumping
        // to silence.
        let mut last = 0.0f32;
        let q = queue.clone();
        let err = |e| eprintln!("audio stream error: {e}");

        let stream = match sample_format {
            SampleFormat::F32 => device.build_output_stream(
                config,
                move |data: &mut [f32], _| {
                    let mut q = q.lock().unwrap();
                    for frame in data.chunks_mut(channels) {
                        last = q.pop_front().unwrap_or(last * 0.9);
                        for s in frame.iter_mut() {
                            *s = last;
                        }
                    }
                },
                err,
                None,
            ),
            SampleFormat::I16 => device.build_output_stream(
                config,
                move |data: &mut [i16], _| {
                    let mut q = q.lock().unwrap();
                    for frame in data.chunks_mut(channels) {
                        last = q.pop_front().unwrap_or(last * 0.9);
                        let v = (last.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                        for s in frame.iter_mut() {
                            *s = v;
                        }
                    }
                },
                err,
                None,
            ),
            SampleFormat::U16 => device.build_output_stream(
                config,
                move |data: &mut [u16], _| {
                    let mut q = q.lock().unwrap();
                    for frame in data.chunks_mut(channels) {
                        last = q.pop_front().unwrap_or(last * 0.9);
                        let v = ((last.clamp(-1.0, 1.0) * 0.5 + 0.5) * u16::MAX as f32) as u16;
                        for s in frame.iter_mut() {
                            *s = v;
                        }
                    }
                },
                err,
                None,
            ),
            other => return Err(format!("unsupported sample format {other:?}")),
        }
        .map_err(|e| format!("could not open audio stream: {e}"))?;

        stream
            .play()
            .map_err(|e| format!("could not start audio: {e}"))?;

        Ok(AudioOut {
            _stream: stream,
            queue,
            sample_rate,
            channels,
            device_name,
        })
    }
}
