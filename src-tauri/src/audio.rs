//! Microphone capture, in memory only.
//!
//! The cpal stream is `!Send` on some backends, so it is created, played and
//! dropped entirely inside one OS thread. The rest of the app only sees the
//! shared sample buffer and a peak meter, so nothing here can reach disk.
//!
//! Output format is always 16 kHz mono i16 (what Gemini wants): if the device
//! offers that natively we ask for it, otherwise we capture at the device
//! default and downmix + resample in memory.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// Gemini expects 16 kHz mono.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;
pub const LEVEL_EVENT: &str = "audio://level";
/// 20 Hz is enough for a convincing waveform.
pub const LEVEL_INTERVAL: Duration = Duration::from_millis(50);

const JOIN_POLL: Duration = Duration::from_millis(5);
const READY_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_RECORDING_DURATION: Duration = Duration::from_secs(60);
/// Meter falloff per tick, so the bar does not flicker.
const LEVEL_DECAY: f32 = 0.55;

/// A finished recording, already 16 kHz mono i16. Never persisted.
#[derive(Debug, Clone, Default)]
pub struct AudioClip {
    pub pcm16_mono: Vec<i16>,
    pub sample_rate: u32,
    pub duration_ms: u64,
}

impl AudioClip {
    /// Wraps the PCM in a WAV container in memory: `generateContent` takes a
    /// container format rather than raw PCM.
    pub fn to_wav(&self) -> Vec<u8> {
        let data_len = (self.pcm16_mono.len() * 2) as u32;
        let mut out = Vec::with_capacity(44 + data_len as usize);

        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        out.extend_from_slice(&self.sample_rate.to_le_bytes());
        out.extend_from_slice(&(self.sample_rate * 2).to_le_bytes()); // byte rate
        out.extend_from_slice(&2u16.to_le_bytes()); // block align
        out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());

        for sample in &self.pcm16_mono {
            out.extend_from_slice(&sample.to_le_bytes());
        }
        out
    }
}

/// Microphone inventory for the debug panel.
#[derive(Debug, Clone, Serialize)]
pub struct MicInfo {
    pub available: bool,
    pub device: Option<String>,
    pub sample_rate: u32,
    pub channels: u16,
    pub format: String,
    pub error: Option<String>,
}

