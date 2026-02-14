use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Get audio input device by name, or default if not found
pub fn get_input_device_by_name(device_name: Option<&str>) -> Result<cpal::Device, String> {
    println!("🔍 DEBUG get_input_device_by_name: device_name = {:?}", device_name);
    println!("🔍 DEBUG device_name length = {:?}", device_name.map(|s| s.len()));

    let host = cpal::default_host();

    if let Some(name) = device_name {
        let name_trimmed = name.trim();
        println!("🔍 DEBUG: Searching for device '{}' (trimmed: '{}')", name, name_trimmed);
        println!("🔍 DEBUG: Name bytes: {:?}", name_trimmed.as_bytes());

        // Try to find device by name
        let devices = host
            .input_devices()
            .map_err(|e| format!("Failed to get input devices: {}", e))?;

        println!("🔍 DEBUG: Listing all available devices:");
        let mut found = false;
        for device in devices {
            if let Ok(device_name_str) = device.name() {
                let device_name_trimmed = device_name_str.trim();
                println!("🔍   - Available: '{}' (trimmed: '{}')", device_name_str, device_name_trimmed);
                println!("🔍     Bytes: {:?}", device_name_trimmed.as_bytes());

                // Try exact match first, then trimmed match
                if device_name_str == name || device_name_trimmed == name_trimmed {
                    println!("✅ Found matching device: {} (match type: {})",
                             name,
                             if device_name_str == name { "exact" } else { "trimmed" });
                    found = true;
                    return Ok(device);
                }
            }
        }

        if !found {
            println!("⚠️ Selected device '{}' not found in list, using default", name);
        }
    } else {
        println!("🔍 DEBUG: No device name provided, using default");
    }

    // Fallback to default
    let default = host
        .default_input_device()
        .ok_or("No input device available")?;

    if let Ok(name) = default.name() {
        println!("🎤 Using default device: {}", name);
    }

    Ok(default)
}

pub struct AudioRecorder {
    recording: Arc<Mutex<bool>>,
    audio_data: Arc<Mutex<Vec<f32>>>,
}

pub struct StreamingAudioRecorder {
    recording: Arc<Mutex<bool>>,
    chunk_sender: Option<mpsc::UnboundedSender<Vec<i16>>>,
    stream: Option<cpal::Stream>, // Keep stream alive, drop when done
}

