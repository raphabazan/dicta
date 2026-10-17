mod audio;
mod capture;
mod db;
mod delivery;
mod diagnostics;
mod dictation;
mod lifecycle;
pub mod openai;
mod queue;
pub mod segmentation;
mod system_audio;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, State};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};

#[cfg(target_os = "windows")]
use windows::core::PWSTR;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::CloseHandle;
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

fn ts() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let millis = now.subsec_millis();
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    format!("[{:02}:{:02}:{:02}.{:03}]", hours, minutes, seconds, millis)
}

macro_rules! tlog {
    ($($arg:tt)*) => {
        println!("{} {}", ts(), format!($($arg)*));
    };
}

// Re-export TranscriptionEntry from db module
use db::TranscriptionEntry;

fn context(state: &AppState, app: AppHandle) -> dictation::Context {
    dictation::Context {
        app,
        database: state.database.clone(),
        client: state.openai_client.clone(),
        manager: state.sessions.clone(),
        last_text: state.last_transcription.clone(),
        is_recording: state.is_recording.clone(),
        root: state.session_dir.clone(),
    }
}

fn load_segment_config(database: &db::Database) -> segmentation::SegmentConfig {
    database
        .load_setting("segment_config")
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(|c: &segmentation::SegmentConfig| c.validate().is_ok())
        .unwrap_or_default()
}

fn toggle_recording_from_hotkey(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let command_app = app.clone();
        let state = app.state::<AppState>();
        let recording = *state.is_recording.lock().unwrap();
        let result = if recording {
            stop_recording_audio(state, command_app).await
        } else {
            start_recording_audio(state, command_app).await
        };
        if result.is_err() {
            diagnostics::event(
                "recording_toggle_failed",
                serde_json::json!({"was_recording":recording}),
            );
            let _ = app.emit(
                "recording-error",
                "Não foi possível iniciar ou encerrar a gravação. Confira a sessão no Dicta.",
            );
        }
    });
}

struct AppState {
    openai_client: Arc<openai::OpenAIClient>,
    database: Arc<db::Database>,
    sessions: Arc<dictation::Manager>,
    session_dir: PathBuf,
    next_target: Arc<Mutex<Option<delivery::Target>>>,
    is_recording: Arc<Mutex<bool>>,
    prompt_mode: Arc<Mutex<Option<String>>>,
    last_transcription: Arc<Mutex<Option<String>>>,
    tts_enabled: Arc<Mutex<bool>>,
    tts_sink: Arc<Mutex<Option<rodio::Sink>>>,
    tts_stream_handle: Arc<Mutex<Option<rodio::OutputStreamHandle>>>,
    tts_active: Arc<Mutex<bool>>,
    queue_dir: PathBuf,
    shortcut_errors: Arc<Mutex<Vec<String>>>,
}

const DEFAULT_PROMPT_MODEL: &str = "gpt-5.4";
const DEFAULT_TRANSCRIPTION_LANGUAGES: [&str; 2] = ["pt", "en"];
const DEFAULT_GAME_MODE_ENABLED: bool = false;

fn parse_bool_setting(value: Option<String>, default: bool) -> bool {
    value
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(default)
}

fn normalize_prompt_model(model: &str) -> String {
    match model {
        "gpt-5.4" | "gpt-6.1-sol" | "gpt-4o-mini" | "transcribe-only" => model.to_string(),
        _ => "transcribe-only".to_string(),
    }
}

fn load_selected_prompt_model(database: &db::Database) -> String {
    database
        .load_setting("selected_prompt_model")
        .ok()
        .flatten()
        .map(|model| normalize_prompt_model(&model))
        .unwrap_or_else(|| "transcribe-only".to_string())
}

fn load_user_prompt_model(database: &db::Database) -> String {
    database
        .load_setting("user_prompt_model")
        .ok()
        .flatten()
        .map(|model| normalize_prompt_model(&model))
        .filter(|model| model != "transcribe-only")
        .unwrap_or_else(|| DEFAULT_PROMPT_MODEL.to_string())
}

fn normalize_transcription_languages(languages: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();

    for language in languages {
        let value = language.trim().to_lowercase();
        if value.is_empty() {
            continue;
        }

        if !normalized.iter().any(|existing| existing == &value) {
            normalized.push(value);
        }
    }

    if normalized.is_empty() {
        DEFAULT_TRANSCRIPTION_LANGUAGES
            .iter()
            .map(|value| value.to_string())
            .collect()
    } else {
        normalized
    }
}

fn load_transcription_languages(database: &db::Database) -> Vec<String> {
    let stored = database
        .load_setting("transcription_languages")
        .ok()
        .flatten()
        .map(|value| {
            value
                .split(',')
                .map(|part| part.trim().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    normalize_transcription_languages(stored)
}

fn save_transcription_languages(
    database: &db::Database,
    languages: Vec<String>,
) -> Result<Vec<String>, String> {
    let normalized = normalize_transcription_languages(languages);
    database
        .save_setting("transcription_languages", &normalized.join(","))
        .map_err(|e| format!("Failed to save transcription languages: {}", e))?;
    Ok(normalized)
}

fn normalize_game_mode_processes(processes: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();

    for process in processes {
        let value = process.trim().to_lowercase();
        if value.is_empty() {
            continue;
        }

        if !normalized.iter().any(|existing| existing == &value) {
            normalized.push(value);
        }
    }

    normalized
}

fn load_game_mode_enabled(database: &db::Database) -> bool {
    parse_bool_setting(
        database.load_setting("game_mode_enabled").ok().flatten(),
        DEFAULT_GAME_MODE_ENABLED,
    )
}

fn load_game_mode_processes(database: &db::Database) -> Vec<String> {
    let stored = database
        .load_setting("game_mode_processes")
        .ok()
        .flatten()
        .map(|value| {
            value
                .split(',')
                .map(|part| part.trim().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    normalize_game_mode_processes(stored)
}

fn save_game_mode_processes(
    database: &db::Database,
    processes: Vec<String>,
) -> Result<Vec<String>, String> {
    let normalized = normalize_game_mode_processes(processes);
    database
        .save_setting("game_mode_processes", &normalized.join(","))
        .map_err(|e| format!("Failed to save game mode processes: {}", e))?;
    Ok(normalized)
}

#[cfg(target_os = "windows")]
fn get_foreground_process_info() -> Option<(String, String)> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return None;
    }

    let mut process_id = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut process_id));
    }
    if process_id == 0 {
        return None;
    }

    let process_handle =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }.ok()?;
    let mut buffer = vec![0u16; 1024];
    let mut len = buffer.len() as u32;

    let result = unsafe {
        QueryFullProcessImageNameW(
            process_handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut len,
        )
    };

    let _ = unsafe { CloseHandle(process_handle) };
    if result.is_err() {
        return None;
    }

    let full_path = String::from_utf16_lossy(&buffer[..len as usize]).to_lowercase();
    let process_name = PathBuf::from(&full_path)
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_lowercase())?;

    Some((process_name, full_path))
}

#[cfg(not(target_os = "windows"))]
fn get_foreground_process_info() -> Option<(String, String)> {
    None
}

fn should_block_hotkeys_for_game_mode(database: &db::Database) -> bool {
    if !load_game_mode_enabled(database) {
        return false;
    }

    let Some((process_name, process_path)) = get_foreground_process_info() else {
        return false;
    };

    let blocked_processes = load_game_mode_processes(database);
    let is_steam_game = process_path.contains("steamapps\\common\\");
    let is_blocked_process = blocked_processes.iter().any(|value| value == &process_name);

    if is_steam_game || is_blocked_process {
        tlog!(
            "Game mode blocked hotkey for foreground process {} ({})",
            process_name,
            process_path
        );
        return true;
    }

    false
}

fn build_transcription_hints(database: &db::Database) -> (Option<String>, Option<String>) {
    let languages = load_transcription_languages(database);

    if languages.len() == 1 {
        return (Some(languages[0].clone()), None);
    }

    let display_names: Vec<&str> = languages
        .iter()
        .map(|code| match code.as_str() {
            "pt" => "Portuguese",
            "en" => "English",
            "es" => "Spanish",
            "fr" => "French",
            "de" => "German",
            "it" => "Italian",
            other => other,
        })
        .collect();

    (
        None,
        Some(format!(
            "The audio may contain any of these spoken languages: {}.",
            display_names.join(", ")
        )),
    )
}

