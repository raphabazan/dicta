//! Recording sessions own their audio and result. UI events are snapshots, not commands.
use crate::{
    capture::{Capture, StopHandle},
    db::{Database, SessionRecord, StoredSegment},
    delivery::{self, Target},
    diagnostics,
    openai::OpenAIClient,
    segmentation::{merge_text, Segment, SegmentConfig, Segmenter},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, AtomicU8, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager as _};
use tokio::{sync::Semaphore, task::JoinSet};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub segments: SegmentConfig,
    pub asr_model: String,
    pub prompt_model: Option<String>,
    pub languages: Vec<String>,
    pub hint: Option<String>,
    pub web_search: bool,
    pub rate: u32,
    #[serde(default)]
    pub input_text: Option<String>,
    #[serde(default)]
    pub image_data: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub id: String,
    pub generation: u64,
    pub sequence: u64,
    pub phase: String,
    pub total: u32,
    pub completed: u32,
    pub duration_ms: i64,
    pub text: String,
    pub error: Option<String>,
    pub segmented: bool,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            id: String::new(),
            generation: 0,
            sequence: 0,
            phase: "idle".into(),
            total: 0,
            completed: 0,
            duration_ms: 0,
            text: String::new(),
            error: None,
            segmented: false,
        }
    }
}
struct Runtime {
    snapshot: Snapshot,
    stop: Option<StopHandle>,
    interruption: Arc<AtomicU8>,
    running: bool,
}
pub struct Manager {
    inner: Mutex<Runtime>,
}
impl Manager {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Runtime {
                snapshot: Snapshot::default(),
                stop: None,
                interruption: Arc::new(AtomicU8::new(0)),
                running: false,
            }),
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        self.inner.lock().unwrap().snapshot.clone()
    }
    pub fn busy(&self) -> bool {
        self.inner.lock().unwrap().running
    }
    pub fn interruption(&self, id: &str) -> u8 {
        let runtime = self.inner.lock().unwrap();
        if runtime.snapshot.id == id {
            runtime.interruption.load(Ordering::Acquire)
        } else {
            0
        }
    }
    pub fn finished(&self, id: &str) {
        let mut r = self.inner.lock().unwrap();
        if r.snapshot.id == id {
            r.running = false;
            r.stop = None;
        }
    }
    pub fn begin(&self, id: &str, segmented: bool) -> Result<Arc<AtomicU8>, String> {
        let mut runtime = self.inner.lock().unwrap();
        if runtime.running {
            return Err("Já há uma sessão em andamento.".into());
        }
        let generation = runtime.snapshot.generation + 1;
        runtime.snapshot = Snapshot {
            id: id.into(),
            generation,
            phase: "starting".into(),
            segmented,
            ..Snapshot::default()
        };
        runtime.interruption = Arc::new(AtomicU8::new(0));
        runtime.stop = None;
        runtime.running = true;
        Ok(runtime.interruption.clone())
    }
    pub fn attach(&self, id: &str, stop: StopHandle) {
        let mut r = self.inner.lock().unwrap();
        if r.snapshot.id == id {
            r.stop = Some(stop);
        }
    }
    pub fn update(&self, app: &AppHandle, id: &str, change: impl FnOnce(&mut Snapshot)) {
        let snapshot = {
            let mut r = self.inner.lock().unwrap();
            if r.snapshot.id != id {
                return;
            }
            change(&mut r.snapshot);
            r.snapshot.sequence += 1;
            r.snapshot.clone()
        };
        diagnostics::event(
            "session_state",
            json!({"id":snapshot.id,"phase":snapshot.phase,"sequence":snapshot.sequence,"total":snapshot.total,"completed":snapshot.completed,"error":snapshot.error}),
        );
        let _ = app.emit("session-state", &snapshot);
    }
    pub fn stop(&self, app: &AppHandle) -> Result<(), String> {
        let id = {
            let r = self.inner.lock().unwrap();
            if r.snapshot.phase != "recording" {
                return Err("Nenhuma gravação ativa.".into());
            }
            if let Some(stop) = &r.stop {
                stop.stop();
            }
            r.snapshot.id.clone()
        };
        self.update(app, &id, |s| s.phase = "finalizing".into());
        Ok(())
    }
    pub fn interrupt(&self, app: &AppHandle, discard: bool) {
        let id = {
            let r = self.inner.lock().unwrap();
            if !matches!(
                r.snapshot.phase.as_str(),
                "starting" | "recording" | "finalizing" | "transcribing" | "processing_command"
            ) {
                return;
            }
            r.interruption
                .store(if discard { 1 } else { 2 }, Ordering::Release);
            if let Some(stop) = &r.stop {
                stop.stop();
            }
            r.snapshot.id.clone()
        };
        self.update(app, &id, |s| {
            s.phase = if discard { "cancelled" } else { "recoverable" }.into();
            s.error = if discard {
                None
            } else {
                Some("session_interrupted".into())
            };
        });
    }
}