impl AudioRecorder {
    pub fn new() -> Self {
        Self {
            recording: Arc::new(Mutex::new(false)),
            audio_data: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn start_recording(&self, device_name: Option<String>) -> Result<(), String> {
        let recording = self.recording.clone();
        let audio_data = self.audio_data.clone();

        *recording.lock().unwrap() = true;
        audio_data.lock().unwrap().clear();

        // Create stream in a separate thread (stream is not Send, so must stay in one thread)
        std::thread::spawn(move || {
            // Create the audio stream in this thread
            let host = match get_input_device_by_name(device_name.as_deref()) {
                Ok(device) => device,
                Err(e) => {
                    eprintln!("❌ Failed to get input device: {}", e);
                    *recording.lock().unwrap() = false;
                    return;
                }
            };

            let config = match host.default_input_config() {
                Ok(cfg) => cfg,
                Err(e) => {
                    eprintln!("❌ Failed to get default input config: {}", e);
                    *recording.lock().unwrap() = false;
                    return;
                }
            };

            println!("🎤 Using input device: {}", host.name().unwrap_or_default());
            println!("📊 Sample rate: {}", config.sample_rate().0);
            println!("📊 Sample format: {:?}", config.sample_format());
            println!("📊 Channels: {}", config.channels());

            let recording_for_callback = recording.clone();
            let channels = config.channels() as usize;

            let stream = match host.build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if *recording_for_callback.lock().unwrap() {
                        let mut audio = audio_data.lock().unwrap();

                        // Convert stereo/multi-channel to mono by averaging channels
                        if channels == 1 {
                            audio.extend_from_slice(data);
                        } else {
                            for frame in data.chunks_exact(channels) {
                                let sum: f32 = frame.iter().sum();
                                audio.push(sum / channels as f32);
                            }
                        }
                    }
                },
                |err| eprintln!("Stream error: {}", err),
                None,
            ) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("❌ Failed to build input stream: {}", e);
                    *recording.lock().unwrap() = false;
                    return;
                }
            };

            if let Err(e) = stream.play() {
                eprintln!("❌ Failed to play stream: {}", e);
                *recording.lock().unwrap() = false;
                return;
            }

            println!("🎤 Whisper: Audio stream thread started");

            // Keep stream alive while recording
            while *recording.lock().unwrap() {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }

            // Drop stream to release microphone
            drop(stream);
            println!("🎤 Whisper: Microphone released");
        });

        Ok(())
    }

    pub fn stop_recording(&self) -> Vec<f32> {
        *self.recording.lock().unwrap() = false;

        // Wait a bit for the stream thread to clean up
        std::thread::sleep(std::time::Duration::from_millis(200));

        let data = self.audio_data.lock().unwrap().clone();
        println!("🛑 Recording stopped. Captured {} samples", data.len());

        // Check audio levels
        if !data.is_empty() {
            let max_amplitude = data.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            let avg_amplitude = data.iter().map(|s| s.abs()).sum::<f32>() / data.len() as f32;
            println!("📊 Max amplitude: {:.4}, Avg amplitude: {:.4}", max_amplitude, avg_amplitude);

            if max_amplitude < 0.001 {
                println!("⚠️ WARNING: Audio levels are very low! Check microphone.");
            }
        }

        data
    }

    fn build_stream_f32(
        &self,
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        recording: Arc<Mutex<bool>>,
        audio_data: Arc<Mutex<Vec<f32>>>,
    ) -> Result<cpal::Stream, String> {
        let channels = config.channels as usize;

        let stream = device
            .build_input_stream(
                config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if *recording.lock().unwrap() {
                        let mut audio = audio_data.lock().unwrap();

                        // Convert stereo/multi-channel to mono by averaging channels
                        if channels == 1 {
                            audio.extend_from_slice(data);
                        } else {
                            for frame in data.chunks_exact(channels) {
                                let sum: f32 = frame.iter().sum();
                                audio.push(sum / channels as f32);
                            }
                        }
                    }
                },
                |err| eprintln!("Stream error: {}", err),
                None,
            )
            .map_err(|e| format!("Failed to build input stream: {}", e))?;

        Ok(stream)
    }

    fn build_stream_i16(
        &self,
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        recording: Arc<Mutex<bool>>,
        audio_data: Arc<Mutex<Vec<f32>>>,
    ) -> Result<cpal::Stream, String> {
        let stream = device
            .build_input_stream(
                config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if *recording.lock().unwrap() {
                        let mut audio = audio_data.lock().unwrap();
                        for &sample in data.iter() {
                            audio.push(sample as f32 / i16::MAX as f32);
                        }
                    }
                },
                |err| eprintln!("Stream error: {}", err),
                None,
            )
            .map_err(|e| format!("Failed to build input stream: {}", e))?;

        Ok(stream)
    }

    fn build_stream_u16(
        &self,
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        recording: Arc<Mutex<bool>>,
        audio_data: Arc<Mutex<Vec<f32>>>,
    ) -> Result<cpal::Stream, String> {
        let stream = device
            .build_input_stream(
                config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    if *recording.lock().unwrap() {
                        let mut audio = audio_data.lock().unwrap();
                        for &sample in data.iter() {
                            audio.push((sample as f32 - 32768.0) / 32768.0);
                        }
                    }
                },
                |err| eprintln!("Stream error: {}", err),
                None,
            )
            .map_err(|e| format!("Failed to build input stream: {}", e))?;

        Ok(stream)
    }
}

// Streaming Audio Recorder for Realtime API
impl StreamingAudioRecorder {
    pub fn new() -> Self {
        Self {
            recording: Arc::new(Mutex::new(false)),
            chunk_sender: None,
            stream: None,
        }
    }

