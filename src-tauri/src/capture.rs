//! CPAL stream lives on one native thread. The callback never waits on HTTP/disk.
use cpal::traits::{DeviceTrait, StreamTrait};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use tokio::sync::mpsc::{channel, Receiver, Sender};

pub struct Capture {
    pub rate: u32,
    pub frames: Receiver<Vec<f32>>,
    running: Arc<AtomicBool>,
    fault: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
#[derive(Clone)]
pub struct StopHandle(Arc<AtomicBool>);
impl StopHandle {
    pub fn stop(&self) {
        self.0.store(false, Ordering::Release);
    }
}

pub fn mono<T: cpal::SizedSample + cpal::Sample>(data: &[T], channels: usize) -> Vec<f32>
where
    f32: cpal::FromSample<T>,
{
    data.chunks_exact(channels)
        .map(|frame| frame.iter().map(|v| v.to_sample::<f32>()).sum::<f32>() / channels as f32)
        .collect()
}

fn stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sender: Sender<Vec<f32>>,
    running: Arc<AtomicBool>,
    fault: Arc<AtomicBool>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let channels = config.channels as usize;
    let error_running = running.clone();
    let error_fault = fault.clone();
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                if running.load(Ordering::Acquire) && sender.try_send(mono(data, channels)).is_err()
                {
                    // Fail closed on overflow; never produce an apparently complete transcript.
                    fault.store(true, Ordering::Release);
                    running.store(false, Ordering::Release);
                }
            },
            move |_| {
                error_fault.store(true, Ordering::Release);
                error_running.store(false, Ordering::Release);
            },
            None,
        )
        .map_err(|_| "Não foi possível abrir o microfone no formato selecionado.".into())
}
impl Capture {
    pub fn start(device_name: Option<String>) -> Result<Self, String> {
        #[cfg(debug_assertions)]
        if let Some(path) = std::env::var_os("DICTA_TEST_AUDIO") {
            return Self::fixture(path.into());
        }
        let running = Arc::new(AtomicBool::new(true));
        let fault = Arc::new(AtomicBool::new(false));
        let thread_running = running.clone();
        let thread_fault = fault.clone();
        let (sender, frames) = channel(256);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            let setup = (|| {
                let device = crate::audio::get_input_device_by_name(device_name.as_deref())?;
                let supported = device
                    .default_input_config()
                    .map_err(|_| "Microfone indisponível.".to_string())?;
                let rate = supported.sample_rate().0;
                let format = supported.sample_format();
                let config = supported.into();
                let stream = match format {
                    cpal::SampleFormat::F32 => stream::<f32>(
                        &device,
                        &config,
                        sender,
                        thread_running.clone(),
                        thread_fault.clone(),
                    ),
                    cpal::SampleFormat::I16 => stream::<i16>(
                        &device,
                        &config,
                        sender,
                        thread_running.clone(),
                        thread_fault.clone(),
                    ),
                    cpal::SampleFormat::U16 => stream::<u16>(
                        &device,
                        &config,
                        sender,
                        thread_running.clone(),
                        thread_fault.clone(),
                    ),
                    _ => Err("Formato de microfone não suportado.".into()),
                }?;
                stream
                    .play()
                    .map_err(|_| "Não foi possível iniciar o microfone.".to_string())?;
                Ok::<_, String>((stream, rate))
            })();
            match setup {
                Ok((stream, rate)) => {
                    if ready_tx.send(Ok(rate)).is_err() {
                        return;
                    }
                    while thread_running.load(Ordering::Acquire) {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    drop(stream);
                }
                Err(error) => {
                    thread_running.store(false, Ordering::Release);
                    let _ = ready_tx.send(Err(error));
                }
            }
        });
        match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
            Ok(Ok(rate)) => Ok(Self {
                rate,
                frames,
                running,
                fault,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => {
                running.store(false, Ordering::Release);
                Err("Tempo esgotado ao iniciar o microfone.".into())
            }
        }
    }
    pub fn stop_handle(&self) -> StopHandle {
        StopHandle(self.running.clone())
    }
    pub fn failed(&self) -> bool {
        self.fault.load(Ordering::Acquire)
    }
    #[cfg(debug_assertions)]
    fn fixture(path: std::path::PathBuf) -> Result<Self, String> {
        let reader =
            hound::WavReader::open(path).map_err(|_| "Test fixture missing".to_string())?;
        let spec = reader.spec();
        if spec.channels != 1 || spec.bits_per_sample != 16 {
            return Err("Test fixture must be mono PCM16".into());
        }
        let samples = reader
            .into_samples::<i16>()
            .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "Invalid test fixture".to_string())?;
        let (sender, frames) = channel(256);
        let running = Arc::new(AtomicBool::new(true));
        let flag = running.clone();
        let fault = Arc::new(AtomicBool::new(false));
        let fixture_fault = fault.clone();
        let thread = std::thread::spawn(move || {
            for frame in samples.chunks((spec.sample_rate / 50) as usize) {
                if !flag.load(Ordering::Acquire) {
                    break;
                }
                if sender.try_send(frame.to_vec()).is_err() {
                    fixture_fault.store(true, Ordering::Release);
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        });
        Ok(Self {
            rate: spec.sample_rate,
            frames,
            running,
            fault,
            thread: Some(thread),
        })
    }
}
impl Drop for Capture {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_input_formats_convert_channels() {
        assert_eq!(mono(&[1.0f32, -1.0, 0.5, 0.5], 2), vec![0.0, 0.5]);
        assert!(mono(&[i16::MAX], 1)[0] > 0.99);
        assert!(mono(&[32768u16], 1)[0].abs() < 0.001);
    }
}
