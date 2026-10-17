use std::path::Path;
pub fn read_wav_to_f32_with_rate(path: &str) -> Result<(Vec<f32>, u32), String> {
    let mut reader =
        hound::WavReader::open(path).map_err(|_| "queue_audio_unavailable".to_string())?;
    let spec = reader.spec();
    if spec.channels != 1 || spec.sample_rate == 0 {
        return Err("queue_audio_format".into());
    }
    let samples = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .map(|v| v.map_err(|_| "queue_audio_sample".to_string()))
            .collect::<Result<Vec<_>, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 2f32.powi(spec.bits_per_sample as i32 - 1);
            reader
                .samples::<i32>()
                .map(|v| {
                    v.map(|n| n as f32 / scale)
                        .map_err(|_| "queue_audio_sample".to_string())
                })
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    Ok((samples, spec.sample_rate))
}
pub fn delete_wav_file(path: &str) {
    if Path::new(path)
        .extension()
        .is_some_and(|s| s.eq_ignore_ascii_case("wav"))
    {
        let _ = std::fs::remove_file(path);
    }
}