#[derive(Clone)]
pub struct Context {
    pub app: AppHandle,
    pub database: Arc<Database>,
    pub client: Arc<OpenAIClient>,
    pub manager: Arc<Manager>,
    pub last_text: Arc<Mutex<Option<String>>>,
    pub is_recording: Arc<Mutex<bool>>,
    pub root: PathBuf,
}
static NEXT: AtomicU64 = AtomicU64::new(1);
pub fn new_id() -> String {
    format!(
        "{}-{}-{}",
        crate::now_ms(),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

fn error_code(error: &str) -> String {
    if error.starts_with("API error (") {
        return error.chars().take(140).collect();
    }
    if [
        "api_timeout",
        "api_connection",
        "empty_transcription",
        "audio_too_large",
        "capture_failed",
        "session_interrupted",
        "empty_response",
        "incomplete_response",
    ]
    .contains(&error)
    {
        error.into()
    } else {
        "processing_failed".into()
    }
}
pub fn fail(ctx: &Context, id: &str, error: &str, status: &str) {
    let status = if ctx.manager.interruption(id) == 1 {
        "cancelled"
    } else {
        status
    };
    let snapshot = ctx.manager.snapshot();
    let _ = ctx.database.update_session(
        id,
        if status == "cancelled" {
            "recoverable"
        } else {
            status
        },
        snapshot.duration_ms,
        &snapshot.text,
    );
    let _ = ctx.database.release_session(id);
    ctx.manager.update(&ctx.app, id, |s| {
        s.phase = status.into();
        s.error = if status == "cancelled" {
            None
        } else {
            Some(error_code(error))
        };
    });
    let _ = ctx.app.emit("sessions-updated", ());
}

pub fn prepare(
    ctx: &Context,
    config: &mut Config,
    microphone: Option<String>,
    id: &str,
) -> Result<(Capture, Arc<AtomicU8>), String> {
    let interruption = ctx.manager.begin(id, config.segments.enabled)?;
    if !(cfg!(debug_assertions) && std::env::var_os("DICTA_TEST_AUDIO").is_some()) {
        let _ = ctx.app.emit("recording-sound-start", ());
        std::thread::sleep(Duration::from_millis(200));
    }
    let capture = match Capture::start(microphone) {
        Ok(value) => value,
        Err(error) => {
            ctx.manager.update(&ctx.app, id, |s| {
                s.phase = "failed".into();
                s.error = Some("microphone_unavailable".into());
            });
            return Err(error);
        }
    };
    config.rate = capture.rate;
    let result = (|| {
        fs::create_dir_all(ctx.root.join(id)).map_err(|_| "spool_unavailable".to_string())?;
        ctx.database
            .create_session(&SessionRecord {
                id: id.into(),
                status: "recording".into(),
                created_at: crate::now_ms(),
                duration_ms: 0,
                config: serde_json::to_string(config).map_err(|_| "session_config".to_string())?,
                text: String::new(),
                result: String::new(),
            })
            .map_err(|_| "session_storage".to_string())?;
        Ok::<_, String>(())
    })();
    if let Err(error) = result {
        ctx.manager
            .update(&ctx.app, id, |s| s.phase = "failed".into());
        return Err(error);
    }
    ctx.manager.attach(id, capture.stop_handle());
    ctx.manager
        .update(&ctx.app, id, |s| s.phase = "recording".into());
    Ok((capture, interruption))
}

fn seal_wav(file: &Path, rate: u32, samples: &[f32]) -> Result<(), String> {
    let pending = file.with_extension("tmp");
    let mut writer = hound::WavWriter::create(
        &pending,
        hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .map_err(|_| "spool_write".to_string())?;
    for sample in samples {
        writer
            .write_sample((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            .map_err(|_| "spool_write".to_string())?;
    }
    writer.finalize().map_err(|_| "spool_write".to_string())?;
    // FlushFileBuffers requires GENERIC_WRITE on Windows, including after finalize.
    OpenOptions::new()
        .write(true)
        .open(&pending)
        .and_then(|f| f.sync_all())
        .map_err(|_| "spool_write".to_string())?;
    fs::rename(&pending, file).map_err(|_| "spool_write".to_string())?;
    Ok(())
}

fn persist_segment(
    ctx: &Context,
    id: &str,
    index: u32,
    rate: u32,
    segment: Segment,
) -> Result<StoredSegment, String> {
    let file = ctx.root.join(id).join(format!("segment-{index:04}.wav"));
    seal_wav(&file, rate, &segment.samples)?;
    let row = StoredSegment {
        session_id: id.into(),
        index,
        path: file.to_string_lossy().into(),
        rate,
        start_sample: segment.start_sample,
        overlap_samples: segment.overlap_samples,
        reason: segment.reason.into(),
        text: None,
    };
    ctx.database
        .add_segment(&row)
        .map_err(|_| "session_storage".to_string())?;
    diagnostics::event(
        "segment_sealed",
        json!({"id":id,"index":index,"rate":rate,"samples":segment.samples.len(),"overlap":segment.overlap_samples,"reason":segment.reason}),
    );
    ctx.manager.update(&ctx.app, id, |s| s.total += 1);
    Ok(row)
}
fn update_preview(ctx: &Context, id: &str) -> Result<(), String> {
    let segments = ctx
        .database
        .load_segments(id)
        .map_err(|_| "session_storage".to_string())?;
    let completed = segments.iter().filter(|s| s.text.is_some()).count() as u32;
    let parts = segments
        .iter()
        .take_while(|s| s.text.is_some())
        .map(|s| (s.text.clone().unwrap_or_default(), s.overlap_samples > 0))
        .collect::<Vec<_>>();
    let text = merge_text(&parts);
    ctx.manager.update(&ctx.app, id, |s| {
        s.completed = completed;
        s.text = text;
    });
    Ok(())
}
fn enqueue(
    jobs: &mut JoinSet<Result<(), String>>,
    ctx: Context,
    segment: StoredSegment,
    config: Config,
    limit: Arc<Semaphore>,
    interruption: Arc<AtomicU8>,
    provider_blocked: Arc<Mutex<Option<String>>>,
) {
    jobs.spawn(async move {
        let _permit = limit
            .acquire_owned()
            .await
            .map_err(|_| "worker_closed".to_string())?;
        if interruption.load(Ordering::Acquire) != 0 {
            return Err("session_interrupted".into());
        }
        if let Some(error) = provider_blocked.lock().unwrap().clone() {
            return Err(error);
        }
        let (samples, rate) = crate::queue::read_wav_to_f32_with_rate(&segment.path)?;
        let text = ctx
            .client
            .for_source(&segment.session_id)
            .transcribe_with_model(
                samples,
                rate,
                &config.asr_model,
                &config.languages,
                config.hint.as_deref(),
            )
            .await;
        let text = match text {
            Ok(text) => text,
            Err(error) => {
                if crate::should_disable_auto_retry(&error) {
                    *provider_blocked.lock().unwrap() = Some(error.clone());
                }
                return Err(error);
            }
        };
        if interruption.load(Ordering::Acquire) != 0 {
            return Err("session_interrupted".into());
        }
        ctx.database
            .complete_segment(&segment.session_id, segment.index, &text)
            .map_err(|_| "session_storage".to_string())?;
        update_preview(&ctx, &segment.session_id)?;
        Ok(())
    });
}

async fn wait_for_interruption(interruption: &AtomicU8) {
    while interruption.load(Ordering::Acquire) == 0 {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn drain_jobs(
    jobs: &mut JoinSet<Result<(), String>>,
    interruption: &AtomicU8,
) -> Result<(), String> {
    let mut first_error = None;
    while !jobs.is_empty() {
        tokio::select! {
            result = jobs.join_next() => {
                if let Some(result) = result {
                    if let Err(error) = result.unwrap_or_else(|_| Err("worker_failed".into())) {
                        first_error.get_or_insert(error);
                    }
                }
            }
            _ = wait_for_interruption(interruption) => {
                jobs.abort_all();
                while jobs.join_next().await.is_some() {}
                return Err("session_interrupted".into());
            }
        }
    }
    if interruption.load(Ordering::Acquire) != 0 {
        return Err("session_interrupted".into());
    }
    first_error.map_or(Ok(()), Err)
}

pub async fn run(
    ctx: Context,
    id: String,
    mut config: Config,
    mut capture: Capture,
    interruption: Arc<AtomicU8>,
    target: Option<Target>,
) -> Result<Option<String>, String> {
    let start = Instant::now();
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    let mut raw = BufWriter::new(
        File::create(ctx.root.join(&id).join("audio.pcm"))
            .map_err(|_| "spool_write".to_string())?,
    );
    let mut segmenter = Segmenter::new(config.rate, config.segments.clone());
    let limit = Arc::new(Semaphore::new(2));
    let provider_blocked = Arc::new(Mutex::new(None));
    let mut jobs = JoinSet::new();
    let mut count = 0u32;
    let mut samples = 0u64;
    let mut first_error = None;
    loop {
        tokio::select! {
            frame=capture.frames.recv()=>{
                let Some(frame)=frame else {break;};
                for sample in &frame{raw.write_all(&((sample.clamp(-1.0,1.0)*i16::MAX as f32) as i16).to_le_bytes()).map_err(|_|"spool_write".to_string())?;}
                raw.flush().map_err(|_|"spool_write".to_string())?;
                samples+=frame.len() as u64;
                for segment in segmenter.push(&frame){
                    let row=persist_segment(&ctx,&id,count,config.rate,segment)?;count+=1;
                    enqueue(&mut jobs,ctx.clone(),row,config.clone(),limit.clone(),interruption.clone(),provider_blocked.clone());
                }
            }
            Some(result)=jobs.join_next(),if !jobs.is_empty()=>{if let Err(error)=result.unwrap_or_else(|_|Err("worker_failed".into())){first_error.get_or_insert(error);}}
            _=ticker.tick()=>{
                raw.flush().and_then(|_|raw.get_ref().sync_data()).map_err(|_|"spool_write".to_string())?;
                let duration=(samples*1000/config.rate as u64) as i64;
                ctx.manager.update(&ctx.app,&id,|s|s.duration_ms=duration);
                ctx.database.update_session(&id,"recording",duration,&ctx.manager.snapshot().text).map_err(|_|"session_storage".to_string())?;
                if start.elapsed()>=Duration::from_secs(360){capture.stop_handle().stop();ctx.manager.update(&ctx.app,&id,|s|s.phase="finalizing".into());}
            }
        }
        if interruption.load(Ordering::Acquire) != 0 {
            capture.stop_handle().stop();
        }
    }
    raw.flush()
        .and_then(|_| raw.get_ref().sync_all())
        .map_err(|_| "spool_write".to_string())?;
    drop(raw);
    // Freeze command choice at stop, before waiting for outstanding ASR requests.
    config.prompt_model = ctx
        .database
        .load_setting("selected_prompt_model")
        .ok()
        .flatten()
        .map(|s| crate::models::normalize(&s).to_string())
        .filter(|s| s != "transcribe-only");
    ctx.database
        .set_session_config(
            &id,
            &serde_json::to_string(&config).map_err(|_| "session_config".to_string())?,
        )
        .map_err(|_| "session_storage".to_string())?;
    *ctx.is_recording.lock().unwrap() = false;
    let _ = crate::system_audio::unmute_system_audio();
    if let Some(widget) = ctx.app.get_webview_window("recording-widget") {
        let _ = widget.hide();
    }
    let duration = (samples * 1000 / config.rate as u64) as i64;
    ctx.database
        .update_session(&id, "transcribing", duration, &ctx.manager.snapshot().text)
        .map_err(|_| "session_storage".to_string())?;
    ctx.manager.update(&ctx.app, &id, |s| {
        s.phase = "transcribing".into();
        s.duration_ms = duration;
    });
    if capture.failed() {
        jobs.abort_all();
        fail(&ctx, &id, "capture_failed", "incomplete");
        return Err("capture_failed".into());
    }
    if interruption.load(Ordering::Acquire) != 0 {
        jobs.abort_all();
        while jobs.join_next().await.is_some() {}
        let status = if interruption.load(Ordering::Acquire) == 1 {
            "cancelled"
        } else {
            "recoverable"
        };
        fail(&ctx, &id, "session_interrupted", status);
        return Ok(None);
    }
    if let Some(tail) = segmenter.finish() {
        let row = persist_segment(&ctx, &id, count, config.rate, tail)?;
        count += 1;
        enqueue(
            &mut jobs,
            ctx.clone(),
            row,
            config.clone(),
            limit,
            interruption.clone(),
            provider_blocked,
        );
    }
    if let Err(error) = drain_jobs(&mut jobs, &interruption).await {
        first_error.get_or_insert(error);
    }
    if interruption.load(Ordering::Acquire) != 0 {
        fail(&ctx, &id, "session_interrupted", "recoverable");
        return Ok(None);
    }
    if let Some(error) = first_error {
        fail(&ctx, &id, &error, "recoverable");
        return Err(error);
    }
    if count == 0 {
        ctx.database
            .update_session(&id, "cancelled", duration, "")
            .map_err(|_| "session_storage".to_string())?;
        ctx.manager.update(&ctx.app, &id, |s| {
            s.phase = "cancelled".into();
            s.error = Some("no_speech".into());
        });
        cleanup(&ctx.root, &id);
        return Ok(None);
    }
    finalize(&ctx, &id, &config, interruption, target, true).await
}

async fn finalize(
    ctx: &Context,
    id: &str,
    config: &Config,
    interruption: Arc<AtomicU8>,
    target: Option<Target>,
    automatic: bool,
) -> Result<Option<String>, String> {
    let segments = ctx
        .database
        .load_segments(id)
        .map_err(|_| "session_storage".to_string())?;
    if segments.iter().any(|s| s.text.is_none()) {
        return Err("segments_pending".into());
    }
    let text = config.input_text.clone().unwrap_or_else(|| {
        merge_text(
            &segments
                .iter()
                .map(|s| (s.text.clone().unwrap_or_default(), s.overlap_samples > 0))
                .collect::<Vec<_>>(),
        )
    });
    let record = ctx
        .database
        .load_session(id)
        .map_err(|_| "session_storage".to_string())?
        .ok_or("session_missing")?;
    if text.trim().is_empty() && config.image_data.is_none() {
        return Err("empty_transcription".into());
    }
    let duration = record.duration_ms;
    ctx.database
        .update_session(id, "transcribing", duration, &text)
        .map_err(|_| "session_storage".to_string())?;
    let result = if !record.result.is_empty() {
        record.result
    } else if let Some(model) = &config.prompt_model {
        ctx.manager
            .update(&ctx.app, id, |s| s.phase = "processing_command".into());
        let history = crate::get_conversation_history(&ctx.database);
        let client = ctx.client.for_source(id);
        let request = client.send_prompt_options(
            &text,
            model,
            &history,
            config.image_data.as_deref(),
            config.web_search,
        );
        let response = tokio::select! {
            result = request => result?,
            _ = wait_for_interruption(&interruption) => return Err("session_interrupted".into()),
        };
        ctx.database
            .stage_session_result(id, &response)
            .map_err(|_| "session_storage".to_string())?;
        response
    } else {
        text.clone()
    };
    if interruption.load(Ordering::Acquire) != 0 {
        return Err("session_interrupted".into());
    }
    let model = config.prompt_model.as_deref().unwrap_or(&config.asr_model);
    ctx.database
        .save_result_once(
            id,
            &result,
            Some(duration),
            model,
            if config.prompt_model.is_some() {
                "prompt"
            } else {
                "transcription"
            },
        )
        .map_err(|_| "session_storage".to_string())?;
    if config.prompt_model.is_some() {
        ctx.database
            .append_conversation_once(id, &text, &result)
            .map_err(|_| "session_storage".to_string())?;
    }
    ctx.database
        .update_session(id, "ready", duration, &text)
        .map_err(|_| "session_storage".to_string())?;
    ctx.database
        .release_session(id)
        .map_err(|_| "session_storage".to_string())?;
    *ctx.last_text.lock().unwrap() = Some(result.clone());
    ctx.manager.update(&ctx.app, id, |s| {
        s.phase = "ready".into();
        s.text = result.clone();
        s.error = None;
    });
    let _ = ctx.app.emit("history-updated", ());
    let _ = ctx.app.emit("sessions-updated", ());
    // A recovered result is never pasted into a potentially unrelated application.
    if automatic && interruption.load(Ordering::Acquire) == 0 {
        let app = ctx.app.clone();
        let paste = result.clone();
        let _ = tokio::task::spawn_blocking(move || delivery::deliver(&app, &paste, target)).await;
    }
    let _ = ctx.app.emit("response-ready", ());
    cleanup(&ctx.root, id);
    Ok(Some(result))
}

pub async fn recover(ctx: Context, id: String) -> Result<(), String> {
    let record = ctx
        .database
        .load_session(&id)
        .map_err(|_| "session_storage".to_string())?
        .ok_or("Sessão não encontrada.")?;
    if record.status == "incomplete" {
        return Err("A captura perdeu áudio. O original disponível foi preservado; esta sessão exige revisão manual.".into());
    }
    let config: Config =
        serde_json::from_str(&record.config).map_err(|_| "session_config".to_string())?;
    let interruption = ctx.manager.begin(&id, config.segments.enabled)?;
    let claimed = ctx
        .database
        .claim_session(&id)
        .map_err(|_| "session_storage".to_string());
    if !matches!(claimed, Ok(true)) {
        ctx.manager.update(&ctx.app, &id, |s| {
            s.phase = "recoverable".into();
            s.error = Some("session_unavailable".into());
        });
        ctx.manager.finished(&id);
        claimed?;
        return Err("Esta sessão já está em recuperação.".into());
    }
    ctx.manager.update(&ctx.app, &id, |s| {
        s.phase = "transcribing".into();
        s.duration_ms = record.duration_ms;
    });
    let result = recover_inner(&ctx, &id, &config, interruption).await;
    if let Err(error) = &result {
        fail(&ctx, &id, error, "recoverable");
    }
    ctx.manager.finished(&id);
    result.map(|_| ())
}
async fn recover_inner(
    ctx: &Context,
    id: &str,
    config: &Config,
    interruption: Arc<AtomicU8>,
) -> Result<Option<String>, String> {
    if config.input_text.is_some() {
        return finalize(ctx, id, config, interruption, None, false).await;
    }
    let mut segments = ctx
        .database
        .load_segments(id)
        .map_err(|_| "session_storage".to_string())?;
    // Raw PCM journal also covers the tail if the process ended between pause boundaries.
    let covered = segments
        .last()
        .map(|s| {
            hound::WavReader::open(&s.path)
                .map(|r| s.start_sample + r.duration() as u64)
                .map_err(|_| "spool_missing".to_string())
        })
        .transpose()?
        .unwrap_or(0);
    let raw =
        fs::read(ctx.root.join(id).join("audio.pcm")).map_err(|_| "spool_missing".to_string())?;
    let duration = (raw.len() as u64 / 2 * 1000 / config.rate as u64) as i64;
    ctx.database
        .update_session(id, "transcribing", duration, &ctx.manager.snapshot().text)
        .map_err(|_| "session_storage".to_string())?;
    ctx.manager
        .update(&ctx.app, id, |s| s.duration_ms = duration);
    if raw.len() as u64 / 2 > covered {
        let overlap = if segments.last().is_some_and(|s| s.reason == "limit") {
            (config.rate as u64 / 2).min(covered)
        } else {
            0
        };
        let tail_start = covered - overlap;
        let audio = raw[tail_start as usize * 2..]
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / i16::MAX as f32)
            .collect::<Vec<_>>();
        if audio.iter().any(|s| s.abs() > 0.00001) {
            let row = persist_segment(
                ctx,
                id,
                segments.len() as u32,
                config.rate,
                Segment {
                    samples: audio,
                    start_sample: tail_start,
                    overlap_samples: overlap as usize,
                    reason: "recovery",
                },
            )?;
            segments.push(row);
        }
    }
    ctx.manager
        .update(&ctx.app, id, |s| s.total = segments.len() as u32);
    let limit = Arc::new(Semaphore::new(2));
    let provider_blocked = Arc::new(Mutex::new(None));
    let mut jobs = JoinSet::new();
    update_preview(ctx, id)?;
    for segment in segments.iter().filter(|s| s.text.is_none()) {
        enqueue(
            &mut jobs,
            ctx.clone(),
            segment.clone(),
            config.clone(),
            limit.clone(),
            interruption.clone(),
            provider_blocked.clone(),
        );
    }
    drain_jobs(&mut jobs, &interruption).await?;
    update_preview(ctx, id)?;
    finalize(ctx, id, config, interruption, None, false).await
}
pub async fn run_text(
    ctx: &Context,
    id: &str,
    config: &Config,
    interruption: Arc<AtomicU8>,
    target: Option<Target>,
) -> Result<Option<String>, String> {
    finalize(ctx, id, config, interruption, target, true).await
}
fn cleanup(root: &Path, id: &str) {
    // id is generated internally and validated at the command boundary.
    let folder = root.join(id);
    if let Ok(entries) = fs::read_dir(&folder) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    let _ = fs::remove_dir(folder);
}
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() < 100 && id.chars().all(|c| c.is_ascii_digit() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audio_spool_is_durable_and_readable_at_real_rates() {
        let folder = std::env::temp_dir().join(format!("dicta-spool-{}", new_id()));
        fs::create_dir(&folder).unwrap();
        for rate in [16_000, 44_100, 48_000] {
            let file = folder.join(format!("segment-{rate}.wav"));
            let samples = vec![0.25f32; rate as usize];
            seal_wav(&file, rate, &samples)
                .expect("a sealed segment must be durable before upload");
            assert!(!file.with_extension("tmp").exists());
            let reader = hound::WavReader::open(&file).unwrap();
            assert_eq!(reader.spec().sample_rate, rate);
            assert_eq!(reader.spec().channels, 1);
            assert_eq!(reader.duration(), rate);
            let persisted = reader
                .into_samples::<i16>()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert_eq!(persisted.len(), samples.len());
            assert_eq!(persisted[0], (0.25 * i16::MAX as f32) as i16);
            fs::remove_file(file).unwrap();
        }
        fs::remove_dir(folder).unwrap();
    }
    #[test]
    fn session_ids_cannot_escape_spool() {
        assert!(valid_id(&new_id()));
        for id in ["..", "../escape", "C:/x", "x/y", ""] {
            assert!(!valid_id(id));
        }
    }
    #[test]
    fn late_session_cannot_begin_while_processing() {
        let manager = Manager::new();
        manager.begin("1", true).unwrap();
        assert!(manager.begin("2", true).is_err());
    }
    #[test]
    fn old_completion_cannot_release_new_session() {
        let manager = Manager::new();
        manager.begin("1", false).unwrap();
        manager.finished("1");
        manager.begin("2", false).unwrap();
        manager.finished("1");
        assert!(manager.busy());
    }
    #[tokio::test]
    async fn cancel_does_not_wait_for_provider_timeout() {
        let mut jobs = JoinSet::new();
        jobs.spawn(async {
            tokio::time::sleep(Duration::from_secs(90)).await;
            Ok(())
        });
        let flag = AtomicU8::new(1);
        let result =
            tokio::time::timeout(Duration::from_secs(1), drain_jobs(&mut jobs, &flag)).await;
        assert_eq!(result.unwrap().unwrap_err(), "session_interrupted");
        assert!(jobs.is_empty());
    }
}