    /// Start recording and return a channel to receive audio chunks
    pub fn start_streaming(&mut self, device_name: Option<String>) -> Result<mpsc::UnboundedReceiver<Vec<i16>>, String> {

        let device = get_input_device_by_name(device_name.as_deref())?;

        // Use device's native sample rate (usually 48kHz)
        let config: cpal::StreamConfig = device
            .default_input_config()
            .map_err(|e| format!("Failed to get default input config: {}", e))?
            .into();

        let native_rate = config.sample_rate.0;
        println!("🎤 Using input device: {}", device.name().unwrap_or_default());
        println!("📊 Native sample rate: {} Hz", native_rate);
        println!("📊 Target sample rate: 24000 Hz (for Realtime API)");
        println!("📊 Channels: {}", config.channels);

        let (tx, rx) = mpsc::unbounded_channel();
        self.chunk_sender = Some(tx.clone());

        let recording = self.recording.clone();
        *recording.lock().unwrap() = true;

        let channels = config.channels as usize;

        // Build stream for i16 samples (PCM 16-bit)
        let stream = device
            .build_input_stream(
                &config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if *recording.lock().unwrap() {
                        // Convert stereo/multi-channel to mono
                        let mono_data: Vec<i16> = if channels == 1 {
                            data.to_vec()
                        } else {
                            data.chunks_exact(channels)
                                .map(|frame| {
                                    let sum: i32 = frame.iter().map(|&s| s as i32).sum();
                                    (sum / channels as i32) as i16
                                })
                                .collect()
                        };

                        // Resample to 24kHz if needed
                        let resampled: Vec<i16> = if native_rate == 24000 {
                            // No resampling needed
                            mono_data
                        } else if native_rate > 24000 && native_rate % 24000 == 0 {
                            // Downsample by decimation (e.g., 48kHz -> 24kHz)
                            let step = (native_rate / 24000) as usize;
                            mono_data.iter().step_by(step).copied().collect()
                        } else if native_rate == 16000 {
                            // Special case: 16kHz -> 24kHz (ratio 2:3)
                            // Upsample by 3, then downsample by 2
                            // Or simpler: linear interpolation
                            let mut result = Vec::with_capacity((mono_data.len() * 3) / 2);
                            for i in 0..mono_data.len() - 1 {
                                let curr = mono_data[i];
                                let next = mono_data[i + 1];
                                // Output 3 samples for every 2 input samples
                                result.push(curr);
                                result.push(((curr as i32 * 2 + next as i32) / 3) as i16); // interpolate
                                if i % 2 == 1 {
                                    result.push(next);
                                }
                            }
                            result
                        } else {
                            // Other rates - linear interpolation
                            let ratio = 24000.0 / native_rate as f32;
                            let output_len = (mono_data.len() as f32 * ratio) as usize;
                            let mut result = Vec::with_capacity(output_len);

                            for i in 0..output_len {
                                let src_pos = i as f32 / ratio;
                                let src_idx = src_pos as usize;

                                if src_idx + 1 < mono_data.len() {
                                    let frac = src_pos - src_idx as f32;
                                    let sample = mono_data[src_idx] as f32 * (1.0 - frac) +
                                                 mono_data[src_idx + 1] as f32 * frac;
                                    result.push(sample as i16);
                                } else if src_idx < mono_data.len() {
                                    result.push(mono_data[src_idx]);
                                }
                            }
                            result
                        };

                        // Send chunk through channel
                        if !resampled.is_empty() {
                            let _ = tx.send(resampled);
                        }
                    }
                },
                |err| eprintln!("Stream error: {}", err),
                None,
            )
            .map_err(|e| format!("Failed to build input stream: {}", e))?;

        stream.play().map_err(|e| format!("Failed to play stream: {}", e))?;

        // Store stream to keep it alive and allow proper cleanup
        self.stream = Some(stream);

        println!("✅ Streaming recording started ({}Hz -> 24kHz)", native_rate);
        Ok(rx)
    }

    pub fn stop_streaming(&mut self) {
        *self.recording.lock().unwrap() = false;

        // Drop the stream to release the microphone
        if let Some(stream) = self.stream.take() {
            drop(stream);
            println!("🎤 Microphone released");
        }

        println!("🛑 Streaming recording stopped");
    }
}

/// Convert i16 PCM samples to bytes (little-endian)
pub fn pcm_to_bytes(samples: &[i16]) -> Vec<u8> {
    samples
        .iter()
        .flat_map(|&sample| sample.to_le_bytes())
        .collect()
}
