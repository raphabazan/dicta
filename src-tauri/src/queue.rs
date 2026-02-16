use std::path::PathBuf;
use std::net::TcpStream;
use std::time::Duration;

pub const MAX_QUEUE_SIZE: i64 = 3;

/// Save raw PCM f32 audio to a WAV file in the queue directory
pub fn save_audio_to_wav(audio: Vec<f32>, dir: &PathBuf) -> Result<PathBuf, String> {
    let filename = format!(
        "queue_{}.wav",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    let path = dir.join(&filename);

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(&path, spec)
        .map_err(|e| format!("WAV create error: {}", e))?;
    for sample in &audio {
        writer
            .write_sample(*sample)
            .map_err(|e| format!("WAV write error: {}", e))?;
    }
    writer
        .finalize()
        .map_err(|e| format!("WAV finalize error: {}", e))?;

    println!("💾 Saved queue audio to {}", path.display());
    Ok(path)
}

/// Read a WAV file back into Vec<f32> for retry (handles both i16 and f32 WAV formats)
pub fn read_wav_to_f32(path: &str) -> Result<Vec<f32>, String> {
    let reader =
        hound::WavReader::open(path).map_err(|e| format!("WAV open error: {}", e))?;
    let spec = reader.spec();
    let samples = read_wav_samples_as_f32(reader, &spec)?;
    println!("📂 Read {} samples from queue WAV", samples.len());
    Ok(samples)
}

/// Internal: read WAV samples as f32 regardless of source format
fn read_wav_samples_as_f32(mut reader: hound::WavReader<std::io::BufReader<std::fs::File>>, spec: &hound::WavSpec) -> Result<Vec<f32>, String> {
    match spec.sample_format {
        hound::SampleFormat::Float => {
            reader.samples::<f32>()
                .map(|s| s.map_err(|e| format!("WAV sample error: {}", e)))
                .collect::<Result<Vec<_>, _>>()
        }
        hound::SampleFormat::Int => {
            let max_val = (1 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>()
                .map(|s| s.map(|v| v as f32 / max_val).map_err(|e| format!("WAV sample error: {}", e)))
                .collect::<Result<Vec<_>, _>>()
        }
    }
}

/// Save raw PCM i16 audio to a WAV file (for realtime mode at variable sample rate)
pub fn save_audio_i16_to_wav(audio: &[i16], sample_rate: u32, dir: &PathBuf) -> Result<PathBuf, String> {
    let filename = format!(
        "queue_rt_{}.wav",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    let path = dir.join(&filename);

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec)
        .map_err(|e| format!("WAV create error: {}", e))?;
    for sample in audio {
        writer
            .write_sample(*sample)
            .map_err(|e| format!("WAV write error: {}", e))?;
    }
    writer
        .finalize()
        .map_err(|e| format!("WAV finalize error: {}", e))?;

    println!("💾 Saved realtime queue audio to {} ({} samples at {}Hz)", path.display(), audio.len(), sample_rate);
    Ok(path)
}

/// Read a WAV file back into Vec<f32> + sample rate for retry (handles both i16 and f32 formats)
pub fn read_wav_to_f32_with_rate(path: &str) -> Result<(Vec<f32>, u32), String> {
    let reader =
        hound::WavReader::open(path).map_err(|e| format!("WAV open error: {}", e))?;
    let spec = reader.spec();
    let sample_rate = spec.sample_rate;
    let samples = read_wav_samples_as_f32(reader, &spec)?;
    println!("📂 Read {} samples at {}Hz from queue WAV", samples.len(), sample_rate);
    Ok((samples, sample_rate))
}

/// Quick connectivity check via TCP probe to Google DNS
pub fn is_online() -> bool {
    TcpStream::connect_timeout(
        &"8.8.8.8:53".parse().unwrap(),
        Duration::from_secs(2),
    )
    .is_ok()
}

/// Delete a WAV file from disk (best-effort)
pub fn delete_wav_file(path: &str) {
    if let Err(e) = std::fs::remove_file(path) {
        eprintln!("⚠️ Failed to delete queue WAV {}: {}", path, e);
    } else {
        println!("🗑️ Deleted queue WAV: {}", path);
    }
}
