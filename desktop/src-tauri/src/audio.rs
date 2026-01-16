//! Native audio recording module using cpal
//!
//! Provides cross-platform audio capture with consistent quality.
//! Records to WAV format for high-quality audio on all platforms.
//!
//! Uses a dedicated audio thread since cpal::Stream is not Send on some platforms.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Commands sent to the audio thread
enum AudioCommand {
    Start(Option<AppHandle>),
    Stop(Sender<Result<AudioData, String>>),
    Cancel,
    Shutdown,
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

/// Thread-safe audio recorder that communicates with a dedicated audio thread
pub struct AudioRecorder {
    command_tx: Sender<AudioCommand>,
    is_recording: Arc<Mutex<bool>>,
    _thread_handle: JoinHandle<()>,
}

impl AudioRecorder {
    /// Create new audio recorder with dedicated audio thread
    pub fn new() -> Self {
        let (command_tx, command_rx) = mpsc::channel();
        let is_recording = Arc::new(Mutex::new(false));
        let is_recording_clone = Arc::clone(&is_recording);

        let thread_handle = thread::spawn(move || {
            audio_thread_main(command_rx, is_recording_clone);
        });

        Self {
            command_tx,
            is_recording,
            _thread_handle: thread_handle,
        }
    }

    /// List available input devices
    pub fn list_devices(&self) -> Result<Vec<AudioDevice>, String> {
        use cpal::traits::{DeviceTrait, HostTrait};

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

    /// Start recording
    pub fn start(&self, app_handle: Option<AppHandle>) -> Result<(), String> {
        {
            let recording = self.is_recording.lock().unwrap();
            if *recording {
                return Err("Already recording".to_string());
            }
        }

        self.command_tx
            .send(AudioCommand::Start(app_handle))
            .map_err(|e| format!("Failed to send start command: {}", e))?;

        // Update recording state
        *self.is_recording.lock().unwrap() = true;
        Ok(())
    }

    /// Stop recording and return the audio data
    pub fn stop(&self) -> Result<AudioData, String> {
        {
            let recording = self.is_recording.lock().unwrap();
            if !*recording {
                return Err("Not recording".to_string());
            }
        }

        let (result_tx, result_rx) = mpsc::channel();

        self.command_tx
            .send(AudioCommand::Stop(result_tx))
            .map_err(|e| format!("Failed to send stop command: {}", e))?;

        // Update recording state
        *self.is_recording.lock().unwrap() = false;

        // Wait for result from audio thread with timeout (10 seconds max)
        // This prevents infinite hang if audio thread panics or gets stuck
        result_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|e| format!("Audio processing timed out or failed: {}", e))?
    }

    /// Cancel recording without returning data
    pub fn cancel(&self) {
        let _ = self.command_tx.send(AudioCommand::Cancel);
        *self.is_recording.lock().unwrap() = false;
    }

