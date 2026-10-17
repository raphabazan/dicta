//! Duration is a target. Close on a speech pause, with a bounded fallback.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SegmentConfig {
    pub enabled: bool,
    pub target_seconds: u32,
    pub silence_ms: u32,
}
impl Default for SegmentConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            target_seconds: 20,
            silence_ms: 500,
        }
    }
}
impl SegmentConfig {
    pub fn validate(&self) -> Result<(), String> {
        if ![10, 20, 30].contains(&self.target_seconds) || !(300..=1500).contains(&self.silence_ms)
        {
            return Err("Use um alvo de 10, 20 ou 30 s e pausa entre 300 e 1500 ms.".into());
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct Segment {
    pub samples: Vec<f32>,
    pub start_sample: u64,
    pub overlap_samples: usize,
    pub reason: &'static str,
}

pub struct Segmenter {
    rate: u32,
    config: SegmentConfig,
    buffer: Vec<f32>,
    total: u64,
    start: u64,
    frame: Vec<f32>,
    silence: usize,
    speech: bool,
    noise: f32,
    overlap: usize,
}
impl Segmenter {
    pub fn new(rate: u32, config: SegmentConfig) -> Self {
        Self {
            rate,
            config,
            buffer: Vec::new(),
            total: 0,
            start: 0,
            frame: Vec::new(),
            silence: 0,
            speech: false,
            noise: 0.0001,
            overlap: 0,
        }
    }
    pub fn push(&mut self, samples: &[f32]) -> Vec<Segment> {
        let mut ready = Vec::new();
        let frame_size = (self.rate as usize / 50).max(1);
        for &sample in samples {
            self.buffer.push(sample);
            self.frame.push(sample);
            self.total += 1;
            if self.frame.len() < frame_size {
                continue;
            }
            let rms =
                (self.frame.iter().map(|v| v * v).sum::<f32>() / self.frame.len() as f32).sqrt();
            // Hysteresis/noise tracking is intentionally local and configurable by
            // experiment; it does not claim semantic or exact word boundaries.
            let threshold = (self.noise * 3.0).clamp(0.001, 0.015);
            if rms > threshold {
                self.speech = true;
                self.silence = 0;
            } else {
                self.silence += self.frame.len();
                self.noise = self.noise * 0.98 + rms * 0.02;
            }
            self.frame.clear();
            let target = self.rate as usize * self.config.target_seconds as usize;
            let maximum = self.rate as usize * (self.config.target_seconds + 10) as usize;
            let paused =
                self.silence >= self.rate as usize * self.config.silence_ms as usize / 1000;
            if self.config.enabled
                && self.speech
                && self.buffer.len() >= target
                && (paused || self.buffer.len() >= maximum)
            {
                ready.push(self.take(if paused { "pause" } else { "limit" }));
            } else if self.config.enabled && !self.speech && self.buffer.len() >= maximum {
                // Keep pre-roll, but do not upload silence every target interval.
                let keep = self.rate as usize * 300 / 1000;
                self.buffer.drain(..self.buffer.len().saturating_sub(keep));
                self.start = self.total - self.buffer.len() as u64;
                self.overlap = 0;
            }
        }
        ready
    }
    fn take(&mut self, reason: &'static str) -> Segment {
        let samples = std::mem::take(&mut self.buffer);
        let output = Segment {
            start_sample: self.start,
            overlap_samples: self.overlap,
            samples,
            reason,
        };
        // Pause cuts do not need overlap. Forced cuts carry a small known region.
        let keep = if reason == "limit" {
            (self.rate as usize / 2).min(output.samples.len())
        } else {
            0
        };
        self.buffer
            .extend_from_slice(&output.samples[output.samples.len() - keep..]);
        self.start = self.total - keep as u64;
        self.overlap = keep;
        self.speech = false;
        self.silence = 0;
        output
    }
    pub fn finish(&mut self) -> Option<Segment> {
        let new_samples = self.buffer.len().saturating_sub(self.overlap);
        // The final tail is never held waiting for VAD. Quiet short speech is kept.
        if new_samples == 0 || !self.buffer.iter().any(|v| v.abs() > 0.00001) {
            return None;
        }
        Some(self.take("stop"))
    }
}

pub fn merge_text(parts: &[(String, bool)]) -> String {
    let mut output = String::new();
    for (text, overlaps) in parts {
        let mut words: Vec<&str> = text.split_whitespace().collect();
        if *overlaps {
            let previous: Vec<&str> = output.split_whitespace().collect();
            // Only examine the known overlap, never deduplicate the whole transcript.
            for count in (1..=6.min(previous.len()).min(words.len())).rev() {
                let normalize = |s: &str| {
                    s.trim_matches(|c: char| !c.is_alphanumeric())
                        .to_lowercase()
                };
                if previous[previous.len() - count..]
                    .iter()
                    .map(|w| normalize(w))
                    .eq(words[..count].iter().map(|w| normalize(w)))
                {
                    words.drain(..count);
                    break;
                }
            }
        }
        if !output.is_empty() && !words.is_empty() {
            output.push(' ');
        }
        output.push_str(&words.join(" "));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> SegmentConfig {
        SegmentConfig {
            enabled: true,
            target_seconds: 20,
            silence_ms: 500,
        }
    }
    #[test]
    fn waits_for_pause_after_target() {
        let mut s = Segmenter::new(1000, config());
        assert!(s.push(&vec![0.1; 21_000]).is_empty());
        let chunks = s.push(&vec![0.0; 500]);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].reason, "pause");
        assert_eq!(chunks[0].samples.len(), 21_500);
    }
    #[test]
    fn continuous_speech_is_bounded_and_covered() {
        let mut s = Segmenter::new(1000, config());
        let chunks = s.push(&vec![0.1; 31_000]);
        assert_eq!(chunks[0].samples.len(), 30_000);
        let tail = s.finish().unwrap();
        assert_eq!(tail.start_sample, 29_500);
        assert_eq!(tail.samples.len(), 1500);
        assert_eq!(tail.overlap_samples, 500);
    }
    #[test]
    fn silence_does_not_upload() {
        let mut s = Segmenter::new(1000, config());
        assert!(s.push(&vec![0.0; 120_000]).is_empty());
        assert!(s.finish().is_none());
        assert!(s.buffer.len() < 30_000);
    }
    #[test]
    fn short_gaps_and_stop_preserve_tail() {
        let mut s = Segmenter::new(1000, config());
        s.push(&vec![0.1; 20_000]);
        assert!(s.push(&vec![0.0; 200]).is_empty());
        s.push(&vec![0.1; 20]);
        assert_eq!(s.finish().unwrap().samples.len(), 20_220);
    }
    #[test]
    fn only_known_overlap_is_reconciled() {
        assert_eq!(
            merge_text(&[("não, não".into(), false), ("não, não".into(), false)]),
            "não, não não, não"
        );
        assert_eq!(
            merge_text(&[("olá mundo".into(), false), ("mundo continua".into(), true)]),
            "olá mundo continua"
        );
    }
    #[test]
    fn whole_file_mode_does_not_send_early() {
        let mut s = Segmenter::new(
            1000,
            SegmentConfig {
                enabled: false,
                ..SegmentConfig::default()
            },
        );
        assert!(s.push(&vec![0.1; 60_000]).is_empty());
        assert_eq!(s.finish().unwrap().samples.len(), 60_000);
    }
}
