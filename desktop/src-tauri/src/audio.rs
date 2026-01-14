//! Native audio recording module using cpal
//!
//! Provides cross-platform audio capture with consistent quality.
//! Records to WAV format for high-quality audio on all platforms.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

/// Audio recorder state
pub struct AudioRecorder {
    device: Option<Device>,
    stream: Option<Stream>,
    is_recording: Arc<AtomicBool>,
    samples: Arc<Mutex<Vec<f32>>>,
    sample_rate: u32,
    app_handle: Option<AppHandle>,
}

/// Recorded audio data returned to frontend
#[derive(serde::Serialize, Clone)]
pub struct AudioData {
    /// Base64 encoded audio (WAV format)
    pub audio_base64: String,
    /// MIME type (audio/wav)
    pub mime_type: String,
    /// Duration in seconds
    pub duration_secs: f32,
    /// Sample rate used
    pub sample_rate: u32,
}

/// Audio device info
#[derive(serde::Serialize, Clone)]
pub struct AudioDevice {
    pub name: String,
    pub is_default: bool,
}

/// Audio level event sent to frontend
#[derive(Clone, serde::Serialize)]
pub struct AudioLevelEvent {
    pub level: f32, // 0.0 to 1.0
}

impl Default for AudioRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioRecorder {
    /// Create new audio recorder
    pub fn new() -> Self {
        Self {
            device: None,
            stream: None,
            is_recording: Arc::new(AtomicBool::new(false)),
            samples: Arc::new(Mutex::new(Vec::new())),
            sample_rate: 44100,
            app_handle: None,
        }
    }

    /// Set the Tauri app handle for emitting events
    pub fn set_app_handle(&mut self, handle: AppHandle) {
        self.app_handle = Some(handle);
    }

    /// List available input devices
    pub fn list_devices(&self) -> Result<Vec<AudioDevice>, String> {
        let host = cpal::default_host();
        let default_device = host.default_input_device();
        let default_name = default_device.as_ref().and_then(|d| d.name().ok());

        let devices: Vec<AudioDevice> = host
            .input_devices()
            .map_err(|e| format!("Failed to enumerate devices: {}", e))?
            .filter_map(|device| {
                device.name().ok().map(|name| AudioDevice {
                    is_default: Some(&name) == default_name.as_ref(),
                    name,
                })
            })
            .collect();

        Ok(devices)
    }

    /// Start recording from the default input device
    pub fn start(&mut self) -> Result<(), String> {
        if self.is_recording.load(Ordering::SeqCst) {
            return Err("Already recording".to_string());
        }

        let host = cpal::default_host();

        // Get default input device
        let device = host
            .default_input_device()
            .ok_or("No input device available")?;

        println!("[Audio] Using device: {:?}", device.name());

        // Get supported config (prefer 44100Hz)
        let config = device
            .default_input_config()
            .map_err(|e| format!("Failed to get config: {}", e))?;

        println!("[Audio] Using config: {:?}", config);

        self.sample_rate = config.sample_rate().0;
        let channels = config.channels();

        // Clear previous samples
        self.samples.lock().clear();
        self.is_recording.store(true, Ordering::SeqCst);

        // Clone references for the audio callback
        let samples = Arc::clone(&self.samples);
        let is_recording = Arc::clone(&self.is_recording);
        let app_handle = self.app_handle.clone();

        let err_fn = |err| {
            eprintln!("[Audio] Stream error: {}", err);
        };

        // Build input stream based on sample format
        let stream_config: StreamConfig = config.clone().into();

        let stream = match config.sample_format() {
            SampleFormat::F32 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        process_audio_data(
                            data,
                            channels as usize,
                            &samples,
                            &is_recording,
                            &app_handle,
                        );
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("Failed to build f32 stream: {}", e))?,

            SampleFormat::I16 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        let float_data: Vec<f32> =
                            data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                        process_audio_data(
                            &float_data,
                            channels as usize,
                            &samples,
                            &is_recording,
                            &app_handle,
                        );
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("Failed to build i16 stream: {}", e))?,