    /// Check if currently recording
    pub fn is_recording(&self) -> bool {
        *self.is_recording.lock().unwrap()
    }
}

impl Drop for AudioRecorder {
    fn drop(&mut self) {
        let _ = self.command_tx.send(AudioCommand::Shutdown);
    }
}

/// Main function for the audio thread
fn audio_thread_main(command_rx: Receiver<AudioCommand>, is_recording: Arc<Mutex<bool>>) {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{SampleFormat, StreamConfig};

    let mut stream: Option<cpal::Stream> = None;
    let mut sample_rate: u32 = 44100;
    let mut app_handle: Option<AppHandle> = None;
    // Shared buffer that persists across commands - accessible by both callback and stop handler
    let samples_buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));

    loop {
        match command_rx.recv() {
            Ok(AudioCommand::Start(handle)) => {
                app_handle = handle;
                // Clear the buffer for new recording
                samples_buffer.lock().unwrap().clear();

                let host = cpal::default_host();

                let device = match host.default_input_device() {
                    Some(d) => d,
                    None => {
                        eprintln!("[Audio] No input device available");
                        *is_recording.lock().unwrap() = false;
                        continue;
                    }
                };

                let config = match device.default_input_config() {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("[Audio] Failed to get config: {}", e);
                        *is_recording.lock().unwrap() = false;
                        continue;
                    }
                };

                sample_rate = config.sample_rate().0;
                let channels = config.channels() as usize;

                println!("[Audio] Starting recording at {} Hz, {} channels", sample_rate, channels);

                // Clone the shared buffer for the audio callback
                let samples_clone = Arc::clone(&samples_buffer);
                let app_handle_clone = app_handle.clone();

                let stream_config: StreamConfig = config.clone().into();

                let err_fn = |err| eprintln!("[Audio] Stream error: {}", err);

                let new_stream = match config.sample_format() {
                    SampleFormat::F32 => device.build_input_stream(
                        &stream_config,
                        move |data: &[f32], _: &cpal::InputCallbackInfo| {
                            process_samples(data, channels, &samples_clone, &app_handle_clone);
                        },
                        err_fn,
                        None,
                    ),
                    SampleFormat::I16 => {
                        let samples_clone = Arc::clone(&samples_buffer);
                        let app_handle_clone = app_handle.clone();
                        device.build_input_stream(
                            &stream_config,
                            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                                let float_data: Vec<f32> =
                                    data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                                process_samples(&float_data, channels, &samples_clone, &app_handle_clone);
                            },
                            err_fn,
                            None,
                        )
                    }
                    SampleFormat::I32 => {
                        let samples_clone = Arc::clone(&samples_buffer);
                        let app_handle_clone = app_handle.clone();
                        device.build_input_stream(
                            &stream_config,
                            move |data: &[i32], _: &cpal::InputCallbackInfo| {
                                let float_data: Vec<f32> =
                                    data.iter().map(|&s| s as f32 / i32::MAX as f32).collect();
                                process_samples(&float_data, channels, &samples_clone, &app_handle_clone);
                            },
                            err_fn,
                            None,
                        )
                    }
                    _ => {
                        eprintln!("[Audio] Unsupported sample format");
                        *is_recording.lock().unwrap() = false;
                        continue;
                    }
                };

                match new_stream {
                    Ok(s) => {
                        if let Err(e) = s.play() {
                            eprintln!("[Audio] Failed to start stream: {}", e);
                            *is_recording.lock().unwrap() = false;
                            continue;
                        }
                        stream = Some(s);
                        println!("[Audio] Recording started");
                    }
                    Err(e) => {
                        eprintln!("[Audio] Failed to build stream: {}", e);
                        *is_recording.lock().unwrap() = false;
                    }
                }
            }

            Ok(AudioCommand::Stop(result_tx)) => {
                // Drop the stream to stop recording
                if let Some(s) = stream.take() {
                    drop(s);
                }

                // Give a small delay for final samples to be processed
                thread::sleep(std::time::Duration::from_millis(50));

                // Get samples from the shared buffer
                let samples = samples_buffer.lock().unwrap().clone();
                let duration_secs = samples.len() as f32 / sample_rate as f32;

                println!("[Audio] Recording stopped: {} samples, {:.2}s", samples.len(), duration_secs);

                // Encode to WAV
                let result = encode_wav(&samples, sample_rate);

                let _ = result_tx.send(result);
                app_handle = None;
            }

            Ok(AudioCommand::Cancel) => {
                if let Some(s) = stream.take() {
                    drop(s);
                }
                samples_buffer.lock().unwrap().clear();
                app_handle = None;
                println!("[Audio] Recording cancelled");
            }

            Ok(AudioCommand::Shutdown) | Err(_) => {
                if let Some(s) = stream.take() {
                    drop(s);
                }
                println!("[Audio] Audio thread shutting down");
                break;
            }
        }
    }
}

/// Process audio samples (called from audio callback)
fn process_samples(
    data: &[f32],
    channels: usize,
    samples: &Arc<Mutex<Vec<f32>>>,
    app_handle: &Option<AppHandle>,
) {
    let mut buffer = samples.lock().unwrap();
    let mut sum = 0.0f32;
    let mut count = 0;

    for frame in data.chunks(channels) {
        // Convert to mono by averaging channels
        let sample: f32 = frame.iter().sum::<f32>() / channels as f32;
        buffer.push(sample);
        sum += sample.abs();
        count += 1;
    }

    // Emit audio level events periodically
    if buffer.len() % 4410 < count {
        let level = if count > 0 {
            (sum / count as f32 * 3.0).min(1.0)
        } else {
            0.0
        };

        if let Some(ref handle) = app_handle {
            let _ = handle.emit("audio_level", AudioLevelEvent { level });
        }
    }
}

/// Encode samples to WAV format
fn encode_wav(samples: &[f32], sample_rate: u32) -> Result<AudioData, String> {
    use std::io::Cursor;

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut buffer = Cursor::new(Vec::new());
    let mut writer = hound::WavWriter::new(&mut buffer, spec)
        .map_err(|e| format!("Failed to create WAV writer: {}", e))?;

    for &sample in samples {
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
    let duration_secs = samples.len() as f32 / sample_rate as f32;

    use base64::Engine;
    let audio_base64 = base64::engine::general_purpose::STANDARD.encode(&wav_data);

    Ok(AudioData {
        audio_base64,
        mime_type: "audio/wav".to_string(),
        duration_secs,
        sample_rate,
    })
}
