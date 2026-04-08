use std::{
    fmt,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, Sample, SampleFormat, SampleRate, SizedSample, StreamConfig,
};
use parking_lot::Mutex;
use rustfft::{num_complex::Complex32, FftPlanner};
use serde::{Deserialize, Serialize};

const TARGET_SAMPLE_RATE: u32 = 44_100;
const FRAME_SIZE: usize = 2_048;
const DEBUG_RMS_PRINT_THRESHOLD: f32 = 0.03;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectorConfig {
    pub amplitude_threshold: f32,
    pub freq_ratio_threshold: f32,
    pub cooldown_secs: f32,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self {
            amplitude_threshold: 0.08,
            freq_ratio_threshold: 1.15,
            cooldown_secs: 0.55,
        }
    }
}

#[derive(Debug)]
pub enum DetectorError {
    NoInputDevice,
    DefaultConfig(cpal::DefaultStreamConfigError),
    SupportedConfigs(cpal::SupportedStreamConfigsError),
    BuildStream(cpal::BuildStreamError),
    PlayStream(cpal::PlayStreamError),
    DeviceName(cpal::DeviceNameError),
    UnsupportedSampleFormat(String),
}

impl fmt::Display for DetectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoInputDevice => write!(f, "no default microphone input device found"),
            Self::DefaultConfig(err) => write!(f, "failed to query default microphone config: {err}"),
            Self::SupportedConfigs(err) => write!(f, "failed to enumerate microphone configs: {err}"),
            Self::BuildStream(err) => write!(f, "failed to open microphone stream: {err}"),
            Self::PlayStream(err) => write!(f, "failed to start microphone stream: {err}"),
            Self::DeviceName(err) => write!(f, "failed to read microphone device name: {err}"),
            Self::UnsupportedSampleFormat(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for DetectorError {}

pub fn spawn_detector(
    config: Arc<Mutex<DetectorConfig>>,
    on_slap: Arc<dyn Fn(f32) + Send + Sync + 'static>,
    on_error: Arc<dyn Fn(String) + Send + Sync + 'static>,
) {
    thread::spawn(move || {
        if let Err(error) = run_detector(config, on_slap) {
            on_error(error.to_string());
        }
    });
}

fn run_detector(
    config: Arc<Mutex<DetectorConfig>>,
    on_slap: Arc<dyn Fn(f32) + Send + Sync + 'static>,
) -> Result<(), DetectorError> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or(DetectorError::NoInputDevice)?;
    let device_name = device.name().map_err(DetectorError::DeviceName)?;
    println!("Using input device: {device_name}");

    let supported = pick_stream_config(&device)?;
    println!(
        "Opening microphone stream at {} Hz, {} channel(s), {:?}",
        supported.config.sample_rate.0, supported.config.channels, supported.sample_format
    );

    let stream = match supported.sample_format {
        SampleFormat::F32 => build_stream::<f32>(&device, &supported.config, config, on_slap)?,
        SampleFormat::I16 => build_stream::<i16>(&device, &supported.config, config, on_slap)?,
        SampleFormat::U16 => build_stream::<u16>(&device, &supported.config, config, on_slap)?,
        other => {
            return Err(DetectorError::UnsupportedSampleFormat(format!(
                "unsupported microphone sample format: {other:?}"
            )))
        }
    };

    stream.play().map_err(DetectorError::PlayStream)?;

    loop {
        thread::sleep(Duration::from_secs(1));
    }
}

struct SelectedConfig {
    config: StreamConfig,
    sample_format: SampleFormat,
}

fn pick_stream_config(device: &Device) -> Result<SelectedConfig, DetectorError> {
    let target_rate = SampleRate(TARGET_SAMPLE_RATE);

    match device.supported_input_configs() {
        Ok(mut ranges) => {
            if let Some(range) = ranges.find(|range| {
                range.min_sample_rate() <= target_rate && range.max_sample_rate() >= target_rate
            }) {
                let sample_format = range.sample_format();
                let config = range.with_sample_rate(target_rate).config();
                return Ok(SelectedConfig {
                    config,
                    sample_format,
                });
            }
        }
        Err(error) => return Err(DetectorError::SupportedConfigs(error)),
    }

    let default = device
        .default_input_config()
        .map_err(DetectorError::DefaultConfig)?;

    Ok(SelectedConfig {
        config: default.config(),
        sample_format: default.sample_format(),
    })
}