/// Strip markdown links, citations, and raw URLs from text for TTS playback.
fn strip_links_for_tts(text: &str) -> String {
    // 1. Markdown links [text](url) → text
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '[' {
            let mut link_text = String::new();
            let mut found_close = false;
            for inner in chars.by_ref() {
                if inner == ']' {
                    found_close = true;
                    break;
                }
                link_text.push(inner);
            }
            if found_close && chars.peek() == Some(&'(') {
                chars.next(); // skip '('
                let mut depth = 1;
                for inner in chars.by_ref() {
                    if inner == '(' {
                        depth += 1;
                    }
                    if inner == ')' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                }
                // Only keep link text if it doesn't look like a domain
                if !looks_like_url(&link_text) {
                    result.push_str(&link_text);
                }
            } else {
                result.push('[');
                result.push_str(&link_text);
                if found_close {
                    result.push(']');
                }
            }
        } else {
            result.push(c);
        }
    }
    // 2. Remove parenthesized content that looks like URLs/domains: (domain.com), (https://...)
    let mut clean = String::with_capacity(result.len());
    let mut i = 0;
    while i < result.len() {
        if result.as_bytes()[i] == b'(' {
            let start = i + 1;
            if let Some(end) = result[start..].find(')') {
                let inner = result[start..start + end].trim();
                if looks_like_url(inner) {
                    i = start + end + 1;
                    continue;
                }
            }
        }
        clean.push(result.as_bytes()[i] as char);
        i += 1;
    }
    // 3. Remove remaining raw URLs
    clean
        .split_whitespace()
        .filter(|w| !looks_like_url(w))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Check if a string looks like a URL or domain name
fn looks_like_url(s: &str) -> bool {
    let s = s.trim_matches(|c: char| c == '(' || c == ')' || c == '[' || c == ']');
    if s.starts_with("http://") || s.starts_with("https://") || s.starts_with("www.") {
        return true;
    }
    // Match domain patterns like "site.com", "site.com.br", "site.org/path"
    let tlds = [
        ".com", ".org", ".net", ".br", ".io", ".dev", ".gov", ".edu", ".info", ".co",
    ];
    tlds.iter().any(|tld| s.contains(tld))
}