            SampleFormat::I32 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[i32], _: &cpal::InputCallbackInfo| {
                        let float_data: Vec<f32> =
                            data.iter().map(|&s| s as f32 / i32::MAX as f32).collect();
                        process_audio_data(
                            &float_data,
                            channels as usize,
                            &samples,
                            &is_recording,
                            &app_handle,
                        );
                    },
                    err_fn,
                    None,
                )
                .map_err(|e| format!("Failed to build i32 stream: {}", e))?,

            format => return Err(format!("Unsupported sample format: {:?}", format)),
        };

        stream
            .play()
            .map_err(|e| format!("Failed to start stream: {}", e))?;

        self.stream = Some(stream);
        self.device = Some(device);

        println!("[Audio] Recording started at {} Hz", self.sample_rate);

        Ok(())
    }

    /// Stop recording and return the audio data
    pub fn stop(&mut self) -> Result<AudioData, String> {
        if !self.is_recording.load(Ordering::SeqCst) {
            return Err("Not recording".to_string());
        }

        self.is_recording.store(false, Ordering::SeqCst);

        // Stop and drop the stream
        if let Some(stream) = self.stream.take() {
            drop(stream);
        }
        self.device = None;

        // Get the recorded samples
        let samples = self.samples.lock().clone();
        let duration_secs = samples.len() as f32 / self.sample_rate as f32;

        println!(
            "[Audio] Recording stopped: {} samples, {:.2}s",
            samples.len(),
            duration_secs
        );

        // Encode to WAV
        let wav_data = self.encode_wav(&samples)?;

        use base64::Engine;
        let audio_base64 = base64::engine::general_purpose::STANDARD.encode(&wav_data);

        Ok(AudioData {
            audio_base64,
            mime_type: "audio/wav".to_string(),
            duration_secs,
            sample_rate: self.sample_rate,
        })
    }

    /// Cancel recording without returning data
    pub fn cancel(&mut self) {
        self.is_recording.store(false, Ordering::SeqCst);
        if let Some(stream) = self.stream.take() {
            drop(stream);
        }
        self.device = None;
        self.samples.lock().clear();
        println!("[Audio] Recording cancelled");
    }

    /// Encode samples to WAV format
    fn encode_wav(&self, samples: &[f32]) -> Result<Vec<u8>, String> {
        use std::io::Cursor;

        let spec = hound::WavSpec {
            channels: 1, // We convert to mono
            sample_rate: self.sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let mut buffer = Cursor::new(Vec::new());
        let mut writer = hound::WavWriter::new(&mut buffer, spec)
            .map_err(|e| format!("Failed to create WAV writer: {}", e))?;

        for &sample in samples {
            // Convert f32 [-1.0, 1.0] to i16, with clamping
            let clamped = sample.clamp(-1.0, 1.0);
            let sample_i16 = (clamped * i16::MAX as f32) as i16;
            writer
                .write_sample(sample_i16)
                .map_err(|e| format!("Failed to write sample: {}", e))?;
        }

        writer
            .finalize()
            .map_err(|e| format!("Failed to finalize WAV: {}", e))?;

        let wav_data = buffer.into_inner();
        println!("[Audio] Encoded WAV: {} bytes", wav_data.len());

        Ok(wav_data)
    }

    /// Check if currently recording
    pub fn is_recording(&self) -> bool {
        self.is_recording.load(Ordering::SeqCst)
    }
}

/// Process audio data from the input stream
fn process_audio_data(
    data: &[f32],
    channels: usize,
    samples: &Arc<Mutex<Vec<f32>>>,
    is_recording: &Arc<AtomicBool>,
    app_handle: &Option<AppHandle>,
) {
    if !is_recording.load(Ordering::SeqCst) {
        return;
    }

    let mut buffer = samples.lock();
    let mut sum = 0.0f32;
    let mut count = 0;

    for frame in data.chunks(channels) {
        // Convert to mono by averaging channels
        let sample: f32 = frame.iter().sum::<f32>() / channels as f32;
        buffer.push(sample);
        sum += sample.abs();
        count += 1;
    }

    // Emit audio level events (throttled by checking buffer size)
    // Only emit every ~100ms worth of samples
    if buffer.len() % 4410 < count {
        let level = if count > 0 {
            (sum / count as f32 * 3.0).min(1.0) // Scale up for visibility
        } else {
            0.0
        };

        if let Some(ref handle) = app_handle {
            let _ = handle.emit("audio_level", AudioLevelEvent { level });
        }
    }
}