fn build_stream<T>(
    device: &Device,
    config: &StreamConfig,
    shared_config: Arc<Mutex<DetectorConfig>>,
    on_slap: Arc<dyn Fn(f32) + Send + Sync + 'static>,
) -> Result<cpal::Stream, DetectorError>
where
    T: Sample + SizedSample,
    f32: cpal::FromSample<T>,
{
    let mut processor = FrameProcessor::new(config.sample_rate.0, config.channels as usize);
    let error_callback = |error| eprintln!("microphone stream error: {error}");

    device
        .build_input_stream(
            config,
            move |data: &[T], _| processor.push_input(data, &shared_config, &on_slap),
            error_callback,
            None,
        )
        .map_err(DetectorError::BuildStream)
}

struct FrameProcessor {
    sample_rate: u32,
    channels: usize,
    frame: Vec<f32>,
    fft_planner: FftPlanner<f32>,
    last_trigger: Option<Instant>,
    last_debug_print: Option<Instant>,
}

impl FrameProcessor {
    fn new(sample_rate: u32, channels: usize) -> Self {
        Self {
            sample_rate,
            channels: channels.max(1),
            frame: Vec::with_capacity(FRAME_SIZE),
            fft_planner: FftPlanner::new(),
            last_trigger: None,
            last_debug_print: None,
        }
    }

    fn push_input<T>(
        &mut self,
        data: &[T],
        shared_config: &Arc<Mutex<DetectorConfig>>,
        on_slap: &Arc<dyn Fn(f32) + Send + Sync + 'static>,
    ) where
        T: Sample,
        f32: cpal::FromSample<T>,
    {
        for chunk in data.chunks(self.channels) {
            let mono = chunk
                .iter()
                .map(|sample| sample.to_sample::<f32>())
                .sum::<f32>()
                / self.channels as f32;

            self.frame.push(mono);

            if self.frame.len() == FRAME_SIZE {
                let frame = self.frame.clone();
                self.frame.clear();
                self.process_frame(&frame, shared_config, on_slap);
            }
        }
    }

    fn process_frame(
        &mut self,
        frame: &[f32],
        shared_config: &Arc<Mutex<DetectorConfig>>,
        on_slap: &Arc<dyn Fn(f32) + Send + Sync + 'static>,
    ) {
        let prepared = high_pass_frame(frame);
        let rms = compute_rms(&prepared);
        let peak = compute_peak(&prepared);

        if rms >= DEBUG_RMS_PRINT_THRESHOLD
            && self
                .last_debug_print
                .is_none_or(|last| last.elapsed() >= Duration::from_millis(250))
        {
            println!("RMS {rms:.4} peak={peak:.4}");
            self.last_debug_print = Some(Instant::now());
        }

        let config = shared_config.lock().clone();
        if rms < config.amplitude_threshold || peak < (config.amplitude_threshold * 2.2) {
            return;
        }

        if self
            .last_trigger
            .is_some_and(|last| last.elapsed() < Duration::from_secs_f32(config.cooldown_secs))
        {
            return;
        }

        println!("STAGE1 PASS rms={rms:.4} peak={peak:.4}");

        let ratio = self.frequency_ratio(&prepared);
        if ratio >= config.freq_ratio_threshold {
            let force = rms.clamp(0.05, 1.0);
            self.last_trigger = Some(Instant::now());
            println!("SLAP CONFIRMED force={force:.4} ratio={ratio:.4}");
            on_slap(force);
        }
    }

    fn frequency_ratio(&mut self, frame: &[f32]) -> f32 {
        let mut spectrum: Vec<Complex32> = frame
            .iter()
            .copied()
            .map(|sample| Complex32::new(sample, 0.0))
            .collect();

        let fft = self.fft_planner.plan_fft_forward(frame.len());
        fft.process(&mut spectrum);

        let bin_hz = self.sample_rate as f32 / frame.len() as f32;
        let mut low_energy = 0.0;
        let mut high_energy = 0.0;

        for (index, value) in spectrum.iter().take(frame.len() / 2).enumerate() {
            let frequency = index as f32 * bin_hz;
            let energy = value.norm_sqr();

            if frequency <= 300.0 {
                low_energy += energy;
            } else if frequency >= 800.0 {
                high_energy += energy;
            }
        }

        low_energy / high_energy.max(1e-6)
    }
}

fn compute_rms(frame: &[f32]) -> f32 {
    let energy = frame.iter().map(|sample| sample * sample).sum::<f32>() / frame.len() as f32;
    energy.sqrt()
}

fn compute_peak(frame: &[f32]) -> f32 {
    frame
        .iter()
        .copied()
        .map(f32::abs)
        .fold(0.0, f32::max)
}

fn high_pass_frame(frame: &[f32]) -> Vec<f32> {
    if frame.is_empty() {
        return Vec::new();
    }

    let mean = frame.iter().copied().sum::<f32>() / frame.len() as f32;
    frame.iter().map(|sample| sample - mean).collect()
}