/// Play TTS audio in chunks (sentence by sentence) with visual widget feedback.
/// Each chunk is generated and played sequentially so audio starts fast.
/// Can be cancelled by setting tts_active to false.
async fn play_tts_chunked(
    app: AppHandle,
    text: String,
    openai: Arc<openai::OpenAIClient>,
    _tts_sink: Arc<Mutex<Option<rodio::Sink>>>,
    _tts_stream_handle: Arc<Mutex<Option<rodio::OutputStreamHandle>>>,
    tts_active: Arc<Mutex<bool>>,
) {
    // Set active flag
    *tts_active.lock().unwrap() = true;

    // Show TTS widget
    if let Some(w) = app.get_webview_window("tts-widget") {
        if let Ok(Some(monitor)) = app.primary_monitor() {
            let screen = monitor.size();
            let x = (screen.width as i32 - 100) / 2;
            let y = screen.height as i32 - 32 - 120;
            let _ = w.set_position(PhysicalPosition::new(x, y));
        }
        let _ = w.show();
    }

    // Strip markdown links and raw URLs so TTS doesn't read them
    let clean_text = strip_links_for_tts(&text);
    let chunks = openai::split_into_tts_chunks(&clean_text);
    println!("🔊 TTS chunked playback: {} chunks", chunks.len());

    // Channel to send audio bytes from async context to the playback thread.
    // The playback thread owns the OutputStream (not Send) and creates Sinks.
    let (audio_tx, audio_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let tts_active_for_thread = tts_active.clone();

    // Dedicated playback thread — owns the OutputStream so it always uses
    // the current default output device (not the one from app startup).
    let playback_thread = std::thread::spawn(move || {
        let (_stream, stream_handle) = match rodio::OutputStream::try_default() {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("❌ Failed to open audio output for TTS: {}", e);
                return;
            }
        };

        let mut current_sink: Option<rodio::Sink> = None;

        while let Ok(audio_bytes) = audio_rx.recv() {
            // Check if cancelled
            if !*tts_active_for_thread.lock().unwrap() {
                break;
            }

            // Wait for previous chunk to finish
            if let Some(ref sink) = current_sink {
                while !sink.empty() {
                    if !*tts_active_for_thread.lock().unwrap() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                if !*tts_active_for_thread.lock().unwrap() {
                    break;
                }
            }

            // Stop previous and play new
            if let Some(s) = current_sink.take() {
                s.stop();
            }

            if let Ok(src) = rodio::Decoder::new(std::io::Cursor::new(audio_bytes)) {
                if let Ok(sink) = rodio::Sink::try_new(&stream_handle) {
                    sink.append(src);
                    current_sink = Some(sink);
                }
            }
        }

        // Wait for last chunk to finish playing
        if let Some(ref sink) = current_sink {
            while !sink.empty() {
                if !*tts_active_for_thread.lock().unwrap() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
        // _stream drops here, releasing output device
    });

    for (i, chunk) in chunks.iter().enumerate() {
        if !*tts_active.lock().unwrap() {
            println!("🔇 TTS cancelled at chunk {}/{}", i + 1, chunks.len());
            break;
        }

        println!(
            "🔊 TTS chunk {}/{}: generating audio for {} chars...",
            i + 1,
            chunks.len(),
            chunk.len()
        );

        match openai.speak_text(chunk).await {
            Ok(audio) => {
                if !*tts_active.lock().unwrap() {
                    println!("🔇 TTS cancelled after generating chunk {}", i + 1);
                    break;
                }
                println!("🔊 TTS chunk {}/{} sent to playback", i + 1, chunks.len());
                if audio_tx.send(audio).is_err() {
                    println!("🔇 Playback thread closed");
                    break;
                }
            }
            Err(e) => {
                eprintln!("❌ TTS chunk {} failed: {}", i + 1, e);
            }
        }
    }

    // Drop sender to signal playback thread there's no more data
    drop(audio_tx);

    // Wait for playback thread to finish
    let _ = playback_thread.join();

    // Clean up
    *tts_active.lock().unwrap() = false;
    if let Some(w) = app.get_webview_window("tts-widget") {
        let _ = w.hide();
    }
    println!("🔊 TTS chunked playback finished");
}

#[tauri::command]
async fn cancel_recording(state: State<'_, AppState>, app: AppHandle) -> Result<String, String> {
    state.sessions.interrupt(&app, true);
    *state.is_recording.lock().unwrap() = false;
    let _ = system_audio::unmute_system_audio();
    Ok("Recording cancelled".into())
}

#[tauri::command]
async fn start_recording_audio(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<String, String> {
    let ctx = context(&state, app.clone());
    let id = dictation::new_id();
    let model = state
        .database
        .load_setting("asr_model")
        .ok()
        .flatten()
        .filter(|m| ["whisper-1", "gpt-transcribe"].contains(&m.as_str()))
        .unwrap_or_else(|| "whisper-1".into());
    let (_, hint) = build_transcription_hints(&state.database);
    let mut config = dictation::Config {
        segments: load_segment_config(&state.database),
        asr_model: model,
        prompt_model: Some(load_selected_prompt_model(&state.database))
            .filter(|m| m != "transcribe-only"),
        languages: load_transcription_languages(&state.database),
        hint,
        web_search: parse_bool_setting(
            state
                .database
                .load_setting("web_search_enabled")
                .ok()
                .flatten(),
            false,
        ),
        rate: 0,
        input_text: None,
        image_data: None,
    };
    let microphone = state
        .database
        .load_setting("selected_microphone")
        .ok()
        .flatten();
    let target = state
        .next_target
        .lock()
        .unwrap()
        .take()
        .or_else(delivery::capture_target);
    let ctx_prepare = ctx.clone();
    let prepare_id = id.clone();
    let prepared = tokio::task::spawn_blocking(move || {
        let result = dictation::prepare(&ctx_prepare, &mut config, microphone, &prepare_id);
        (result, config)
    })
    .await
    .map_err(|_| "Falha ao iniciar captura.".to_string())?;
    let (capture, interruption) = match prepared.0 {
        Ok(value) => value,
        Err(error) => {
            ctx.manager.finished(&id);
            return Err(error);
        }
    };
    *state.is_recording.lock().unwrap() = true;
    if !(cfg!(debug_assertions) && std::env::var_os("DICTA_TEST_AUDIO").is_some()) {
        let _ = system_audio::mute_system_audio();
    }
    let tts = (
        state.tts_enabled.clone(),
        state.tts_sink.clone(),
        state.tts_stream_handle.clone(),
        state.tts_active.clone(),
    );
    tokio::spawn(async move {
        let result = dictation::run(
            ctx.clone(),
            id.clone(),
            prepared.1,
            capture,
            interruption,
            target,
        )
        .await;
        *ctx.is_recording.lock().unwrap() = false;
        let _ = system_audio::unmute_system_audio();
        if let Some(widget) = ctx.app.get_webview_window("recording-widget") {
            let _ = widget.hide();
        }
        if let Err(error) = &result {
            if ctx.manager.snapshot().phase != "incomplete" {
                dictation::fail(&ctx, &id, error, "recoverable");
            }
        }
        ctx.manager.finished(&id);
        if let Ok(Some(text)) = result {
            let prompt = ctx
                .database
                .load_setting("selected_prompt_model")
                .ok()
                .flatten()
                .is_some_and(|m| m != "transcribe-only");
            if prompt && *tts.0.lock().unwrap() {
                tauri::async_runtime::spawn(play_tts_chunked(
                    ctx.app.clone(),
                    text,
                    ctx.client.clone(),
                    tts.1,
                    tts.2,
                    tts.3,
                ));
            }
        }
    });
    Ok("Recording started".into())
}

#[tauri::command]
async fn stop_recording_audio(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<String, String> {
    state.sessions.stop(&app)?;
    *state.is_recording.lock().unwrap() = false;
    let _ = app.emit("recording-sound-stop", ());
    Ok("Finalizing recording".into())
}

#[tauri::command]
fn get_session_state(state: State<'_, AppState>) -> dictation::Snapshot {
    state.sessions.snapshot()
}

#[tauri::command]
fn get_segment_config(state: State<'_, AppState>) -> segmentation::SegmentConfig {
    load_segment_config(&state.database)
}

#[tauri::command]
fn set_segment_config(
    state: State<'_, AppState>,
    config: segmentation::SegmentConfig,
) -> Result<(), String> {
    if state.sessions.busy() {
        return Err("Aguarde a sessão atual terminar.".into());
    }
    config.validate()?;
    state
        .database
        .save_setting(
            "segment_config",
            &serde_json::to_string(&config).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_asr_model(state: State<'_, AppState>) -> String {
    state
        .database
        .load_setting("asr_model")
        .ok()
        .flatten()
        .unwrap_or_else(|| "whisper-1".into())
}

#[tauri::command]
fn set_asr_model(state: State<'_, AppState>, model: String) -> Result<(), String> {
    if state.sessions.busy() {
        return Err("Aguarde a sessão atual terminar.".into());
    }
    if !["whisper-1", "gpt-transcribe"].contains(&model.as_str()) {
        return Err("Modelo de transcrição não suportado.".into());
    }
    state
        .database
        .save_setting("asr_model", &model)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_recoverable_sessions(state: State<'_, AppState>) -> Result<Vec<serde_json::Value>, String> {
    state.database.load_recoverable_sessions().map(|rows|rows.into_iter().map(|row|serde_json::json!({"id":row.id,"status":row.status,"created_at":row.created_at,"duration_ms":row.duration_ms})).collect()).map_err(|e|e.to_string())
}

#[tauri::command]
fn get_web_search_enabled(state: State<'_, AppState>) -> bool {
    parse_bool_setting(
        state
            .database
            .load_setting("web_search_enabled")
            .ok()
            .flatten(),
        false,
    )
}
#[tauri::command]
fn set_web_search_enabled(state: State<'_, AppState>, enabled: bool) -> Result<(), String> {
    state
        .database
        .save_setting("web_search_enabled", if enabled { "true" } else { "false" })
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn retry_session(
    state: State<'_, AppState>,
    app: AppHandle,
    id: String,
) -> Result<(), String> {
    if !dictation::valid_id(&id) {
        return Err("Sessão inválida.".into());
    }
    let ctx = context(&state, app);
    dictation::recover(ctx, id).await
}

#[tauri::command]
fn discard_session(state: State<'_, AppState>, id: String) -> Result<(), String> {
    if !dictation::valid_id(&id) || state.sessions.busy() {
        return Err("Não é possível descartar esta sessão agora.".into());
    }
    state
        .database
        .update_session(&id, "cancelled", 0, "")
        .map_err(|e| e.to_string())?;
    // Remove only generated files in the verified session directory; no recursive deletion.
    let root = state.session_dir.join(&id);
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let _ = std::fs::remove_dir(root);
    Ok(())
}

#[tauri::command]
async fn paste_last_transcription(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<delivery::Outcome, String> {
    let text = state
        .database
        .load_latest_transcription()
        .map_err(|e| e.to_string())?
        .ok_or("Nenhuma transcrição disponível.")?
        .text;
    let target = delivery::capture_target();
    tokio::task::spawn_blocking(move || delivery::deliver(&app, &text, target))
        .await
        .map_err(|_| "Falha na entrega.".into())
}

#[tauri::command]
fn get_build_info() -> String {
    diagnostics::build_id().into()
}

#[tauri::command]
fn get_shortcut_errors(state: State<'_, AppState>) -> Vec<String> {
    state.shortcut_errors.lock().unwrap().clone()
}

#[tauri::command]
fn recover_runtime(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    delivery::invalidate();
    state.sessions.interrupt(&app, false);
    *state.is_recording.lock().unwrap() = false;
    system_audio::unmute_system_audio()?;
    let _ = app.emit("runtime-resumed", ());
    Ok(())
}

#[tauri::command]
fn get_last_transcription(state: State<'_, AppState>) -> Result<String, String> {
    let last = state.last_transcription.lock().unwrap();
    match &*last {
        Some(text) => Ok(text.clone()),
        None => Err("No transcription available".to_string()),
    }
}

#[tauri::command]
fn get_transcription_history(
    state: State<'_, AppState>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<TranscriptionEntry>, String> {
    state
        .database
        .load_history_page(limit.unwrap_or(100), offset.unwrap_or(0))
        .map_err(|e| format!("Failed to load history: {}", e))
}

#[tauri::command]
fn copy_to_clipboard(app: AppHandle, text: String) -> Result<(), String> {
    app.clipboard()
        .write_text(text)
        .map_err(|e| format!("Failed to write to clipboard: {}", e))
}

#[tauri::command]
fn set_use_realtime(use_realtime: bool) -> Result<(), String> {
    if use_realtime {
        Err("O Realtime legado foi retirado. Use o envio por pausas nas configurações.".into())
    } else {
        Ok(())
    }
}
#[tauri::command]
fn get_use_realtime() -> bool {
    false
}

#[tauri::command]
fn list_microphones() -> Result<Vec<String>, String> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();
    let devices: Vec<String> = host
        .input_devices()
        .map_err(|e| format!("Failed to get input devices: {}", e))?
        .filter_map(|device| device.name().ok())
        .collect();

    Ok(devices)
}

#[tauri::command]
fn set_selected_microphone(state: State<'_, AppState>, device_name: String) -> Result<(), String> {
    state
        .database
        .save_setting("selected_microphone", &device_name)
        .map_err(|e| format!("Failed to save microphone setting: {}", e))?;
    println!("🎤 Selected microphone: {}", device_name);
    Ok(())
}

#[tauri::command]
fn get_selected_microphone(state: State<'_, AppState>) -> Result<Option<String>, String> {
    state
        .database
        .load_setting("selected_microphone")
        .map_err(|e| format!("Failed to load microphone setting: {}", e))
}

#[tauri::command]
fn set_selected_prompt_model(
    state: State<'_, AppState>,
    model: String,
    save_as_default: Option<bool>,
) -> Result<(), String> {
    let model = normalize_prompt_model(&model);

    // Save as current session model
    state
        .database
        .save_setting("selected_prompt_model", &model)
        .map_err(|e| format!("Failed to save prompt model setting: {}", e))?;

    *state.prompt_mode.lock().unwrap() = if model == "transcribe-only" {
        None
    } else {
        Some(model.clone())
    };

    // Only save as user_prompt_model (for Ctrl+Shift+Space) when explicitly requested
    // This prevents Ctrl+B or Ctrl+Alt+Space from overwriting the user's preferred model
    if save_as_default.unwrap_or(false) && model != "transcribe-only" {
        state
            .database
            .save_setting("user_prompt_model", &model)
            .map_err(|e| format!("Failed to save user prompt model: {}", e))?;
        println!(
            "💾 Selected prompt model: {} (also saved as user_prompt_model)",
            model
        );
    } else {
        println!(
            "💾 Selected prompt model: {} (user_prompt_model unchanged)",
            model
        );
    }
    Ok(())
}

#[tauri::command]
fn get_selected_prompt_model(state: State<'_, AppState>) -> Result<Option<String>, String> {
    Ok(Some(load_selected_prompt_model(&state.database)))
}

#[tauri::command]
fn get_user_prompt_model(state: State<'_, AppState>) -> Result<String, String> {
    Ok(load_user_prompt_model(&state.database))
}

#[tauri::command]
fn get_current_recording_mode(state: State<'_, AppState>) -> Result<String, String> {
    // Return the model that should be pre-selected based on current prompt_mode
    let prompt_mode = state.prompt_mode.lock().unwrap().clone();

    let model = match prompt_mode.as_deref() {
        Some("gpt-4o-mini") => "gpt-4o-mini".to_string(),
        Some("gpt-5.4") => "gpt-5.4".to_string(),
        Some("gpt-6.1-sol") => "gpt-6.1-sol".to_string(),
        None => "transcribe-only".to_string(),
        Some(other) => {
            println!(
                "⚠️ Unknown prompt mode: {}, defaulting to transcribe-only",
                other
            );
            "transcribe-only".to_string()
        }
    };

    println!(
        "📋 get_current_recording_mode: prompt_mode={:?}, returning model={}",
        prompt_mode, model
    );
    Ok(model)
}

#[tauri::command]
fn get_transcription_languages(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    Ok(load_transcription_languages(&state.database))
}

#[tauri::command]
fn set_transcription_languages(
    state: State<'_, AppState>,
    languages: Vec<String>,
) -> Result<Vec<String>, String> {
    save_transcription_languages(&state.database, languages)
}

#[tauri::command]
fn get_game_mode_enabled(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(load_game_mode_enabled(&state.database))
}

#[tauri::command]
fn set_game_mode_enabled(state: State<'_, AppState>, enabled: bool) -> Result<(), String> {
    state
        .database
        .save_setting("game_mode_enabled", if enabled { "true" } else { "false" })
        .map_err(|e| format!("Failed to save game mode setting: {}", e))
}

#[tauri::command]
fn get_game_mode_processes(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    Ok(load_game_mode_processes(&state.database))
}

#[tauri::command]
fn set_game_mode_processes(
    state: State<'_, AppState>,
    processes: Vec<String>,
) -> Result<Vec<String>, String> {
    save_game_mode_processes(&state.database, processes)
}

// Removed start_pre_buffering - pre-buffering logic moved to audio capture

/// Load conversation history, clearing it first if inactive for 30+ minutes.
fn get_conversation_history(database: &db::Database) -> Vec<db::ConversationMessage> {
    const INACTIVITY_MS: i64 = 30 * 60 * 1000; // 30 minutes

    if let Ok(Some(last_ts)) = database.last_conversation_timestamp() {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        if now_ms - last_ts > INACTIVITY_MS {
            tlog!("🕐 Conversation inactive >30min, clearing history");
            let _ = database.clear_conversation_history();
            return vec![];
        }
    }

    database.load_conversation_history(6).unwrap_or_default()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn emit_queue_updated(app: &AppHandle, database: &db::Database) {
    let count = database.count_queue().unwrap_or(0);
    tlog!("Queue updated, {} items pending", count);
    let _ = app.emit("queue-updated", count);
}

fn should_disable_auto_retry(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("insufficient_quota")
        || [400, 401, 403, 404, 422]
            .iter()
            .any(|status| lower.starts_with(&format!("api error ({status}):")))
        || lower.contains("billing_hard_limit_reached")
        || lower.contains("invalid_api_key")
        || lower.contains("incorrect api key")
        || lower.contains("account_deactivated")
        || lower.contains("model_not_found")
}

fn compact_queue_error(error: &str) -> String {
    let single_line = error.replace(['\r', '\n'], " ");
    let trimmed = single_line.split_whitespace().collect::<Vec<_>>().join(" ");
    let max_len = 240;
    if trimmed.chars().count() <= max_len {
        trimmed
    } else {
        format!("{}...", trimmed.chars().take(max_len).collect::<String>())
    }
}

#[cfg(test)]
mod regression_tests {
    #[test]
    fn queue_errors_are_unicode_safe_and_bounded() {
        for text in [
            format!("{}é final", "a".repeat(239)),
            "á".repeat(500),
            "erro\n com\r espaços".into(),
        ] {
            let result = super::compact_queue_error(&text);
            assert!(result.chars().count() <= 243);
            assert!(!result.contains('\n'));
            assert!(!result.contains('\r'));
        }
    }
    #[test]
    fn permanent_quota_and_model_errors_stop_automatic_retry() {
        assert!(super::should_disable_auto_retry(
            "API error (429): insufficient_quota"
        ));
        assert!(super::should_disable_auto_retry(
            "API error (404): model_not_found"
        ));
        assert!(!super::should_disable_auto_retry("api_timeout"));
        assert!(super::should_disable_auto_retry(
            "API error (401): api_error"
        ));
        assert!(!super::should_disable_auto_retry(
            "API error (429): rate_limit_exceeded"
        ));
    }
}

/// Estimate cost in hundredths of a cent based on model and usage
fn estimate_cost_cents(model: &str, duration_ms: Option<i64>, text: &str) -> i64 {
    match model {
        "whisper" | "whisper-1" | "realtime" | "gpt-transcribe" => {
            // $0.006/min of audio
            let minutes = duration_ms.unwrap_or(0) as f64 / 60_000.0;
            (minutes
                * if model == "gpt-transcribe" {
                    0.0045
                } else {
                    0.006
                }
                * 10_000.0) as i64
        }
        "gpt-4o-mini" => {
            // ~$0.60/1M output tokens, ~4 chars/token
            let tokens = text.len() as f64 / 4.0;
            (tokens * 0.60 / 1_000_000.0 * 10_000.0) as i64
        }
        "gpt-5.4" | "gpt-6.1-sol" => {
            // ~$15/1M output tokens
            let tokens = text.len() as f64 / 4.0;
            (tokens * if model == "gpt-6.1-sol" { 10.0 } else { 15.0 } / 1_000_000.0 * 10_000.0)
                as i64
        }
        _ => 0,
    }
}

#[tauri::command]
async fn send_text_prompt(
    state: State<'_, AppState>,
    app: AppHandle,
    prompt: String,
    model: String,
    image_data: Option<String>,
) -> Result<(), String> {
    let model = normalize_prompt_model(&model);
    if model == "transcribe-only" || (prompt.trim().is_empty() && image_data.is_none()) {
        return Err("Selecione um modelo e escreva um comando.".into());
    }
    if image_data.as_ref().is_some_and(|i| i.len() > 16_000_000) {
        return Err("Imagem muito grande.".into());
    }
    let id = dictation::new_id();
    let interruption = state.sessions.begin(&id, false)?;
    let ctx = context(&state, app.clone());
    let config = dictation::Config {
        segments: segmentation::SegmentConfig::default(),
        asr_model: "whisper-1".into(),
        prompt_model: Some(model),
        languages: vec![],
        hint: None,
        web_search: parse_bool_setting(
            state
                .database
                .load_setting("web_search_enabled")
                .ok()
                .flatten(),
            false,
        ),
        rate: 16000,
        input_text: Some(prompt),
        image_data,
    };
    let target = state.next_target.lock().unwrap().take();
    let saved = state.database.create_session(&db::SessionRecord {
        id: id.clone(),
        status: "transcribing".into(),
        created_at: now_ms(),
        duration_ms: 0,
        config: serde_json::to_string(&config).map_err(|e| e.to_string())?,
        text: String::new(),
        result: String::new(),
    });
    if saved.is_err() {
        state.sessions.finished(&id);
        return Err("Não foi possível preservar o comando.".into());
    }
    state
        .sessions
        .update(&app, &id, |s| s.phase = "processing_command".into());
    let tts = (
        state.tts_enabled.clone(),
        state.tts_sink.clone(),
        state.tts_stream_handle.clone(),
        state.tts_active.clone(),
    );
    tokio::spawn(async move {
        let result = dictation::run_text(&ctx, &id, &config, interruption, target).await;
        if let Err(error) = &result {
            dictation::fail(&ctx, &id, error, "recoverable");
        }
        ctx.manager.finished(&id);
        if let Ok(Some(text)) = result {
            if *tts.0.lock().unwrap() {
                tauri::async_runtime::spawn(play_tts_chunked(
                    ctx.app.clone(),
                    text,
                    ctx.client.clone(),
                    tts.1,
                    tts.2,
                    tts.3,
                ));
            }
        }
    });
    Ok(())
}

#[tauri::command]
async fn get_statistics(
    state: State<'_, AppState>,
    from_ts: i64,
    to_ts: i64,
) -> Result<db::StatsData, String> {
    state
        .database
        .get_stats(from_ts, to_ts)
        .map_err(|e| format!("Failed to get stats: {}", e))
}

#[tauri::command]
fn get_tts_enabled(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(*state.tts_enabled.lock().unwrap())
}

#[tauri::command]
fn set_tts_enabled(
    state: State<'_, AppState>,
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    *state.tts_enabled.lock().unwrap() = enabled;
    state
        .database
        .save_setting("tts_enabled", if enabled { "true" } else { "false" })
        .map_err(|e| format!("Failed to save TTS setting: {}", e))?;
    println!("🔊 TTS {}", if enabled { "enabled" } else { "disabled" });
    // Notify frontend
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit("tts-toggled", enabled);
    }
    // Also notify recording widget
    if let Some(widget) = app.get_webview_window("recording-widget") {
        let _ = widget.emit("tts-toggled", enabled);
    }
    // Show toast
    if let Some(toast) = app.get_webview_window("tts-toast") {
        if let Ok(Some(monitor)) = app.primary_monitor() {
            let screen = monitor.size();
            let x = (screen.width as i32 - 140) / 2;
            let y = screen.height as i32 - 40 - 300;
            let _ = toast.set_position(PhysicalPosition::new(x, y));
        }
        let _ = toast.emit("tts-toast-show", enabled);
        let _ = toast.show();
    }
    Ok(())
}

#[tauri::command]
fn stop_tts_playback(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    // Cancel chunked playback loop
    *state.tts_active.lock().unwrap() = false;
    // Stop current sink
    let mut sink_guard = state.tts_sink.lock().unwrap();
    if let Some(sink) = sink_guard.take() {
        sink.stop();
        println!("🔇 TTS playback stopped");
    }
    // Hide widget
    if let Some(w) = app.get_webview_window("tts-widget") {
        let _ = w.hide();
    }
    Ok(())
}

// --- Queue commands ---

#[tauri::command]
fn get_queue_count(state: State<'_, AppState>) -> Result<i64, String> {
    state.database.count_queue().map_err(|e| e.to_string())
}

#[tauri::command]
fn get_queue_items(state: State<'_, AppState>) -> Result<Vec<db::PendingQueueItem>, String> {
    state.database.load_queue().map_err(|e| e.to_string())
}

#[tauri::command]
async fn retry_pending_queue(state: State<'_, AppState>, app: AppHandle) -> Result<(), String> {
    let database = state.database.clone();
    let openai = state.openai_client.clone();
    let last_transcription = state.last_transcription.clone();
    let app_handle = app.clone();

    tokio::spawn(async move {
        process_retry_queue(database, openai, last_transcription, app_handle, true).await;
    });

    Ok(())
}

#[tauri::command]
async fn delete_single_queue_item(
    state: State<'_, AppState>,
    app: AppHandle,
    id: i64,
) -> Result<(), String> {
    if !state
        .database
        .claim_queue_item(id, true)
        .map_err(|e| e.to_string())?
    {
        return Err("Item em processamento.".into());
    }
    // Find item to delete its WAV file if it has one
    let items = state.database.load_queue().map_err(|e| e.to_string())?;
    if let Some(item) = items.iter().find(|i| i.id == id) {
        if let Some(ref path) = item.audio_path {
            queue::delete_wav_file(path);
        }
    }
    state
        .database
        .delete_queue_item(id)
        .map_err(|e| e.to_string())?;
    emit_queue_updated(&app, &state.database);
    Ok(())
}

#[tauri::command]
async fn retry_single_queue_item(
    state: State<'_, AppState>,
    app: AppHandle,
    id: i64,
) -> Result<(), String> {
    let database = state.database.clone();
    let openai = state.openai_client.clone();
    let last_transcription = state.last_transcription.clone();
    let app_handle = app.clone();

    let items = database.load_queue().map_err(|e| e.to_string())?;
    let item = items
        .into_iter()
        .find(|i| i.id == id)
        .ok_or_else(|| "Item não encontrado na fila".to_string())?;

    tokio::spawn(async move {
        if !database.claim_queue_item(item.id, true).unwrap_or(false) {
            return;
        }
        let result =
            process_single_queue_item(&item, &database, &openai, &last_transcription, &app_handle)
                .await;
        handle_queue_item_result(result, &item, &database, &app_handle);
    });

    Ok(())
}

#[tauri::command]
async fn play_queue_audio(state: State<'_, AppState>, audio_path: String) -> Result<(), String> {
    let path = std::fs::canonicalize(&audio_path).map_err(|_| "Áudio indisponível.".to_string())?;
    let root =
        std::fs::canonicalize(&state.queue_dir).map_err(|_| "Fila indisponível.".to_string())?;
    if !path.starts_with(&root)
        || path
            .extension()
            .is_none_or(|e| !e.eq_ignore_ascii_case("wav"))
    {
        return Err("Áudio fora da fila.".into());
    }
    let sink_state = state.tts_sink.clone();
    std::thread::spawn(move || {
        let result = (|| {
            let (_stream, handle) = rodio::OutputStream::try_default().map_err(|_| ())?;
            let file = std::fs::File::open(path).map_err(|_| ())?;
            let decoder = rodio::Decoder::new(std::io::BufReader::new(file)).map_err(|_| ())?;
            let sink = rodio::Sink::try_new(&handle).map_err(|_| ())?;
            sink.append(decoder);
            *sink_state.lock().map_err(|_| ())? = Some(sink);
            while sink_state
                .lock()
                .map_err(|_| ())?
                .as_ref()
                .is_some_and(|s| !s.empty())
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok::<_, ()>(())
        })();
        if result.is_err() {
            diagnostics::event("playback_failed", serde_json::json!({}));
        }
    });
    Ok(())
}

async fn process_single_queue_item(
    item: &db::PendingQueueItem,
    database: &Arc<db::Database>,
    openai: &Arc<openai::OpenAIClient>,
    last_transcription: &Arc<Mutex<Option<String>>>,
    app: &AppHandle,
) -> Result<(), String> {
    if let Some(text) = database
        .load_result_for_source(&format!("queue:{}", item.id))
        .map_err(|e| e.to_string())?
    {
        *last_transcription.lock().unwrap() = Some(text);
        return Ok(());
    }
    match item.mode.as_str() {
        "whisper-transcribe" => {
            let path = item
                .audio_path
                .as_ref()
                .ok_or("No audio path for whisper-transcribe item")?;
            let (audio, sample_rate) = queue::read_wav_to_f32_with_rate(path)?;
            let transcription_hints = build_transcription_hints(database);
            let text = openai
                .transcribe_audio(
                    audio,
                    sample_rate,
                    transcription_hints.0.as_deref(),
                    transcription_hints.1.as_deref(),
                )
                .await?;
            tlog!(
                "Queue retry: whisper-transcribe succeeded for id={}",
                item.id
            );
            database
                .save_result_once(
                    &format!("queue:{}", item.id),
                    &text,
                    None,
                    "whisper-1",
                    "transcription",
                )
                .map_err(|e| e.to_string())?;
            *last_transcription.lock().unwrap() = Some(text.clone());
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.emit("history-updated", ());
            }

            if let Some(window) = app.get_webview_window("main") {
                let _ = window.emit("response-ready", ());
            }
            Ok(())
        }
        "realtime-audio" => {
            let path = item
                .audio_path
                .as_ref()
                .ok_or("No audio path for realtime-audio item")?;
            let (audio, sample_rate) = queue::read_wav_to_f32_with_rate(path)?;
            let transcription_hints = build_transcription_hints(database);
            let text = openai
                .transcribe_audio(
                    audio,
                    sample_rate,
                    transcription_hints.0.as_deref(),
                    transcription_hints.1.as_deref(),
                )
                .await?;
            tlog!("Queue retry: realtime-audio succeeded for id={}", item.id);
            database
                .save_result_once(
                    &format!("queue:{}", item.id),
                    &text,
                    None,
                    "whisper-1",
                    "transcription",
                )
                .map_err(|e| e.to_string())?;
            *last_transcription.lock().unwrap() = Some(text.clone());
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.emit("history-updated", ());
            }

            if let Some(window) = app.get_webview_window("main") {
                let _ = window.emit("response-ready", ());
            }
            Ok(())
        }
        "whisper-prompt" | "text-prompt" | "realtime-prompt" => {
            let text = item
                .prompt_text
                .as_ref()
                .ok_or("No prompt text for queue item")?;
            let conv_history = get_conversation_history(database);
            let response = openai
                .send_prompt(text, &item.model, &conv_history, None)
                .await?;
            tlog!("Queue retry: {} succeeded for id={}", item.mode, item.id);
            let ts = now_ms();
            database
                .save_result_once(
                    &format!("queue:{}", item.id),
                    &response,
                    None,
                    &item.model,
                    "prompt",
                )
                .map_err(|e| e.to_string())?;
            let _ = database.append_conversation("user", text, ts - 1);
            let _ = database.append_conversation("assistant", &response, ts);
            *last_transcription.lock().unwrap() = Some(response.clone());
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.emit("history-updated", ());
            }

            if let Some(window) = app.get_webview_window("main") {
                let _ = window.emit("response-ready", ());
            }
            Ok(())
        }
        _ => Err(format!("Unknown queue mode: {}", item.mode)),
    }
}

/// Handle result of processing a queue item: delete on success, increment retry on failure
fn handle_queue_item_result(
    result: Result<(), String>,
    item: &db::PendingQueueItem,
    database: &Arc<db::Database>,
    app: &AppHandle,
) {
    match result {
        Ok(()) => {
            let _ = database.delete_queue_item(item.id);
            if let Some(ref path) = item.audio_path {
                queue::delete_wav_file(path);
            }
            let _ = app.emit("queue-item-completed", item.id);
            emit_queue_updated(app, database);
        }
        Err(e) => {
            eprintln!("❌ Queue retry failed for id={}: {}", item.id, e);
            let disable_auto_retry = should_disable_auto_retry(&e);
            let summarized_error = compact_queue_error(&e);
            let _ = database.record_queue_failure(item.id, &summarized_error, disable_auto_retry);
            emit_queue_updated(app, database);
        }
    }
}

/// Process pending queue items: retry API calls for each item
async fn process_retry_queue(
    database: Arc<db::Database>,
    openai: Arc<openai::OpenAIClient>,
    last_transcription: Arc<Mutex<Option<String>>>,
    app: AppHandle,
    include_blocked: bool,
) {
    let items = match database.load_queue() {
        Ok(items) => items,
        Err(e) => {
            eprintln!("❌ Failed to load queue: {}", e);
            return;
        }
    };

    if items.is_empty() {
        return;
    }

    let items: Vec<_> = if include_blocked {
        items
    } else {
        items
            .into_iter()
            .filter(|item| !item.auto_retry_disabled)
            .collect()
    };

    if items.is_empty() {
        return;
    }

    tlog!("Queue retry: processing {} items", items.len());

    for item in items {
        if !database
            .claim_queue_item(item.id, include_blocked)
            .unwrap_or(false)
        {
            continue;
        }
        let result =
            process_single_queue_item(&item, &database, &openai, &last_transcription, &app).await;
        handle_queue_item_result(result, &item, &database, &app);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Resolve app data directory (stable across launch methods)
    let app_data_dir = std::env::var_os("DICTA_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::data_local_dir()
                .unwrap_or_else(|| std::env::current_dir().expect("app data directory"))
                .join(if cfg!(debug_assertions) {
                    "com.dicta.app-dev"
                } else {
                    "com.dicta.app"
                })
        });
    std::fs::create_dir_all(&app_data_dir).ok();
    let _instance = match lifecycle::Instance::acquire(&app_data_dir) {
        Ok(lock) => lock,
        Err(_) => {
            eprintln!("Dicta: este perfil já está aberto ou indisponível.");
            return;
        }
    };

    diagnostics::init(app_data_dir.join("logs"));
    diagnostics::event(
        "startup",
        serde_json::json!({"version":env!("CARGO_PKG_VERSION")}),
    );
    // Load .env file: try app data dir first, then current dir (for dev mode)
    let env_path = app_data_dir.join(".env");
    if env_path.exists() {
        dotenv::from_path(&env_path).ok();
    } else {
        if std::env::var("OPENAI_API_KEY").is_err() {
            if let Some(root) = dirs::data_local_dir() {
                dotenv::from_path(root.join("com.dicta.app/.env")).ok();
            }
        }
        if std::env::var("OPENAI_API_KEY").is_err() {
            dotenv::dotenv().ok();
        }
    }

    // Load OpenAI API key from environment
    let api_key = match std::env::var("OPENAI_API_KEY") {
        Ok(key) => key,
        Err(_) => {
            eprintln!(
                "❌ OPENAI_API_KEY not found. Place a .env file in: {}",
                app_data_dir.display()
            );
            // Show a native error dialog since there's no console in release mode
            #[cfg(not(debug_assertions))]
            {
                use std::process::Command;
                let msg = format!(
                    "OPENAI_API_KEY não encontrada.\n\nCrie um arquivo .env com sua chave em:\n{}\n\nConteúdo do arquivo:\nOPENAI_API_KEY=sk-...",
                    app_data_dir.display()
                );
                let _ = Command::new("powershell")
                    .args(["-Command", &format!("Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.MessageBox]::Show('{}', 'Dicta - Erro', 'OK', 'Error')", msg.replace('\'', "''"))])
                    .output();
            }
            std::process::exit(1);
        }
    };

    // Initialize database in app data directory
    let db_path = app_data_dir.join("dicta.db");
    println!("📁 Database: {}", db_path.display());

    let database = Arc::new(db::Database::new(db_path).expect("Failed to initialize database"));

    // Load TTS preference from DB
    let tts_default = database
        .load_setting("tts_enabled")
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false);
    let _ = database.recover_interrupted_sessions();

    // Playback threads open the current device on demand, including after resume.
    let tts_stream_handle_val = None;

    // Initialize queue directory
    let queue_dir = app_data_dir.join("queue");
    std::fs::create_dir_all(&queue_dir).ok();
    println!("📁 Queue directory: {}", queue_dir.display());

    // Initialize app state
    let session_dir = app_data_dir.join("sessions");
    std::fs::create_dir_all(&session_dir).ok();
    let app_state = AppState {
        openai_client: Arc::new(openai::OpenAIClient::new(api_key)),
        database,
        sessions: Arc::new(dictation::Manager::new()),
        session_dir,
        next_target: Arc::new(Mutex::new(None)),
        is_recording: Arc::new(Mutex::new(false)),
        prompt_mode: Arc::new(Mutex::new(None)),
        last_transcription: Arc::new(Mutex::new(None)),
        tts_enabled: Arc::new(Mutex::new(tts_default)),
        tts_sink: Arc::new(Mutex::new(None)),
        tts_stream_handle: Arc::new(Mutex::new(tts_stream_handle_val)),
        tts_active: Arc::new(Mutex::new(false)),
        queue_dir,
        shortcut_errors: Arc::new(Mutex::new(Vec::new())),
    };

    // Debounce: prevent multiple triggers when keys are held down
    let last_recording_trigger = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(1)));
    let last_recording_trigger_clone = last_recording_trigger.clone();

    let last_paste_trigger = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(1)));
    let last_paste_trigger_clone = last_paste_trigger.clone();

    tauri::Builder::default()
        .manage(app_state)
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    // Only handle key press events, ignore key release
                    let event_str = format!("{:?}", event);
                    if !event_str.contains("Pressed") {
                        return; // Ignore Released events
                    }

                    if let Some(state) = app.try_state::<AppState>() {
                        if should_block_hotkeys_for_game_mode(&state.database) {
                            return;
                        }
                    }

                    if let Some(state)=app.try_state::<AppState>(){
                        if !*state.is_recording.lock().unwrap() {
                            *state.next_target.lock().unwrap()=delivery::capture_target();
                        }
                    }
                    diagnostics::event("hotkey",serde_json::json!({"shortcut":format!("{:?}",shortcut)}));
                    // Check which shortcut was pressed
                    let shortcut_str = format!("{:?}", shortcut);

                    if let Some(state)=app.try_state::<AppState>(){if shortcut_str.contains("Space") && state.sessions.busy() && !*state.is_recording.lock().unwrap(){return;}}
                    #[cfg(debug_assertions)]
                    if std::env::var_os("DICTA_TEST_HOTKEYS").is_some() && shortcut_str.contains("Space") {
                        if let Some(state)=app.try_state::<AppState>(){if !*state.is_recording.lock().unwrap(){let _=state.database.save_setting("selected_prompt_model","transcribe-only");}}
                        if let Some(widget)=app.get_webview_window("recording-widget"){let _=widget.show();}
                        toggle_recording_from_hotkey(app.clone());
                        return;
                    }

                    if shortcut_str.contains("Space") && shortcut_str.contains("CONTROL") && shortcut_str.contains("SHIFT") {
                        // Ctrl+Shift+Space: Toggle recording with selected prompt model
                        let mut last = last_recording_trigger_clone.lock().unwrap();
                        let now = Instant::now();

                        if now.duration_since(*last) > Duration::from_millis(100) {
                            *last = now;
                            println!("🔥 Hotkey pressed: Ctrl+Shift+Space (Prompt mode)");

                            if let Some(state) = app.try_state::<AppState>() {
                                let is_recording = *state.is_recording.lock().unwrap();

                                if !is_recording {
                                    // Ctrl+Shift+Space: use the user's chosen prompt model (separate key)
                                    // This is the model the user picked in the combo box for prompt sessions
                                    let model = load_user_prompt_model(&state.database);

                                    // Save as current session model
                                    let _ = state.database.save_setting("selected_prompt_model", &model);

                                    *state.prompt_mode.lock().unwrap() = Some(model.clone());
                                    println!("🤖 Prompt mode enabled: {} (saved to DB)", model);

                                    // Show widget
                                    if let Some(widget) = app.get_webview_window("recording-widget") {
                                        if let Ok(monitor) = widget.current_monitor() {
                                            if let Some(monitor) = monitor {
                                                let screen_size = monitor.size();
                                                let widget_width = 155;
                                                let widget_height = 200; // Height increased for combo box
                                                let bottom_margin = 200; // More space from taskbar

                                                let x = (screen_size.width as i32 - widget_width) / 2;
                                                let y = screen_size.height as i32 - widget_height - bottom_margin;

                                                let _ = widget.set_position(PhysicalPosition::new(x, y));
                                            }
                                        }
                                        let _ = widget.show();
                                        // Tell widget which model is active
                                        let _ = widget.emit("model-selected", model.clone());
                                    }
                                } else {
                                    // Stopping recording - DON'T clear prompt_mode here
                                    // It will be cleared in stop_realtime_recording after being used
                                    println!("🛑 [Ctrl+Shift+Space] Stopping - prompt_mode will be used in stop handler");

                                    if let Some(widget) = app.get_webview_window("recording-widget") {
                                        let _ = widget.hide();
                                    }
                                }
                            }

                            // Emit event to frontend
                            toggle_recording_from_hotkey(app.clone());
                        } else {
                            println!("⏭️ Ctrl+Shift+Space ignored (debounce)");
                        }
                    } else if shortcut_str.contains("Space") && shortcut_str.contains("CONTROL") && shortcut_str.contains("ALT") {
                        // Ctrl+Alt+Space: Toggle recording with GPT-5.4 prompt mode
                        let mut last = last_recording_trigger_clone.lock().unwrap();
                        let now = Instant::now();

                        if now.duration_since(*last) > Duration::from_millis(100) {
                            *last = now;
                            println!("🔥 Hotkey pressed: Ctrl+Alt+Space (GPT-5.4 mode)");

                            if let Some(state) = app.try_state::<AppState>() {
                                let is_recording = *state.is_recording.lock().unwrap();

                                if !is_recording {
                                    // Set prompt mode to gpt-5.4 and save to database
                                    let _ = state.database.save_setting("selected_prompt_model", "gpt-5.4");
                                    *state.prompt_mode.lock().unwrap() = Some("gpt-5.4".to_string());
                                    println!("🤖 Prompt mode enabled: gpt-5.4 (saved to DB)");

                                    // Show widget
                                    if let Some(widget) = app.get_webview_window("recording-widget") {
                                        if let Ok(monitor) = widget.current_monitor() {
                                            if let Some(monitor) = monitor {
                                                let screen_size = monitor.size();
                                                let widget_width = 155;
                                                let widget_height = 120; // Height increased for combo box
                                                let bottom_margin = 200; // More space from taskbar

                                                let x = (screen_size.width as i32 - widget_width) / 2;
                                                let y = screen_size.height as i32 - widget_height - bottom_margin;

                                                let _ = widget.set_position(PhysicalPosition::new(x, y));
                                            }
                                        }
                                        let _ = widget.show();
                                        // Tell widget which model is active
                                        let _ = widget.emit("model-selected", "gpt-5.4".to_string());
                                    }
                                } else {
                                    // Stopping recording - DON'T clear prompt_mode here
                                    // It will be cleared in stop_realtime_recording after being used
                                    println!("🛑 [Ctrl+Alt+Space] Stopping - prompt_mode (gpt-5.4) will be used in stop handler");

                                    if let Some(widget) = app.get_webview_window("recording-widget") {
                                        let _ = widget.hide();
                                    }
                                }
                            }

                            // Emit event to frontend
                            toggle_recording_from_hotkey(app.clone());
                        } else {
                            println!("⏭️ Ctrl+Alt+Space ignored (debounce)");
                        }
                    } else if shortcut_str.contains("Space") {
                        // Ctrl+Space: Toggle recording (with minimal debounce for safety)
                        let mut last = last_recording_trigger_clone.lock().unwrap();
                        let now = Instant::now();

                        // Only trigger if 100ms have passed (minimal debounce, since we filter Pressed)
                        if now.duration_since(*last) > Duration::from_millis(100) {
                            *last = now;
                            println!("🔥 Hotkey pressed: Ctrl+Space");

                            // Show/hide widget based on recording state
                            if let Some(state) = app.try_state::<AppState>() {
                                let is_recording = *state.is_recording.lock().unwrap();

                                if !is_recording {
                                    println!("📝 Ctrl+Space starting - forcing transcribe-only mode");
                                    let _ = state.database.save_setting("selected_prompt_model", "transcribe-only");
                                    *state.prompt_mode.lock().unwrap() = None;
                                    let widget_model = "transcribe-only".to_string();

                                    // Starting recording - show widget
                                    if let Some(widget) = app.get_webview_window("recording-widget") {
                                        // Position widget at bottom-center of screen
                                        if let Ok(monitor) = widget.current_monitor() {
                                            if let Some(monitor) = monitor {
                                                let screen_size = monitor.size();
                                                let widget_width = 155;
                                                let widget_height = 120; // Height increased for combo box
                                                let bottom_margin = 200; // More space from taskbar

                                                let x = (screen_size.width as i32 - widget_width) / 2;
                                                let y = screen_size.height as i32 - widget_height - bottom_margin;

                                                let _ = widget.set_position(PhysicalPosition::new(x, y));
                                            }
                                        }
                                        let _ = widget.show();
                                        // Tell widget which model is active
                                        let _ = widget.emit("model-selected", widget_model);
                                    }
                                } else {
                                    // Stopping recording with Ctrl+Space
                                    let current_prompt_mode = state.prompt_mode.lock().unwrap().clone();
                                    println!("🛑 [Ctrl+Space] Stopping recording - prompt_mode = {:?}", current_prompt_mode);
                                    println!("📌 Prompt mode will be preserved for stop_realtime_recording");

                                    // Hide widget
                                    if let Some(widget) = app.get_webview_window("recording-widget") {
                                        let _ = widget.hide();
                                    }
                                }
                            }

                            // Emit event to frontend
                            toggle_recording_from_hotkey(app.clone());
                        } else {
                            println!("⏭️ Ctrl+Space ignored (debounce - too fast)");
                        }
                    } else if shortcut_str.contains("KeyB") && shortcut_str.contains("CONTROL") {
                        // Ctrl+B: Open prompt input window
                        tlog!("🔥 Hotkey pressed: Ctrl+B");
                        if let Some(prompt_window) = app.get_webview_window("prompt-input") {
                            if let Ok(monitor) = prompt_window.current_monitor() {
                                if let Some(monitor) = monitor {
                                    let screen_size = monitor.size();
                                    let win_width = 400i32;
                                    let win_height = 160i32;
                                    let x = (screen_size.width as i32 - win_width) / 2;
                                    let y = screen_size.height as i32 - win_height - 200;
                                    let _ = prompt_window.set_position(PhysicalPosition::new(x, y));
                                }
                            }
                            let _ = prompt_window.show();
                            let _ = prompt_window.set_focus();
                        }
                    } else if shortcut_str.contains("KeyS") && shortcut_str.contains("CONTROL") && shortcut_str.contains("ALT") {
                        // Ctrl+Alt+S: Toggle TTS
                        tlog!("🔥 Hotkey pressed: Ctrl+Alt+S (Toggle TTS)");
                        if let Some(state) = app.try_state::<AppState>() {
                            let new_val = {
                                let mut enabled = state.tts_enabled.lock().unwrap();
                                *enabled = !*enabled;
                                *enabled
                            };
                            let _ = state.database.save_setting("tts_enabled", if new_val { "true" } else { "false" });
                            println!("🔊 TTS toggled: {}", if new_val { "ON" } else { "OFF" });
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.emit("tts-toggled", new_val);
                            }
                            // Also emit to recording widget
                            if let Some(widget) = app.get_webview_window("recording-widget") {
                                let _ = widget.emit("tts-toggled", new_val);
                            }
                            // Show toast
                            if let Some(toast) = app.get_webview_window("tts-toast") {
                                if let Ok(Some(monitor)) = app.primary_monitor() {
                                    let screen = monitor.size();
                                    let x = (screen.width as i32 - 140) / 2;
                                    let y = screen.height as i32 - 40 - 300;
                                    let _ = toast.set_position(PhysicalPosition::new(x, y));
                                }
                                let _ = toast.emit("tts-toast-show", new_val);
                                let _ = toast.show();
                            }
                        }
                    } else if shortcut_str.contains("KeyS") && shortcut_str.contains("ALT") && shortcut_str.contains("SHIFT") {
                        // Alt+Shift+S: Stop TTS playback or read last message
                        tlog!("🔥 Hotkey pressed: Alt+Shift+S (TTS action)");
                        if let Some(state) = app.try_state::<AppState>() {
                            let is_active = *state.tts_active.lock().unwrap();

                            if is_active {
                                // Stop current chunked playback
                                *state.tts_active.lock().unwrap() = false;
                                {
                                    let mut sink_guard = state.tts_sink.lock().unwrap();
                                    if let Some(sink) = sink_guard.take() {
                                        sink.stop();
                                    }
                                }
                                if let Some(w) = app.get_webview_window("tts-widget") {
                                    let _ = w.hide();
                                }
                                println!("🔇 TTS playback stopped via Alt+Shift+S");
                            } else {
                                // Read last message aloud (chunked)
                                let last_text = state.last_transcription.lock().unwrap().clone();
                                if let Some(text) = last_text {
                                    let openai = state.openai_client.clone();
                                    let tts_sink = state.tts_sink.clone();
                                    let tts_handle = state.tts_stream_handle.clone();
                                    let tts_active = state.tts_active.clone();
                                    let app_clone = app.clone();
                                    tauri::async_runtime::spawn(play_tts_chunked(
                                        app_clone, text, openai, tts_sink, tts_handle, tts_active,
                                    ));
                                } else {
                                    println!("⚠️ No message to read aloud");
                                }
                            }
                        }
                    } else if shortcut_str.contains("KeyZ") {
                        let mut last=last_paste_trigger_clone.lock().unwrap();
                        if last.elapsed()<Duration::from_millis(1000){return;}*last=Instant::now();drop(last);
                        if let Some(state)=app.try_state::<AppState>() {
                            if let Ok(Some(entry))=state.database.load_latest_transcription(){
                                let target=delivery::capture_target();let app=app.clone();
                                std::thread::spawn(move||delivery::deliver(&app,&entry.text,target));
                            }
                        }
                    }

                })
                .build()
        )
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![
            start_recording_audio,
            stop_recording_audio,
            cancel_recording,
            get_last_transcription,
            get_transcription_history,
            copy_to_clipboard,
            get_session_state,get_segment_config,set_segment_config,get_asr_model,set_asr_model,
            get_recoverable_sessions,retry_session,discard_session,paste_last_transcription,get_build_info,get_shortcut_errors,recover_runtime,get_web_search_enabled,set_web_search_enabled,
            set_use_realtime,
            get_use_realtime,
            list_microphones,
            set_selected_microphone,
            get_selected_microphone,
            set_selected_prompt_model,
            get_selected_prompt_model,
            get_user_prompt_model,
            get_current_recording_mode,
            get_transcription_languages,
            set_transcription_languages,
            get_game_mode_enabled,
            set_game_mode_enabled,
            get_game_mode_processes,
            set_game_mode_processes,
            send_text_prompt,
            get_statistics,
            get_tts_enabled,
            set_tts_enabled,
            stop_tts_playback,
            get_queue_count,
            get_queue_items,
            retry_pending_queue,
            delete_single_queue_item,
            retry_single_queue_item,
            play_queue_audio
        ])
        .setup(|app| {
            lifecycle::install(app.handle())?;
            if cfg!(debug_assertions) && std::env::var_os("DICTA_SHOW_MAIN").is_some(){if let Some(w)=app.get_webview_window("main"){let _=w.set_title("Dicta — teste isolado");let _=w.show();}}
            if let Some(w)=app.get_webview_window("recording-widget"){let _=w.set_focusable(false);}
            // Create tray menu
            let show_item = MenuItem::with_id(app, "show", "Abrir Dicta", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
            let recover_item=MenuItem::with_id(app,"recover","Recuperar sessão e colagem",true,None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &recover_item, &quit_item])?;

            // Build system tray
            let _tray = TrayIconBuilder::with_id("main-tray")
                .tooltip("Dicta - Voice Transcription")
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "recover" => {
                        let state=app.state::<AppState>();delivery::invalidate();state.sessions.interrupt(app,false);
                        let _=system_audio::unmute_system_audio();let _=app.emit("runtime-resumed",());
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, .. } = event {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            // Handle window close event - minimize to tray instead of closing
            if let Some(window) = app.get_webview_window("main") {
                let window_clone = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        // Prevent default close behavior
                        api.prevent_close();
                        // Hide window instead
                        let _ = window_clone.hide();
                    }
                });
            }

            // Clear any stale mute from a previous crash
            let _ = system_audio::unmute_system_audio();

            // Register global hotkeys (use .ok() to avoid crash if shortcut is already taken)
            let shortcuts = [
                Shortcut::new(Some(Modifiers::CONTROL), Code::Space),
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space),
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space),
                Shortcut::new(Some(Modifiers::ALT | Modifiers::SHIFT), Code::KeyZ),
                Shortcut::new(Some(Modifiers::CONTROL), Code::KeyB),
                Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyS),
                Shortcut::new(Some(Modifiers::ALT | Modifiers::SHIFT), Code::KeyS),
            ];
            let shortcuts=if cfg!(debug_assertions) && std::env::var_os("DICTA_TEST_HOTKEYS").is_some(){vec![Shortcut::new(Some(Modifiers::CONTROL|Modifiers::ALT|Modifiers::SHIFT),Code::Space)]}else{shortcuts.to_vec()};
            for shortcut in &shortcuts {
                if std::env::var_os("DICTA_DISABLE_HOTKEYS").is_some(){continue;}
                if let Err(e) = app.global_shortcut().register(*shortcut) {
                    diagnostics::event("shortcut_registration_failed",serde_json::json!({"shortcut":format!("{:?}",shortcut),"error":e.to_string()}));
                    app.state::<AppState>().shortcut_errors.lock().unwrap().push(shortcut.to_string());
                }
            }

            // Start background queue retry loop (every 30 seconds)
            {
                let state = app.state::<AppState>();
                let db_for_queue = state.database.clone();
                let openai_for_queue = state.openai_client.clone();
                let last_tx_for_queue = state.last_transcription.clone();
                let app_for_queue = app.handle().clone();

                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(Duration::from_secs(30)).await;
                        process_retry_queue(
                            db_for_queue.clone(),
                            openai_for_queue.clone(),
                            last_tx_for_queue.clone(),
                            app_for_queue.clone(),
                            false,
                        ).await;
                    }
                });
            }

            println!("✅ Dicta is running!");
            println!("📌 Press Ctrl+Space to start/stop recording");
            println!("📌 Press Ctrl+Shift+Space for the saved prompt model");
            println!("📌 Press Ctrl+Alt+Space for GPT-5.4 prompt mode");
            println!("📌 Press Alt+Shift+Z to paste last transcription");
            println!("📌 Press Ctrl+B to open prompt input window");
            println!("📌 Press Ctrl+Alt+S to toggle TTS");
            println!("📌 Press Ctrl+S to stop TTS / read last message");
            println!("🔑 OpenAI API key loaded");

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