impl MicInfo {
    fn missing(error: impl Into<String>) -> Self {
        Self {
            available: false,
            device: None,
            sample_rate: 0,
            channels: 0,
            format: String::new(),
            error: Some(error.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AudioStatus {
    pub level: f32,
    pub recording: bool,
    pub device: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
struct MeterStatus {
    device: Option<String>,
    error: Option<String>,
}

/// Shared peak meter: written by the audio callback, read by the UI ticker.
#[derive(Debug, Default)]
pub struct Meter {
    /// `f32` peak as raw bits; `fetch_max` works because it is non-negative.
    peak_bits: AtomicU32,
    recording: AtomicBool,
    status: Mutex<MeterStatus>,
}

impl Meter {
    pub fn report_peak(&self, peak: f32) {
        let value = peak.clamp(0.0, 1.0);
        if !value.is_finite() {
            return;
        }
        self.peak_bits.fetch_max(value.to_bits(), Ordering::Relaxed);
    }

    /// Reads and clears the peak accumulated since the previous tick.
    pub fn take_level(&self) -> f32 {
        f32::from_bits(self.peak_bits.swap(0, Ordering::Relaxed))
    }

    pub fn recording(&self) -> bool {
        self.recording.load(Ordering::SeqCst)
    }

    pub fn set_recording(&self, recording: bool) {
        self.recording.store(recording, Ordering::SeqCst);
    }

    pub fn set_device(&self, device: Option<String>) {
        if let Ok(mut status) = self.status.lock() {
            status.device = device;
        }
    }

    pub fn set_error(&self, error: Option<String>) {
        if let Ok(mut status) = self.status.lock() {
            status.error = error;
        }
    }

    pub fn status(&self) -> AudioStatus {
        let (device, error) = match self.status.lock() {
            Ok(status) => (status.device.clone(), status.error.clone()),
            Err(_) => (None, Some("microphone status lock poisoned".to_string())),
        };
        AudioStatus {
            level: 0.0,
            recording: self.recording(),
            device,
            error,
        }
    }
}

/// Emits `audio://level` every 50 ms while recording, plus one final idle event
/// on each stop so the waveform collapses immediately.
pub fn spawn_meter(app: AppHandle, meter: Arc<Meter>) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(LEVEL_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut decayed = 0.0f32;
        let mut was_recording = false;

        loop {
            ticker.tick().await;

            let recording = meter.recording();
            let peak = meter.take_level();

            decayed = if recording {
                peak.max(decayed * LEVEL_DECAY)
            } else {
                0.0
            };

            let changed = recording != was_recording;
            was_recording = recording;

            if !recording && !changed {
                continue;
            }

            let status = meter.status();
            let payload = AudioStatus {
                level: decayed,
                ..status
            };
            if let Err(err) = app.emit(LEVEL_EVENT, payload) {
                eprintln!("[audio] level emit failed: {err}");
            }
        }
    });
}

pub fn probe_default_input() -> MicInfo {
    let host = cpal::default_host();
    let Some(device) = host.default_input_device() else {
        return MicInfo::missing("no microphone is available");
    };

    let name = device.name().ok();
    match device.default_input_config() {
        Ok(supported) => {
            let format = format!("{:?}", supported.sample_format());
            let config: cpal::StreamConfig = supported.into();
            MicInfo {
                available: true,
                device: name,
                sample_rate: config.sample_rate.0,
                channels: config.channels,
                format,
                error: None,
            }
        }
        Err(err) => MicInfo {
            available: false,
            device: name,
            sample_rate: 0,
            channels: 0,
            format: String::new(),
            error: Some(format!("the microphone exposes no input format: {err}")),
        },
    }
}

struct ReadyInfo {
    source_rate: u32,
    source_channels: u16,
    device: Option<String>,
}

struct Session {
    buffer: Arc<Mutex<Vec<f32>>>,
    limit_reached: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    source_rate: u32,
    source_channels: u16,
}

pub struct Recorder {
    session: Option<Session>,
    meter: Arc<Meter>,
}

impl Recorder {
    pub fn new(meter: Arc<Meter>) -> Self {
        Self {
            session: None,
            meter,
        }
    }

    pub fn is_recording(&self) -> bool {
        self.session.is_some()
    }

    /// Starts capturing on a background thread and returns once the stream is
    /// actually running, so a press can never silently record nothing.
    pub fn start(&mut self) -> Result<()> {
        if self.session.is_some() {
            return Ok(());
        }

        self.meter.set_error(None);
        self.meter.set_recording(true);

        let buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(
            TARGET_SAMPLE_RATE as usize * 2,
        )));
        let stop = Arc::new(AtomicBool::new(false));
        let limit_reached = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel::<Result<ReadyInfo>>();

        let thread_buffer = Arc::clone(&buffer);
        let thread_stop = Arc::clone(&stop);
        let thread_limit_reached = Arc::clone(&limit_reached);
        let thread_meter = Arc::clone(&self.meter);
        let handle = thread::Builder::new()
            .name("audio-capture".into())
            .spawn(move || {
                capture_thread(
                    thread_buffer,
                    thread_stop,
                    thread_limit_reached,
                    thread_meter,
                    ready_tx,
                )
            })
            .map_err(|err| anyhow!("spawn the capture thread: {err}"))?;

        let ready = ready_rx.recv_timeout(READY_TIMEOUT);

        match ready {
            Ok(Ok(info)) => {
                self.meter.set_device(info.device);
                self.session = Some(Session {
                    buffer,
                    limit_reached,
                    stop,
                    handle: Some(handle),
                    source_rate: info.source_rate,
                    source_channels: info.source_channels,
                });
                Ok(())
            }
            Ok(Err(err)) => {
                stop.store(true, Ordering::SeqCst);
                let _ = handle.join();
                self.fail(err)
            }
            Err(err) => {
                stop.store(true, Ordering::SeqCst);
                let _ = handle.join();
                self.fail(anyhow!("the microphone did not start in time: {err}"))
            }
        }
    }

    fn fail(&self, err: anyhow::Error) -> Result<()> {
        self.meter.set_recording(false);
        self.meter.set_error(Some(format!("{err:#}")));
        Err(err)
    }

    /// Stops the stream, joins the capture thread and returns a 16 kHz mono
    /// i16 clip. Fails only if nothing was recording.
    pub fn stop(&mut self) -> Result<AudioClip> {
        let Some(mut session) = self.session.take() else {
            self.meter.set_recording(false);
            return Err(anyhow!("not recording"));
        };

        session.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = session.handle.take() {
            if handle.join().is_err() {
                eprintln!("[audio] capture thread panicked");
            }
        }

        self.meter.set_recording(false);
        let _ = self.meter.take_level();
        if session.limit_reached.load(Ordering::SeqCst) {
            return Err(anyhow!(
                "recording reached the 60-second limit; release the shortcut and try a shorter clip"
            ));
        }

        let interleaved = {
            let guard = session
                .buffer
                .lock()
                .map_err(|_| anyhow!("audio buffer lock poisoned"))?;
            guard.clone()
        };

        let mono = downmix(&interleaved, session.source_channels);
        let pcm16_mono = resample_to_target(&mono, session.source_rate);
        let duration_ms = (pcm16_mono.len() as u64 * 1000) / u64::from(TARGET_SAMPLE_RATE).max(1);

        Ok(AudioClip {
            pcm16_mono,
            sample_rate: TARGET_SAMPLE_RATE,
            duration_ms,
        })
    }
}

fn capture_thread(
    buffer: Arc<Mutex<Vec<f32>>>,
    stop: Arc<AtomicBool>,
    limit_reached: Arc<AtomicBool>,
    meter: Arc<Meter>,
    ready: mpsc::Sender<Result<ReadyInfo>>,
) {
    match build_stream(buffer, Arc::clone(&limit_reached), Arc::clone(&meter)) {
        Ok((stream, info)) => {
            if ready.send(Ok(info)).is_err() {
                return;
            }
            while !stop.load(Ordering::SeqCst) {
                thread::sleep(JOIN_POLL);
            }
            drop(stream);
        }
        Err(err) => {
            let _ = ready.send(Err(err));
        }
    }
}

struct ChosenConfig {
    config: cpal::StreamConfig,
    format: cpal::SampleFormat,
}

/// Prefers a native 16 kHz mono f32 stream; otherwise falls back to the device
/// default and resamples afterwards.
fn choose_config(device: &cpal::Device) -> Result<ChosenConfig> {
    if let Ok(ranges) = device.supported_input_configs() {
        for range in ranges {
            if range.channels() != 1 || range.sample_format() != cpal::SampleFormat::F32 {
                continue;
            }
            if range.min_sample_rate().0 > TARGET_SAMPLE_RATE
                || range.max_sample_rate().0 < TARGET_SAMPLE_RATE
            {
                continue;
            }
            if let Some(supported) =
                range.try_with_sample_rate(cpal::SampleRate(TARGET_SAMPLE_RATE))
            {
                return Ok(ChosenConfig {
                    config: supported.config(),
                    format: supported.sample_format(),
                });
            }
        }
    }

    let supported = device
        .default_input_config()
        .context("the microphone exposes no input format")?;
    let format = supported.sample_format();
    Ok(ChosenConfig {
        config: supported.into(),
        format,
    })
}

fn build_stream(
    buffer: Arc<Mutex<Vec<f32>>>,
    limit_reached: Arc<AtomicBool>,
    meter: Arc<Meter>,
) -> Result<(cpal::Stream, ReadyInfo)> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("no microphone is available"))?;
    let device_name = device.name().ok();

    let chosen = choose_config(&device)?;
    let source_rate = chosen.config.sample_rate.0;
    let source_channels = chosen.config.channels;
    let max_samples = (source_rate as usize)
        .checked_mul(source_channels as usize)
        .and_then(|samples_per_second| {
            samples_per_second.checked_mul(MAX_RECORDING_DURATION.as_secs() as usize)
        })
        .ok_or_else(|| anyhow!("the microphone format has an unsupported sample rate"))?;

    let stream = match chosen.format {
        cpal::SampleFormat::F32 => {
            let sink = Arc::clone(&buffer);
            let limit = Arc::clone(&limit_reached);
            let peak_meter = Arc::clone(&meter);
            device.build_input_stream(
                &chosen.config,
                move |data: &[f32], _info: &cpal::InputCallbackInfo| {
                    let peak = data
                        .iter()
                        .fold(0.0f32, |acc, sample| acc.max(sample.abs()));
                    append_samples(
                        &sink,
                        data.iter().map(|sample| sample.clamp(-1.0, 1.0)),
                        max_samples,
                        &limit,
                    );
                    peak_meter.report_peak(peak);
                },
                |err| eprintln!("[audio] stream error: {err}"),
                None,
            )
        }
        cpal::SampleFormat::I16 => {
            let sink = Arc::clone(&buffer);
            let limit = Arc::clone(&limit_reached);
            let peak_meter = Arc::clone(&meter);
            device.build_input_stream(
                &chosen.config,
                move |data: &[i16], _info: &cpal::InputCallbackInfo| {
                    let peak = data.iter().fold(0.0f32, |acc, sample| {
                        acc.max((*sample as f32 / 32_768.0).abs())
                    });
                    append_samples(
                        &sink,
                        data.iter().map(|sample| *sample as f32 / 32_768.0),
                        max_samples,
                        &limit,
                    );
                    peak_meter.report_peak(peak);
                },
                |err| eprintln!("[audio] stream error: {err}"),
                None,
            )
        }
        cpal::SampleFormat::U16 => {
            let sink = Arc::clone(&buffer);
            let limit = Arc::clone(&limit_reached);
            let peak_meter = Arc::clone(&meter);
            device.build_input_stream(
                &chosen.config,
                move |data: &[u16], _info: &cpal::InputCallbackInfo| {
                    let peak = data.iter().fold(0.0f32, |acc, sample| {
                        acc.max(((*sample as f32 - 32_768.0) / 32_768.0).abs())
                    });
                    append_samples(
                        &sink,
                        data.iter()
                            .map(|sample| (*sample as f32 - 32_768.0) / 32_768.0),
                        max_samples,
                        &limit,
                    );
                    peak_meter.report_peak(peak);
                },
                |err| eprintln!("[audio] stream error: {err}"),
                None,
            )
        }
        other => return Err(anyhow!("unsupported microphone sample format: {other:?}")),
    }
    .map_err(|err| anyhow!("open the microphone stream: {err}"))?;

    stream
        .play()
        .map_err(|err| anyhow!("start the microphone stream: {err}"))?;

    Ok((
        stream,
        ReadyInfo {
            source_rate,
            source_channels,
            device: device_name,
        },
    ))
}

fn append_samples(
    buffer: &Mutex<Vec<f32>>,
    samples: impl ExactSizeIterator<Item = f32>,
    max_samples: usize,
    limit_reached: &AtomicBool,
) {
    let Ok(mut buffer) = buffer.lock() else {
        return;
    };
    let remaining = max_samples.saturating_sub(buffer.len());
    let incoming = samples.len();
    buffer.extend(samples.take(remaining));
    if incoming > remaining {
        limit_reached.store(true, Ordering::SeqCst);
    }
}

fn downmix(interleaved: &[f32], channels: u16) -> Vec<f32> {
    let count = channels.max(1) as usize;
    if count == 1 {
        return interleaved.to_vec();
    }

    let frames = interleaved.len() / count;
    let mut mono = Vec::with_capacity(frames);
    for frame in 0..frames {
        let base = frame * count;
        let mut sum = 0.0f32;
        for channel in 0..count {
            if let Some(sample) = interleaved.get(base + channel) {
                sum += *sample;
            }
        }
        mono.push(sum / count as f32);
    }
    mono
}

fn resample_to_target(input: &[f32], from_rate: u32) -> Vec<i16> {
    if input.is_empty() {
        return Vec::new();
    }
    if from_rate == 0 || from_rate == TARGET_SAMPLE_RATE {
        return input.iter().map(|sample| to_pcm16(*sample)).collect();
    }

    let ratio = f64::from(TARGET_SAMPLE_RATE) / f64::from(from_rate);
    let out_len = ((input.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);

    for index in 0..out_len {
        let source = index as f64 / ratio;
        let left = source.floor() as usize;
        let frac = (source - left as f64) as f32;
        let a = input.get(left).copied().unwrap_or(0.0);
        let b = input.get(left + 1).copied().unwrap_or(a);
        out.push(to_pcm16(a + (b - a) * frac));
    }
    out
}

fn to_pcm16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * 32_767.0).round() as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_buffer_stops_at_its_limit() {
        let buffer = Mutex::new(Vec::new());
        let limit_reached = AtomicBool::new(false);

        append_samples(&buffer, [0.1, 0.2, 0.3].into_iter(), 2, &limit_reached);

        assert_eq!(*buffer.lock().expect("buffer lock"), vec![0.1, 0.2]);
        assert!(limit_reached.load(Ordering::SeqCst));
    }

    #[test]
    fn recording_buffer_accepts_samples_up_to_its_limit() {
        let buffer = Mutex::new(Vec::new());
        let limit_reached = AtomicBool::new(false);

        append_samples(&buffer, [0.1, 0.2].into_iter(), 2, &limit_reached);

        assert_eq!(*buffer.lock().expect("buffer lock"), vec![0.1, 0.2]);
        assert!(!limit_reached.load(Ordering::SeqCst));
    }
}
