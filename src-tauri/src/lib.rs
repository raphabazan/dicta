mod audio;
mod openai;
mod realtime;
mod db;
mod system_audio;

use tauri::{Emitter, Manager, State, AppHandle, PhysicalPosition};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, GlobalShortcutExt};
use tauri_plugin_clipboard_manager::ClipboardExt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use enigo::{Enigo, Key, Keyboard, Settings};

fn ts() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
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

fn auto_paste_text(app: &AppHandle, text: &str) -> Result<(), String> {
    println!("🔄 Auto-pasting text...");

    // 1. Read current clipboard (with retry)
    let original_clipboard = {
        let mut attempts = 0;
        loop {
            match app.clipboard().read_text() {
                Ok(content) => break content,
                Err(e) => {
                    attempts += 1;
                    if attempts >= 3 {
                        println!("⚠️ Failed to read clipboard after 3 attempts, using empty string");
                        break String::new();
                    }
                    println!("⚠️ Clipboard read attempt {} failed: {}, retrying...", attempts, e);
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        }
    };

    // Safely truncate clipboard preview (handle UTF-8 char boundaries)
    let clipboard_preview = if original_clipboard.len() > 30 {
        original_clipboard.chars().take(30).collect::<String>() + "..."
    } else {
        original_clipboard.clone()
    };
    println!("💾 Saved original clipboard: '{}'", clipboard_preview);

    // 2. Write transcribed text to clipboard (with retry)
    {
        let mut attempts = 0;
        loop {
            match app.clipboard().write_text(text) {
                Ok(_) => {
                    println!("📋 Transcription written to clipboard");
                    break;
                }
                Err(e) => {
                    attempts += 1;
                    if attempts >= 3 {
                        return Err(format!("Failed to write to clipboard after 3 attempts: {}", e));
                    }
                    println!("⚠️ Clipboard write attempt {} failed: {}, retrying...", attempts, e);
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        }
    }

    // 3. Wait for:
    // - Clipboard to update
    // - User to release Alt+Shift+Z keys (CRITICAL!)
    // - Focus to return to the target application
    // IMPORTANT: We need to wait long enough for the user to release the modifier keys
    // (Alt+Shift+Z) otherwise the Ctrl+V simulation won't work because the OS sees
    // conflicting modifier keys pressed at the same time
    println!("⏳ Waiting 1000ms for keys to be released...");
    std::thread::sleep(Duration::from_millis(1000));

    // 4. Simulate Ctrl+V
    let mut enigo = Enigo::new(&Settings::default())
        .map_err(|e| format!("Failed to create Enigo: {:?}", e))?;

    enigo.key(Key::Control, enigo::Direction::Press)
        .map_err(|e| format!("Failed to press Ctrl: {:?}", e))?;
    enigo.key(Key::Unicode('v'), enigo::Direction::Click)
        .map_err(|e| format!("Failed to press V: {:?}", e))?;
    enigo.key(Key::Control, enigo::Direction::Release)
        .map_err(|e| format!("Failed to release Ctrl: {:?}", e))?;

    println!("⌨️ Simulated Ctrl+V");

    // 5. Wait for paste to complete and check if clipboard changed
    std::thread::sleep(Duration::from_millis(150));

    // 6. Check if clipboard still has our transcribed text
    let current_clipboard = app.clipboard().read_text()
        .map_err(|e| format!("Failed to read clipboard after paste: {}", e))?;

    // If clipboard still has our text, the paste likely succeeded
    // Only restore if clipboard was consumed (changed)
    if current_clipboard == text {
        println!("📋 Clipboard unchanged - paste likely succeeded, restoring original");
        app.clipboard().write_text(&original_clipboard)
            .map_err(|e| format!("Failed to restore clipboard: {}", e))?;
        println!("♻️ Restored original clipboard");
    } else {
        println!("🔄 Clipboard was consumed - paste succeeded, keeping current state");
    }

    Ok(())
}

struct AppState {
    audio_recorder: Arc<Mutex<audio::AudioRecorder>>,
    openai_client: Arc<openai::OpenAIClient>,
    realtime_client: Arc<realtime::RealtimeClient>,
    database: Arc<db::Database>,
    is_recording: Arc<Mutex<bool>>,
    use_realtime: Arc<Mutex<bool>>, // Track which API to use
    prompt_mode: Arc<Mutex<Option<String>>>, // Track prompt mode: None, Some("gpt-4o-mini"), or Some("gpt-4o")
    current_session_transcript: Arc<Mutex<String>>, // Accumulate transcript for current session
    last_transcription: Arc<Mutex<Option<String>>>,
    paste_in_progress: Arc<Mutex<bool>>,
    recording_start_time: Arc<Mutex<Option<Instant>>>,
    speech_active: Arc<Mutex<bool>>, // Track if speech is currently being detected
    last_speech_end: Arc<Mutex<Option<Instant>>>, // Track when last speech ended
    last_transcription_time: Arc<Mutex<Option<Instant>>>, // Track when last transcription.completed arrived
    tts_enabled: Arc<Mutex<bool>>,
    tts_sink: Arc<Mutex<Option<rodio::Sink>>>,
    tts_stream_handle: Arc<Mutex<Option<rodio::OutputStreamHandle>>>,
}

#[tauri::command]
async fn cancel_recording(state: State<'_, AppState>) -> Result<String, String> {
    let mut is_recording = state.is_recording.lock().unwrap();
    if !*is_recording {
        return Err("Not recording".to_string());
    }

    println!("❌ Cancelling recording...");

    // For Whisper mode, just stop recording without processing
    let recorder = state.audio_recorder.lock().unwrap();
    let _ = recorder.stop_recording(); // Discard audio data
    *is_recording = false;

    // Restore system audio
    if let Err(e) = system_audio::unmute_system_audio() {
        eprintln!("⚠️ Failed to unmute system audio: {}", e);
    }

    Ok("Recording cancelled".to_string())
}

#[tauri::command]
async fn start_recording_audio(state: State<'_, AppState>, app: AppHandle) -> Result<String, String> {
    let mut is_recording = state.is_recording.lock().unwrap();
    if *is_recording {
        return Err("Already recording".to_string());
    }

    println!("🎤 Starting audio recording...");

    // Set recording start time
    *state.recording_start_time.lock().unwrap() = Some(Instant::now());

    // Get selected microphone from settings
    let selected_mic = state.database.load_setting("selected_microphone")
        .ok()
        .flatten();

    let recorder = state.audio_recorder.lock().unwrap();
    recorder.start_recording(selected_mic)?;
    *is_recording = true;

    // Mute system audio while recording
    if let Err(e) = system_audio::mute_system_audio() {
        eprintln!("⚠️ Failed to mute system audio: {}", e);
    }

    // Spawn timer task for Whisper mode
    let is_recording_flag = state.is_recording.clone();
    let recording_start = state.recording_start_time.clone();
    let app_clone = app.clone();

    tokio::spawn(async move {
        let mut warning_shown = false;
        let mut auto_stop_triggered = false;

        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            if !*is_recording_flag.lock().unwrap() {
                break;
            }

            if let Some(start_time) = *recording_start.lock().unwrap() {
                let elapsed = start_time.elapsed();

                // Show warning at 5 minutes
                if elapsed >= Duration::from_secs(5 * 60) && !warning_shown {
                    warning_shown = true;
                    println!("⚠️ [WHISPER] 5 seconds elapsed, showing warning...");
                    println!("⚠️ [WHISPER] Elapsed time: {:?}", elapsed);

                    if let Some(warning) = app_clone.get_webview_window("warning-widget") {
                        println!("⚠️ [WHISPER] Found warning widget");

                        if let Some(widget) = app_clone.get_webview_window("recording-widget") {
                            println!("⚠️ [WHISPER] Found recording widget");
                            if let Ok(widget_pos) = widget.outer_position() {
                                let warning_x = widget_pos.x - 77;
                                let warning_y = widget_pos.y - 70;
                                println!("⚠️ [WHISPER] Positioning warning at x:{}, y:{}", warning_x, warning_y);
                                match warning.set_position(PhysicalPosition::new(warning_x, warning_y)) {
                                    Ok(_) => println!("⚠️ [WHISPER] ✅ Position set successfully"),
                                    Err(e) => println!("⚠️ [WHISPER] ❌ Failed to set position: {}", e),
                                }
                            }
                        } else {
                            println!("⚠️ [WHISPER] ❌ Recording widget not found for positioning");
                        }

                        match warning.show() {
                            Ok(_) => {
                                println!("⚠️ [WHISPER] ✅ Warning shown successfully");

                                // Auto-hide warning after 4 seconds
                                let warning_clone = warning.clone();
                                tokio::spawn(async move {
                                    tokio::time::sleep(tokio::time::Duration::from_secs(4)).await;
                                    println!("⚠️ [WHISPER] Auto-hiding warning after 4 seconds");
                                    match warning_clone.hide() {
                                        Ok(_) => println!("⚠️ [WHISPER] ✅ Warning auto-hidden successfully"),
                                        Err(e) => println!("⚠️ [WHISPER] ❌ Failed to auto-hide warning: {}", e),
                                    }
                                });
                            },
                            Err(e) => println!("⚠️ [WHISPER] ❌ Failed to show warning: {}", e),
                        }
                    } else {
                        println!("⚠️ [WHISPER] ❌ Warning widget not found!");
                    }
                }

                // Auto-stop at 6 minutes
                if elapsed >= Duration::from_secs(6 * 60) && !auto_stop_triggered {
                    auto_stop_triggered = true;
                    println!("⏰ [WHISPER] 6 minutes limit reached, auto-stopping...");
                    println!("⏰ [WHISPER] Elapsed time: {:?}", elapsed);

                    // DON'T set is_recording = false here - let the frontend's stopRecording() do it

                    if let Some(window) = app_clone.get_webview_window("main") {
                        println!("⏰ [WHISPER] Found main window, emitting widget-stop-recording event");
                        match window.emit("widget-stop-recording", ()) {
                            Ok(_) => println!("⏰ [WHISPER] ✅ Event emitted successfully"),
                            Err(e) => println!("⏰ [WHISPER] ❌ Failed to emit event: {}", e),
                        }
                    } else {
                        println!("⏰ [WHISPER] ❌ Main window not found!");
                    }

                    // Hide recording widget
                    if let Some(widget) = app_clone.get_webview_window("recording-widget") {
                        println!("⏰ [WHISPER] Found recording widget, hiding it");
                        match widget.hide() {
                            Ok(_) => println!("⏰ [WHISPER] ✅ Widget hidden successfully"),
                            Err(e) => println!("⏰ [WHISPER] ❌ Failed to hide widget: {}", e),
                        }
                    } else {
                        println!("⏰ [WHISPER] ❌ Recording widget not found!");
                    }

                    // DON'T break - let the loop continue until frontend calls stop
                }
            }
        }
    });

    Ok("Recording started".to_string())
}

#[tauri::command]
async fn stop_recording_audio(state: State<'_, AppState>, app: tauri::AppHandle) -> Result<String, String> {
    let mut is_recording = state.is_recording.lock().unwrap();
    if !*is_recording {
        return Err("Not recording".to_string());
    }

    println!("⏹️ Stopping audio recording...");
    let recorder = state.audio_recorder.lock().unwrap();
    let audio_data = recorder.stop_recording();
    *is_recording = false;

    // Restore system audio
    if let Err(e) = system_audio::unmute_system_audio() {
        eprintln!("⚠️ Failed to unmute system audio: {}", e);
    }

    // Capture recording duration for stats
    let duration_ms = state.recording_start_time.lock().unwrap()
        .map(|start| start.elapsed().as_millis() as i64);

    if audio_data.is_empty() {
        return Err("No audio recorded".to_string());
    }

    // Check if we're in prompt mode
    let prompt_mode = state.prompt_mode.lock().unwrap().clone();

    // Load conversation history before spawning (inactivity check happens here)
    let conv_history = get_conversation_history(&state.database);

    // Transcribe (without post-processing for speed)
    let openai = state.openai_client.clone();
    let last_transcription = state.last_transcription.clone();
    let database = state.database.clone();
    let app_handle = app.clone();
    let tts_enabled = state.tts_enabled.clone();
    let tts_sink = state.tts_sink.clone();
    let tts_stream_handle = state.tts_stream_handle.clone();
    let openai_for_tts = state.openai_client.clone();
    tokio::spawn(async move {
        match openai.transcribe_audio(audio_data, 48000).await {
            Ok(transcribed_text) => {
                println!("✨ Transcribed: {}", transcribed_text);

                // Check if we're in prompt mode
                if let Some(model) = prompt_mode {
                    println!("🤖 Prompt mode active with model: {}", model);

                    // Send transcribed text as prompt to GPT
                    match openai.send_prompt(&transcribed_text, &model, &conv_history, None).await {
                        Ok(gpt_response) => {
                            println!("✨ GPT Response: {}", gpt_response);

                            // Save GPT response as last transcription
                            *last_transcription.lock().unwrap() = Some(gpt_response.clone());

                            // Save to database (save the GPT response, not the prompt)
                            let timestamp = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_millis() as i64;
                            let cost = estimate_cost_cents(&model, duration_ms, &gpt_response);

                            if let Err(e) = database.save_transcription(&gpt_response, timestamp, duration_ms, Some(&model), Some(cost), Some("prompt")) {
                                eprintln!("❌ Failed to save to database: {}", e);
                            }

                            // Save to conversation history
                            let _ = database.append_conversation("user", &transcribed_text, timestamp - 1);
                            let _ = database.append_conversation("assistant", &gpt_response, timestamp);

                            // Notify frontend
                            if let Some(window) = app_handle.get_webview_window("main") {
                                let _ = window.emit("history-updated", ());
                            }

                            // Auto-paste GPT response
                            match auto_paste_text(&app_handle, &gpt_response) {
                                Ok(_) => println!("✅ GPT response auto-pasted successfully"),
                                Err(e) => {
                                    eprintln!("⚠️ Auto-paste failed: {}", e);
                                    if let Some(window) = app_handle.get_webview_window("main") {
                                        let _ = window.emit("paste-failed", ());
                                    }
                                }
                            }

                            // Notification sound
                            if let Some(window) = app_handle.get_webview_window("main") {
                                let _ = window.emit("response-ready", ());
                            }

                            // TTS
                            if *tts_enabled.lock().unwrap() {
                                let openai_tts = openai_for_tts.clone();
                                let tts_sink2 = tts_sink.clone();
                                let tts_handle2 = tts_stream_handle.clone();
                                let tts_text = gpt_response.clone();
                                tokio::spawn(async move {
                                    if let Ok(audio) = openai_tts.speak_text(&tts_text).await {
                                        {
                                            let mut sg = tts_sink2.lock().unwrap();
                                            if let Some(s) = sg.take() { s.stop(); }
                                        }
                                        let hg = tts_handle2.lock().unwrap();
                                        if let Some(h) = hg.as_ref() {
                                            if let Ok(src) = rodio::Decoder::new(std::io::Cursor::new(audio)) {
                                                if let Ok(sink) = rodio::Sink::try_new(h) {
                                                    sink.append(src);
                                                    *tts_sink2.lock().unwrap() = Some(sink);
                                                    println!("🔊 TTS playback started");
                                                }
                                            }
                                        }
                                    }
                                });
                            }
                        }
                        Err(e) => eprintln!("❌ GPT prompt error: {}", e),
                    }
                } else {
                    // Normal transcription mode
                    // Save last transcription
                    *last_transcription.lock().unwrap() = Some(transcribed_text.clone());

                    // Save to database
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as i64;
                    let cost = estimate_cost_cents("whisper", duration_ms, &transcribed_text);

                    if let Err(e) = database.save_transcription(&transcribed_text, timestamp, duration_ms, Some("whisper"), Some(cost), Some("transcription")) {
                        eprintln!("❌ Failed to save to database: {}", e);
                    }

                    // Notify frontend that history was updated
                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.emit("history-updated", ());
                    }

                    // Auto-paste: save clipboard, paste, restore
                    match auto_paste_text(&app_handle, &transcribed_text) {
                        Ok(_) => println!("✅ Text auto-pasted successfully"),
                        Err(e) => {
                            eprintln!("⚠️ Auto-paste failed: {}", e);
                            // Notify frontend of failure
                            if let Some(window) = app_handle.get_webview_window("main") {
                                let _ = window.emit("paste-failed", ());
                            }
                        }
                    }

                    // Notification sound
                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.emit("response-ready", ());
                    }

                    // TTS
                    if *tts_enabled.lock().unwrap() {
                        let openai_tts = openai_for_tts.clone();
                        let tts_sink2 = tts_sink.clone();
                        let tts_handle2 = tts_stream_handle.clone();
                        let tts_text = transcribed_text.clone();
                        tokio::spawn(async move {
                            if let Ok(audio) = openai_tts.speak_text(&tts_text).await {
                                let mut sg = tts_sink2.lock().unwrap();
                                if let Some(s) = sg.take() { s.stop(); }
                                let hg = tts_handle2.lock().unwrap();
                                if let Some(h) = hg.as_ref() {
                                    if let Ok(src) = rodio::Decoder::new(std::io::Cursor::new(audio)) {
                                        if let Ok(sink) = rodio::Sink::try_new(h) {
                                            sink.append(src);
                                            *tts_sink2.lock().unwrap() = Some(sink);
                                        }
                                    }
                                }
                            }
                        });
                    }
                }
            }
            Err(e) => eprintln!("❌ Transcription error: {}", e),
        }
    });

    Ok("Recording stopped, processing...".to_string())
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
fn get_transcription_history(state: State<'_, AppState>) -> Result<Vec<TranscriptionEntry>, String> {
    state.database.load_transcriptions()
        .map_err(|e| format!("Failed to load history: {}", e))
}

#[tauri::command]
fn copy_to_clipboard(app: AppHandle, text: String) -> Result<(), String> {
    app.clipboard().write_text(text)
        .map_err(|e| format!("Failed to write to clipboard: {}", e))
}

#[tauri::command]
fn set_use_realtime(state: State<'_, AppState>, use_realtime: bool) -> Result<(), String> {
    *state.use_realtime.lock().unwrap() = use_realtime;
    println!("🔄 Switched to {} mode", if use_realtime { "Realtime" } else { "Whisper" });
    Ok(())
}

#[tauri::command]
fn get_use_realtime(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(*state.use_realtime.lock().unwrap())
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
    state.database.save_setting("selected_microphone", &device_name)
        .map_err(|e| format!("Failed to save microphone setting: {}", e))?;
    println!("🎤 Selected microphone: {}", device_name);
    Ok(())
}

#[tauri::command]
fn get_selected_microphone(state: State<'_, AppState>) -> Result<Option<String>, String> {
    state.database.load_setting("selected_microphone")
        .map_err(|e| format!("Failed to load microphone setting: {}", e))
}

#[tauri::command]
fn set_selected_prompt_model(state: State<'_, AppState>, model: String, save_as_default: Option<bool>) -> Result<(), String> {
    // Save as current session model
    state.database.save_setting("selected_prompt_model", &model)
        .map_err(|e| format!("Failed to save prompt model setting: {}", e))?;

    // Only save as user_prompt_model (for Ctrl+Shift+Space) when explicitly requested
    // This prevents Ctrl+B or Ctrl+Alt+Space from overwriting the user's preferred model
    if save_as_default.unwrap_or(false) && model != "transcribe-only" {
        state.database.save_setting("user_prompt_model", &model)
            .map_err(|e| format!("Failed to save user prompt model: {}", e))?;
        println!("💾 Selected prompt model: {} (also saved as user_prompt_model)", model);
    } else {
        println!("💾 Selected prompt model: {} (user_prompt_model unchanged)", model);
    }
    Ok(())
}

#[tauri::command]
fn get_selected_prompt_model(state: State<'_, AppState>) -> Result<Option<String>, String> {
    state.database.load_setting("selected_prompt_model")
        .map_err(|e| format!("Failed to load prompt model setting: {}", e))
}

#[tauri::command]
fn get_current_recording_mode(state: State<'_, AppState>) -> Result<String, String> {
    // Return the model that should be pre-selected based on current prompt_mode
    let prompt_mode = state.prompt_mode.lock().unwrap().clone();

    let model = match prompt_mode.as_deref() {
        Some("gpt-4o-mini") => "gpt-4o-mini".to_string(),
        Some("gpt-4.1") => "gpt-4.1".to_string(),
        None => "transcribe-only".to_string(),
        Some(other) => {
            println!("⚠️ Unknown prompt mode: {}, defaulting to transcribe-only", other);
            "transcribe-only".to_string()
        }
    };

    println!("📋 get_current_recording_mode: prompt_mode={:?}, returning model={}", prompt_mode, model);
    Ok(model)
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

/// Estimate cost in hundredths of a cent based on model and usage
fn estimate_cost_cents(model: &str, duration_ms: Option<i64>, text: &str) -> i64 {
    match model {
        "whisper" | "realtime" => {
            // $0.006/min of audio
            let minutes = duration_ms.unwrap_or(0) as f64 / 60_000.0;
            (minutes * 0.006 * 10_000.0) as i64
        }
        "gpt-4o-mini" => {
            // ~$0.60/1M output tokens, ~4 chars/token
            let tokens = text.len() as f64 / 4.0;
            (tokens * 0.60 / 1_000_000.0 * 10_000.0) as i64
        }
        "gpt-4.1" => {
            // ~$8/1M output tokens
            let tokens = text.len() as f64 / 4.0;
            (tokens * 8.0 / 1_000_000.0 * 10_000.0) as i64
        }
        _ => 0,
    }
}

#[tauri::command]
async fn send_text_prompt(state: State<'_, AppState>, app: AppHandle, prompt: String, model: String, image_data: Option<String>) -> Result<(), String> {
    println!("{} 🤖 send_text_prompt called - model: {}, image: {}, prompt: {}", ts(), model, image_data.is_some(), &prompt[..prompt.len().min(80)]);

    let openai = state.openai_client.clone();
    let database = state.database.clone();
    let last_transcription = state.last_transcription.clone();
    let app_handle = app.clone();
    let tts_enabled = state.tts_enabled.clone();
    let tts_sink = state.tts_sink.clone();
    let tts_stream_handle = state.tts_stream_handle.clone();
    let openai_for_tts = state.openai_client.clone();

    // Load conversation history before spawning
    let conv_history = get_conversation_history(&state.database);

    tokio::spawn(async move {
        match openai.send_prompt(&prompt, &model, &conv_history, image_data.as_deref()).await {
            Ok(response) => {
                println!("{} ✅ Text prompt response: {}", ts(), &response[..response.len().min(80)]);
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64;

                // Save to transcription history (for Alt+Shift+Z)
                let cost = estimate_cost_cents(&model, None, &response);
                if let Err(e) = database.save_transcription(&response, timestamp, None, Some(&model), Some(cost), Some("prompt")) {
                    eprintln!("❌ Failed to save text prompt response: {}", e);
                }

                // Save to conversation history
                let _ = database.append_conversation("user", &prompt, timestamp - 1);
                let _ = database.append_conversation("assistant", &response, timestamp);

                *last_transcription.lock().unwrap() = Some(response.clone());

                // Notify frontend to refresh history
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.emit("history-updated", ());
                }

                // Auto-paste response
                if let Err(e) = auto_paste_text(&app_handle, &response) {
                    eprintln!("❌ Failed to paste text prompt response: {}", e);
                }

                // Notify frontend that response is ready (for notification sound)
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.emit("response-ready", ());
                }

                // TTS
                if *tts_enabled.lock().unwrap() {
                    let tts_text = response.clone();
                    if let Ok(audio) = openai_for_tts.speak_text(&tts_text).await {
                        {
                            let mut sg = tts_sink.lock().unwrap();
                            if let Some(s) = sg.take() { s.stop(); }
                        }
                        let hg = tts_stream_handle.lock().unwrap();
                        if let Some(h) = hg.as_ref() {
                            if let Ok(src) = rodio::Decoder::new(std::io::Cursor::new(audio)) {
                                if let Ok(sink) = rodio::Sink::try_new(h) {
                                    sink.append(src);
                                    *tts_sink.lock().unwrap() = Some(sink);
                                    println!("🔊 TTS playback started");
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("❌ Text prompt failed: {}", e);
            }
        }
    });

    Ok(())
}

#[tauri::command]
async fn start_realtime_recording(state: State<'_, AppState>, app: AppHandle) -> Result<String, String> {
    let mut is_recording = state.is_recording.lock().unwrap();
    if *is_recording {
        return Err("Already recording".to_string());
    }

    println!("🎤 Starting realtime transcription...");
    *is_recording = true;

    // Mute system audio while recording
    if let Err(e) = system_audio::mute_system_audio() {
        eprintln!("⚠️ Failed to mute system audio: {}", e);
    }

    // Set recording start time
    *state.recording_start_time.lock().unwrap() = Some(Instant::now());

    drop(is_recording);

    // Reset current session transcript and speech state
    *state.current_session_transcript.lock().unwrap() = String::new();
    *state.speech_active.lock().unwrap() = false;
    *state.last_speech_end.lock().unwrap() = None;
    *state.last_transcription_time.lock().unwrap() = None;

    // Get selected microphone from settings
    let selected_mic = state.database.load_setting("selected_microphone")
        .ok()
        .flatten();

    println!("🔍 DEBUG: selected_mic from DB = {:?}", selected_mic);

    let realtime_client = state.realtime_client.clone();
    let current_session_transcript = state.current_session_transcript.clone();
    let is_recording_flag = state.is_recording.clone();
    let recording_start = state.recording_start_time.clone();
    let speech_active_for_listener = state.speech_active.clone();
    let last_speech_end_for_listener = state.last_speech_end.clone();
    let speech_active_for_stop = state.speech_active.clone();
    let last_speech_end_for_stop = state.last_speech_end.clone();
    let last_transcription_time_for_listener = state.last_transcription_time.clone();
    let last_transcription_time_for_stop = state.last_transcription_time.clone();
    let app_handle = app.clone();

    tokio::spawn(async move {
        match realtime_client.connect().await {
            Ok(session) => {
                println!("✅ Connected to Realtime API");

                // Configure session
                if let Err(e) = session.configure_transcription().await {
                    eprintln!("❌ Failed to configure session: {}", e);
                    *is_recording_flag.lock().unwrap() = false;
                    return;
                }

                // Start audio streaming in a blocking thread (cpal requires this)
                let (audio_tx, mut audio_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<i16>>();
                let is_recording_for_audio = is_recording_flag.clone();
                let selected_mic_for_thread = selected_mic.clone();

                println!("🔍 DEBUG: selected_mic_for_thread = {:?}", selected_mic_for_thread);

                std::thread::spawn(move || {
                    println!("🔍 DEBUG: Inside thread, selected_mic = {:?}", selected_mic_for_thread);
                    let mut streaming_recorder = audio::StreamingAudioRecorder::new();

                    // Start streaming and get the channel
                    let mut local_audio_rx = match streaming_recorder.start_streaming(selected_mic_for_thread) {
                        Ok(rx) => rx,
                        Err(e) => {
                            eprintln!("❌ Failed to start streaming: {}", e);
                            *is_recording_for_audio.lock().unwrap() = false;
                            return;
                        }
                    };

                    // Forward audio chunks to the async channel
                    while let Some(chunk) = local_audio_rx.blocking_recv() {
                        // Check if we should stop
                        if !*is_recording_for_audio.lock().unwrap() {
                            println!("🛑 Audio thread detected stop signal");
                            break;
                        }

                        if audio_tx.send(chunk).is_err() {
                            println!("🛑 Audio receiver closed");
                            break;
                        }
                    }

                    // Clean up - stop_streaming will release the microphone
                    streaming_recorder.stop_streaming();
                    println!("🎤 Audio thread finished");
                });

                // Clone session for sending audio
                let session_clone = Arc::new(session);
                let session_for_audio = session_clone.clone();
                let session_for_commit = session_clone.clone();

                // Spawn task to send audio chunks to WebSocket
                let audio_task = tokio::spawn(async move {
                    while let Some(audio_chunk) = audio_rx.recv().await {
                        let audio_bytes = audio::pcm_to_bytes(&audio_chunk);
                        if let Err(e) = session_for_audio.send_audio(&audio_bytes).await {
                            eprintln!("❌ Failed to send audio: {}", e);
                            break;
                        }
                    }
                    println!("🛑 Audio streaming finished");
                });

                // Clone for the event listener
                let is_recording_flag_check = is_recording_flag.clone();
                let app_for_listen = app_handle.clone();

                // Listen for transcription events with periodic stop check
                let listen_task = tokio::spawn(async move {
                    let _ = session_clone
                        .listen_for_events(|event| match event {
                            realtime::TranscriptionEvent::Delta(delta) => {
                                println!("📝 Delta: {}", delta.delta);

                                // Accumulate in session transcript
                                current_session_transcript.lock().unwrap().push_str(&delta.delta);

                                // Emit delta to frontend for live display
                                if let Some(window) = app_for_listen.get_webview_window("main") {
                                    let _ = window.emit("transcription-delta", delta.delta.clone());
                                }
                            }
                            realtime::TranscriptionEvent::Completed(_completed) => {
                                // Don't auto-paste on each VAD completion - wait for user to stop
                                println!("✨ Turn completed (VAD detected pause)");
                                *last_transcription_time_for_listener.lock().unwrap() = Some(Instant::now());
                            }
                            realtime::TranscriptionEvent::SpeechStarted => {
                                *speech_active_for_listener.lock().unwrap() = true;
                                println!("🗣️ Speech tracking: ACTIVE");
                            }
                            realtime::TranscriptionEvent::SpeechStopped => {
                                *speech_active_for_listener.lock().unwrap() = false;
                                *last_speech_end_for_listener.lock().unwrap() = Some(Instant::now());
                                println!("🔇 Speech tracking: STOPPED");
                            }
                        })
                        .await;
                });

                // Poll for stop signal and check time limit
                println!("👀 Monitoring for stop signal and time limit...");
                let app_for_warning = app_handle.clone();
                let mut warning_shown = false;
                let mut auto_stop_triggered = false;

                loop {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                    let still_recording = *is_recording_flag_check.lock().unwrap();

                    if !still_recording {
                        println!("🛑 Stop signal detected (is_recording = false), waiting for last transcriptions...");
                        break;
                    }

                    // Check recording duration
                    if let Some(start_time) = *recording_start.lock().unwrap() {
                        let elapsed = start_time.elapsed();

                        // Show warning at 5 minutes
                        if elapsed >= Duration::from_secs(5 * 60) && !warning_shown {
                            warning_shown = true;
                            println!("⚠️ [REALTIME] 5 seconds elapsed, showing warning...");
                            println!("⚠️ [REALTIME] Elapsed time: {:?}", elapsed);

                            if let Some(warning) = app_for_warning.get_webview_window("warning-widget") {
                                println!("⚠️ [REALTIME] Found warning widget");

                                if let Some(widget) = app_for_warning.get_webview_window("recording-widget") {
                                    println!("⚠️ [REALTIME] Found recording widget");
                                    if let Ok(widget_pos) = widget.outer_position() {
                                        // Position warning above widget
                                        let warning_x = widget_pos.x - 77; // Center warning above widget
                                        let warning_y = widget_pos.y - 70; // 10px above widget
                                        println!("⚠️ [REALTIME] Positioning warning at x:{}, y:{}", warning_x, warning_y);
                                        match warning.set_position(PhysicalPosition::new(warning_x, warning_y)) {
                                            Ok(_) => println!("⚠️ [REALTIME] ✅ Position set successfully"),
                                            Err(e) => println!("⚠️ [REALTIME] ❌ Failed to set position: {}", e),
                                        }
                                    }
                                } else {
                                    println!("⚠️ [REALTIME] ❌ Recording widget not found for positioning");
                                }

                                match warning.show() {
                                    Ok(_) => {
                                        println!("⚠️ [REALTIME] ✅ Warning shown successfully");

                                        // Auto-hide warning after 4 seconds
                                        let warning_clone = warning.clone();
                                        tokio::spawn(async move {
                                            tokio::time::sleep(tokio::time::Duration::from_secs(4)).await;
                                            println!("⚠️ [REALTIME] Auto-hiding warning after 4 seconds");
                                            match warning_clone.hide() {
                                                Ok(_) => println!("⚠️ [REALTIME] ✅ Warning auto-hidden successfully"),
                                                Err(e) => println!("⚠️ [REALTIME] ❌ Failed to auto-hide warning: {}", e),
                                            }
                                        });
                                    },
                                    Err(e) => println!("⚠️ [REALTIME] ❌ Failed to show warning: {}", e),
                                }
                            } else {
                                println!("⚠️ [REALTIME] ❌ Warning widget not found!");
                            }
                        }

                        // Auto-stop at 6 minutes
                        if elapsed >= Duration::from_secs(6 * 60) && !auto_stop_triggered {
                            auto_stop_triggered = true;
                            println!("⏰ [REALTIME] 6 minutes limit reached, auto-stopping...");
                            println!("⏰ [REALTIME] Elapsed time: {:?}", elapsed);

                            // DON'T set is_recording = false here - let the frontend's stopRecording() do it
                            // This prevents the "Not recording" error

                            // Emit event to frontend to trigger full stop (which handles transcription save, paste, etc)
                            if let Some(window) = app_for_warning.get_webview_window("main") {
                                println!("⏰ [REALTIME] Emitting widget-stop-recording event to frontend");
                                match window.emit("widget-stop-recording", ()) {
                                    Ok(_) => println!("⏰ [REALTIME] ✅ Event emitted successfully"),
                                    Err(e) => println!("⏰ [REALTIME] ❌ Failed to emit event: {}", e),
                                }
                            }

                            // Hide recording widget
                            if let Some(widget) = app_for_warning.get_webview_window("recording-widget") {
                                println!("⏰ [REALTIME] Found recording widget, hiding it");
                                match widget.hide() {
                                    Ok(_) => println!("⏰ [REALTIME] ✅ Widget hidden successfully"),
                                    Err(e) => println!("⏰ [REALTIME] ❌ Failed to hide widget: {}", e),
                                }
                            } else {
                                println!("⏰ [REALTIME] ❌ Recording widget not found!");
                            }

                            // DON'T break - let the loop continue until frontend calls stop
                            // which will set is_recording = false and trigger the break at line 478
                        }
                    }

                    if listen_task.is_finished() {
                        println!("🛑 Listen task finished unexpectedly");
                        break;
                    }
                }

                // Mic stopped. Now:
                // 1. Force-commit the audio buffer so API processes whatever was in-flight
                // 2. Wait for transcription.completed to arrive (not speech_stopped which may not come)
                // 3. Timeout quickly if nothing was in-flight
                println!("🎙️ Mic stopped, committing buffer and waiting for final transcription...");

                let stop_time = Instant::now();

                // Remember if speech was active at stop time
                let speech_was_active = *speech_active_for_stop.lock().unwrap();
                let had_any_speech = last_speech_end_for_stop.lock().unwrap().is_some() || speech_was_active;
                let transcription_before_stop = last_transcription_time_for_stop.lock().unwrap().clone();

                if had_any_speech {
                    // Explicitly commit the buffer - forces API to transcribe whatever audio is buffered
                    println!("{} 🔔 Committing audio buffer to force transcription of in-flight audio...", ts());
                    if let Err(e) = session_for_commit.commit_audio().await {
                        println!("⚠️ commit_audio failed (may be ok if VAD already committed): {}", e);
                    }

                    // Wait for a NEW transcription.completed to arrive after our stop time
                    // This is faster than waiting for speech_stopped
                    let max_wait = Duration::from_millis(3500);
                    loop {
                        tokio::time::sleep(tokio::time::Duration::from_millis(80)).await;

                        let latest_transcription = last_transcription_time_for_stop.lock().unwrap().clone();
                        let elapsed = stop_time.elapsed();

                        // Check if a new transcription arrived after we stopped
                        let new_transcription_arrived = match (latest_transcription, transcription_before_stop) {
                            (Some(latest), Some(before)) => latest > before,
                            (Some(_), None) => true,
                            _ => false,
                        };

                        if new_transcription_arrived {
                            println!("{} ✅ Final transcription arrived ({:.0}ms after stop)", ts(), elapsed.as_millis());
                            // Small buffer to ensure the text is accumulated
                            tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
                            break;
                        }

                        if elapsed > max_wait {
                            println!("{} ⏱️ No new transcription after {:.0}ms - was speech fully sent before stop?", ts(), elapsed.as_millis());
                            break;
                        }
                    }
                } else {
                    println!("📭 No speech detected during recording, stopping immediately");
                }

                // Now abort the tasks
                println!("🛑 Aborting audio and listen tasks...");
                audio_task.abort();
                listen_task.abort();

                println!("✅ Session cleanup complete");
                *is_recording_flag.lock().unwrap() = false;
            }
            Err(e) => {
                eprintln!("❌ Failed to connect to Realtime API: {}", e);
                *is_recording_flag.lock().unwrap() = false;
            }
        }
    });

    Ok("Realtime recording started".to_string())
}

#[tauri::command]
async fn stop_realtime_recording(state: State<'_, AppState>, app: AppHandle) -> Result<String, String> {
    println!("📞 stop_realtime_recording called");

    {
        let mut is_recording = state.is_recording.lock().unwrap();
        if !*is_recording {
            println!("⚠️ Not recording (is_recording already false)");
            return Err("Not recording".to_string());
        }

        println!("⏹️ Setting is_recording = false...");
        *is_recording = false;
        println!("✅ is_recording is now false");
    } // Drop lock before await

    // Restore system audio
    if let Err(e) = system_audio::unmute_system_audio() {
        eprintln!("⚠️ Failed to unmute system audio: {}", e);
    }

    // Capture recording duration for stats
    let duration_ms = state.recording_start_time.lock().unwrap()
        .map(|start| start.elapsed().as_millis() as i64);

    // Wait for the internal spawn task to finish cleanup.
    // The spawn signals completion by setting is_recording_flag=false (different from AppState.is_recording).
    // We wait up to 5s for the spawn to finish its commit+transcription wait.
    println!("⏳ Waiting for final transcription after stop...");
    {
        let wait_start = Instant::now();
        let transcription_at_stop = state.last_transcription_time.lock().unwrap().clone();
        let had_speech = state.last_speech_end.lock().unwrap().is_some()
            || *state.speech_active.lock().unwrap();
        let max_wait = Duration::from_millis(4500);

        if had_speech {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                let latest = state.last_transcription_time.lock().unwrap().clone();
                let new_arrived = match (latest, transcription_at_stop) {
                    (Some(l), Some(b)) => l > b,
                    (Some(_), None) => true,
                    _ => false,
                };

                if new_arrived {
                    println!("{} ✅ Transcription received, reading transcript now", ts());
                    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                    break;
                }

                if wait_start.elapsed() > max_wait {
                    println!("{} ⏱️ Timeout waiting for transcription ({:.0}ms)", ts(), wait_start.elapsed().as_millis());
                    break;
                }
            }
        } else {
            println!("📭 No speech, proceeding immediately");
        }
    }

    // Get accumulated transcript
    println!("📝 Getting accumulated transcript...");
    let transcript = state.current_session_transcript.lock().unwrap().clone();
    println!("📝 Transcript length: {} characters", transcript.len());

    // Check selected model in database FIRST (allows changing model during any recording)
    let (should_use_prompt, selected_model) = {
        let mut pm = state.prompt_mode.lock().unwrap();
        let mode = pm.clone();
        println!("🔍 DEBUG: prompt_mode at start of stop_realtime_recording = {:?}", mode);

        // Always check the current selected model in database
        let current_model = state.database.load_setting("selected_prompt_model")
            .ok()
            .flatten()
            .unwrap_or_else(|| {
                println!("⚠️ No model found in settings, defaulting to transcribe-only");
                "transcribe-only".to_string()
            });
        println!("🔍 DEBUG: Current selected model in database = '{}'", current_model);

        *pm = None; // Clear for next recording

        // If model is "transcribe-only", treat as normal transcription (no prompt)
        if current_model == "transcribe-only" {
            println!("📝 Model is 'transcribe-only' - will NOT send to GPT");
            (false, String::new())
        } else {
            println!("🤖 Model is '{}' - WILL send to GPT (regardless of how recording started)", current_model);
            (true, current_model)
        }
    };

    println!("🎯 Final decision: should_use_prompt = {}, selected_model = '{}'", should_use_prompt, selected_model);

    if !transcript.is_empty() {
        // Check if we need to send to GPT first
        if should_use_prompt {
            println!("🤖 [REALTIME] Prompt mode active with model: {}", selected_model);

            // Load conversation history before spawning
            let conv_history = get_conversation_history(&state.database);

            // Send transcript as prompt to GPT
            let openai = state.openai_client.clone();
            let database = state.database.clone();
            let last_transcription = state.last_transcription.clone();
            let app_clone = app.clone();
            let transcript_clone = transcript.clone();
            let tts_enabled_rt = state.tts_enabled.clone();
            let tts_sink_rt = state.tts_sink.clone();
            let tts_handle_rt = state.tts_stream_handle.clone();
            let openai_tts_rt = state.openai_client.clone();

            tokio::spawn(async move {
                match openai.send_prompt(&transcript_clone, &selected_model, &conv_history, None).await {
                    Ok(gpt_response) => {
                        println!("✨ GPT Response: {}", gpt_response);

                        // Save GPT response to database (not the transcript)
                        let timestamp = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_millis() as i64;

                        let cost = estimate_cost_cents(&selected_model, duration_ms, &gpt_response);
                        if let Err(e) = database.save_transcription(&gpt_response, timestamp, duration_ms, Some(&selected_model), Some(cost), Some("prompt")) {
                            eprintln!("❌ Failed to save to database: {}", e);
                        }

                        // Save to conversation history
                        let _ = database.append_conversation("user", &transcript_clone, timestamp - 1);
                        let _ = database.append_conversation("assistant", &gpt_response, timestamp);

                        // Update last transcription with GPT response
                        *last_transcription.lock().unwrap() = Some(gpt_response.clone());

                        // Notify frontend
                        if let Some(window) = app_clone.get_webview_window("main") {
                            let _ = window.emit("history-updated", ());
                        }

                        // Auto-paste GPT response
                        match auto_paste_text(&app_clone, &gpt_response) {
                            Ok(_) => println!("✅ GPT response auto-pasted"),
                            Err(e) => eprintln!("⚠️ Auto-paste failed: {}", e),
                        }

                        // Notification sound
                        if let Some(window) = app_clone.get_webview_window("main") {
                            let _ = window.emit("response-ready", ());
                        }

                        // TTS
                        if *tts_enabled_rt.lock().unwrap() {
                            let tts_text = gpt_response.clone();
                            if let Ok(audio) = openai_tts_rt.speak_text(&tts_text).await {
                                {
                                    let mut sg = tts_sink_rt.lock().unwrap();
                                    if let Some(s) = sg.take() { s.stop(); }
                                }
                                let hg = tts_handle_rt.lock().unwrap();
                                if let Some(h) = hg.as_ref() {
                                    if let Ok(src) = rodio::Decoder::new(std::io::Cursor::new(audio)) {
                                        if let Ok(sink) = rodio::Sink::try_new(h) {
                                            sink.append(src);
                                            *tts_sink_rt.lock().unwrap() = Some(sink);
                                            println!("🔊 TTS playback started");
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => eprintln!("❌ GPT prompt error: {}", e),
                }
            });
        } else {
            // Normal mode: just paste the transcript
            // Save to database (single entry for entire session)
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64;

            let cost = estimate_cost_cents("realtime", duration_ms, &transcript);
            if let Err(e) = state.database.save_transcription(&transcript, timestamp, duration_ms, Some("realtime"), Some(cost), Some("transcription")) {
                eprintln!("❌ Failed to save to database: {}", e);
            }

            // Update last transcription
            *state.last_transcription.lock().unwrap() = Some(transcript.clone());

            // Notify frontend
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.emit("history-updated", ());
            }

            // Auto-paste the full session transcript
            let app_clone = app.clone();
            let text_clone = transcript.clone();
            let app_for_sound = app.clone();
            let tts_enabled_nm = state.tts_enabled.clone();
            let tts_sink_nm = state.tts_sink.clone();
            let tts_handle_nm = state.tts_stream_handle.clone();
            let openai_tts_nm = state.openai_client.clone();
            let tts_text_nm = transcript.clone();
            std::thread::spawn(move || {
                match auto_paste_text(&app_clone, &text_clone) {
                    Ok(_) => println!("✅ Session transcript auto-pasted"),
                    Err(e) => eprintln!("⚠️ Auto-paste failed: {}", e),
                }

                // Notification sound
                if let Some(window) = app_for_sound.get_webview_window("main") {
                    let _ = window.emit("response-ready", ());
                }

                // TTS (spawn async task from sync thread via tauri runtime)
                if *tts_enabled_nm.lock().unwrap() {
                    tauri::async_runtime::spawn(async move {
                        if let Ok(audio) = openai_tts_nm.speak_text(&tts_text_nm).await {
                            {
                                let mut sg = tts_sink_nm.lock().unwrap();
                                if let Some(s) = sg.take() { s.stop(); }
                            }
                            let hg = tts_handle_nm.lock().unwrap();
                            if let Some(h) = hg.as_ref() {
                                if let Ok(src) = rodio::Decoder::new(std::io::Cursor::new(audio)) {
                                    if let Ok(sink) = rodio::Sink::try_new(h) {
                                        sink.append(src);
                                        *tts_sink_nm.lock().unwrap() = Some(sink);
                                        println!("🔊 TTS playback started");
                                    }
                                }
                            }
                        }
                    });
                }
            });
        }
    }

    Ok("Realtime recording stopped".to_string())
}

#[tauri::command]
async fn get_statistics(state: State<'_, AppState>, from_ts: i64, to_ts: i64) -> Result<db::StatsData, String> {
    state.database.get_stats(from_ts, to_ts)
        .map_err(|e| format!("Failed to get stats: {}", e))
}

#[tauri::command]
fn get_tts_enabled(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(*state.tts_enabled.lock().unwrap())
}

#[tauri::command]
fn set_tts_enabled(state: State<'_, AppState>, app: AppHandle, enabled: bool) -> Result<(), String> {
    *state.tts_enabled.lock().unwrap() = enabled;
    state.database.save_setting("tts_enabled", if enabled { "true" } else { "false" })
        .map_err(|e| format!("Failed to save TTS setting: {}", e))?;
    println!("🔊 TTS {}", if enabled { "enabled" } else { "disabled" });
    // Notify frontend
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit("tts-toggled", enabled);
    }
    Ok(())
}

#[tauri::command]
fn stop_tts_playback(state: State<'_, AppState>) -> Result<(), String> {
    let mut sink_guard = state.tts_sink.lock().unwrap();
    if let Some(sink) = sink_guard.take() {
        sink.stop();
        println!("🔇 TTS playback stopped");
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Load .env file
    dotenv::dotenv().ok();

    // Load OpenAI API key from environment
    let api_key = std::env::var("OPENAI_API_KEY")
        .expect("OPENAI_API_KEY must be set in .env file");

    // Initialize database - get app data directory
    let db_path = std::env::current_dir()
        .expect("Failed to get current directory")
        .join("dicta.db");

    println!("📁 Database will be created at: {}", db_path.display());

    let database = Arc::new(
        db::Database::new(db_path)
            .expect("Failed to initialize database")
    );

    // Load TTS preference from DB
    let tts_default = database.load_setting("tts_enabled")
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false);

    // Initialize audio output stream for TTS
    // Leak the OutputStream so it lives for the app's lifetime (it's not Send, can't go in AppState)
    let tts_stream_handle_val = match rodio::OutputStream::try_default() {
        Ok((stream, handle)) => {
            // Leak the stream so it stays alive forever (app-lifetime resource)
            std::mem::forget(stream);
            Some(handle)
        }
        Err(e) => {
            eprintln!("⚠️ Failed to initialize audio output for TTS: {}", e);
            None
        }
    };

    // Initialize app state
    let app_state = AppState {
        audio_recorder: Arc::new(Mutex::new(audio::AudioRecorder::new())),
        openai_client: Arc::new(openai::OpenAIClient::new(api_key.clone())),
        realtime_client: Arc::new(realtime::RealtimeClient::new(api_key)),
        database,
        is_recording: Arc::new(Mutex::new(false)),
        use_realtime: Arc::new(Mutex::new(true)), // Default to Realtime API
        prompt_mode: Arc::new(Mutex::new(None)),
        current_session_transcript: Arc::new(Mutex::new(String::new())),
        last_transcription: Arc::new(Mutex::new(None)),
        paste_in_progress: Arc::new(Mutex::new(false)),
        recording_start_time: Arc::new(Mutex::new(None)),
        speech_active: Arc::new(Mutex::new(false)),
        last_speech_end: Arc::new(Mutex::new(None)),
        last_transcription_time: Arc::new(Mutex::new(None)),
        tts_enabled: Arc::new(Mutex::new(tts_default)),
        tts_sink: Arc::new(Mutex::new(None)),
        tts_stream_handle: Arc::new(Mutex::new(tts_stream_handle_val)),
    };

    // Debounce: prevent multiple triggers when keys are held down
    let last_recording_trigger = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(1)));
    let last_recording_trigger_clone = last_recording_trigger.clone();

    let last_paste_trigger = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(1)));
    let last_paste_trigger_clone = last_paste_trigger.clone();

    tauri::Builder::default()
        .manage(app_state)
        .plugin(tauri_plugin_shell::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    // Only handle key press events, ignore key release
                    let event_str = format!("{:?}", event);
                    if !event_str.contains("Pressed") {
                        return; // Ignore Released events
                    }

                    // Check which shortcut was pressed
                    let shortcut_str = format!("{:?}", shortcut);

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
                                    let model = state.database.load_setting("user_prompt_model")
                                        .ok()
                                        .flatten()
                                        .unwrap_or_else(|| "gpt-4o-mini".to_string());

                                    // Save as current session model
                                    let _ = state.database.save_setting("selected_prompt_model", &model);

                                    *state.prompt_mode.lock().unwrap() = Some(model.clone());
                                    println!("🤖 Prompt mode enabled: {} (saved to DB)", model);

                                    // Show widget
                                    if let Some(widget) = app.get_webview_window("recording-widget") {
                                        if let Ok(monitor) = widget.current_monitor() {
                                            if let Some(monitor) = monitor {
                                                let screen_size = monitor.size();
                                                let widget_width = 125;
                                                let widget_height = 120; // Height increased for combo box
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
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.emit("toggle-recording", ());
                            }
                        } else {
                            println!("⏭️ Ctrl+Shift+Space ignored (debounce)");
                        }
                    } else if shortcut_str.contains("Space") && shortcut_str.contains("CONTROL") && shortcut_str.contains("ALT") {
                        // Ctrl+Alt+Space: Toggle recording with GPT-4o prompt mode
                        let mut last = last_recording_trigger_clone.lock().unwrap();
                        let now = Instant::now();

                        if now.duration_since(*last) > Duration::from_millis(100) {
                            *last = now;
                            println!("🔥 Hotkey pressed: Ctrl+Alt+Space (GPT-4o mode)");

                            if let Some(state) = app.try_state::<AppState>() {
                                let is_recording = *state.is_recording.lock().unwrap();

                                if !is_recording {
                                    // Set prompt mode to gpt-4.1 and save to database
                                    let _ = state.database.save_setting("selected_prompt_model", "gpt-4.1");
                                    *state.prompt_mode.lock().unwrap() = Some("gpt-4.1".to_string());
                                    println!("🤖 Prompt mode enabled: gpt-4.1 (saved to DB)");

                                    // Show widget
                                    if let Some(widget) = app.get_webview_window("recording-widget") {
                                        if let Ok(monitor) = widget.current_monitor() {
                                            if let Some(monitor) = monitor {
                                                let screen_size = monitor.size();
                                                let widget_width = 125;
                                                let widget_height = 120; // Height increased for combo box
                                                let bottom_margin = 200; // More space from taskbar

                                                let x = (screen_size.width as i32 - widget_width) / 2;
                                                let y = screen_size.height as i32 - widget_height - bottom_margin;

                                                let _ = widget.set_position(PhysicalPosition::new(x, y));
                                            }
                                        }
                                        let _ = widget.show();
                                        // Tell widget which model is active
                                        let _ = widget.emit("model-selected", "gpt-4.1".to_string());
                                    }
                                } else {
                                    // Stopping recording - DON'T clear prompt_mode here
                                    // It will be cleared in stop_realtime_recording after being used
                                    println!("🛑 [Ctrl+Alt+Space] Stopping - prompt_mode (gpt-4.1) will be used in stop handler");

                                    if let Some(widget) = app.get_webview_window("recording-widget") {
                                        let _ = widget.hide();
                                    }
                                }
                            }

                            // Emit event to frontend
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.emit("toggle-recording", ());
                            }
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
                                    // Check if prompt mode was already set by Ctrl+Shift+Space or Ctrl+Alt+Space
                                    let current_prompt_mode = state.prompt_mode.lock().unwrap().clone();

                                    // Determine which model to show in widget
                                    let widget_model = if current_prompt_mode.is_none() {
                                        println!("📝 Ctrl+Space starting - setting prompt mode to None (normal transcription)");
                                        let _ = state.database.save_setting("selected_prompt_model", "transcribe-only");
                                        *state.prompt_mode.lock().unwrap() = None;
                                        "transcribe-only".to_string()
                                    } else {
                                        println!("⚠️ Ctrl+Space starting but prompt_mode already set to {:?} - keeping it", current_prompt_mode);
                                        current_prompt_mode.clone().unwrap_or_else(|| "transcribe-only".to_string())
                                    };

                                    // Starting recording - show widget
                                    if let Some(widget) = app.get_webview_window("recording-widget") {
                                        // Position widget at bottom-center of screen
                                        if let Ok(monitor) = widget.current_monitor() {
                                            if let Some(monitor) = monitor {
                                                let screen_size = monitor.size();
                                                let widget_width = 125;
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
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.emit("toggle-recording", ());
                            }
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
                        }
                    } else if shortcut_str.contains("KeyS") && shortcut_str.contains("ALT") && shortcut_str.contains("SHIFT") {
                        // Alt+Shift+S: Stop TTS playback or read last message
                        tlog!("🔥 Hotkey pressed: Alt+Shift+S (TTS action)");
                        if let Some(state) = app.try_state::<AppState>() {
                            // Check if something is playing
                            let is_playing = {
                                let sink_guard = state.tts_sink.lock().unwrap();
                                sink_guard.as_ref().map(|s| !s.empty()).unwrap_or(false)
                            };

                            if is_playing {
                                // Stop current playback
                                let mut sink_guard = state.tts_sink.lock().unwrap();
                                if let Some(sink) = sink_guard.take() {
                                    sink.stop();
                                    println!("🔇 TTS playback stopped via Ctrl+S");
                                }
                            } else {
                                // Read last message aloud
                                let last_text = state.last_transcription.lock().unwrap().clone();
                                if let Some(text) = last_text {
                                    println!("🔊 Reading last message via TTS: {}...", &text[..text.len().min(50)]);
                                    let openai = state.openai_client.clone();
                                    let tts_sink = state.tts_sink.clone();
                                    let tts_handle = state.tts_stream_handle.clone();
                                    tauri::async_runtime::spawn(async move {
                                        if let Ok(audio) = openai.speak_text(&text).await {
                                            {
                                                let mut sg = tts_sink.lock().unwrap();
                                                if let Some(s) = sg.take() { s.stop(); }
                                            }
                                            let hg = tts_handle.lock().unwrap();
                                            if let Some(h) = hg.as_ref() {
                                                if let Ok(src) = rodio::Decoder::new(std::io::Cursor::new(audio)) {
                                                    if let Ok(sink) = rodio::Sink::try_new(h) {
                                                        sink.append(src);
                                                        *tts_sink.lock().unwrap() = Some(sink);
                                                        println!("🔊 TTS playback started");
                                                    }
                                                }
                                            }
                                        }
                                    });
                                } else {
                                    println!("⚠️ No message to read aloud");
                                }
                            }
                        }
                    } else if shortcut_str.contains("KeyZ") {
                        // Alt+Shift+Z: Get last transcription from history and paste it
                        tlog!("🔥 Hotkey pressed: Alt+Shift+Z");

                        // Get app state
                        if let Some(state) = app.try_state::<AppState>() {
                            // Check if paste is already in progress
                            let mut paste_in_progress = state.paste_in_progress.lock().unwrap();
                            if *paste_in_progress {
                                println!("⏭️ Alt+Shift+Z ignored (paste already in progress)");
                                return;
                            }

                            // Check debounce (increased to account for the paste operation duration)
                            let mut last_paste = last_paste_trigger_clone.lock().unwrap();
                            let now = Instant::now();
                            // Total paste operation takes ~600ms (delay) + ~300ms (restore) = ~900ms
                            // We use 1000ms debounce to be safe
                            if now.duration_since(*last_paste) < Duration::from_millis(1000) {
                                println!("⏭️ Alt+Shift+Z ignored (debounce - paste takes ~900ms)");
                                return;
                            }

                            *last_paste = now;
                            drop(last_paste); // Release debounce lock

                            // Get last transcription from database
                            match state.database.load_transcriptions() {
                                Ok(history) if !history.is_empty() => {
                                    let last_entry = &history[0]; // First entry is most recent
                                    println!("📋 Pasting last transcription from history: {}", last_entry.text);
                                    let text_clone = last_entry.text.clone();

                                    // Mark paste as in progress
                                    *paste_in_progress = true;
                                    drop(paste_in_progress); // Release lock before spawning thread

                                    let app_handle = app.app_handle().clone();
                                    let paste_flag = state.paste_in_progress.clone();

                                    // auto_paste_text handles: save clipboard, copy text, paste (Ctrl+V), restore clipboard
                                    std::thread::spawn(move || {
                                        // Small delay to ensure clipboard is ready
                                        std::thread::sleep(std::time::Duration::from_millis(100));

                                        if let Err(e) = auto_paste_text(&app_handle, &text_clone) {
                                            eprintln!("❌ Failed to paste: {}", e);
                                        }
                                        // Mark paste as complete
                                        *paste_flag.lock().unwrap() = false;
                                        println!("✅ Paste operation completed");
                                    });
                                }
                                _ => {
                                    *paste_in_progress = false; // Reset flag if no transcription available
                                    println!("⚠️ No transcription available to paste");
                                }
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
            start_realtime_recording,
            stop_realtime_recording,
            set_use_realtime,
            get_use_realtime,
            list_microphones,
            set_selected_microphone,
            get_selected_microphone,
            set_selected_prompt_model,
            get_selected_prompt_model,
            get_current_recording_mode,
            send_text_prompt,
            get_statistics,
            get_tts_enabled,
            set_tts_enabled,
            stop_tts_playback
        ])
        .setup(|app| {
            // Create tray menu
            let show_item = MenuItem::with_id(app, "show", "Abrir Dicta", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

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

            // Register global hotkeys
            let shortcut_record = Shortcut::new(Some(Modifiers::CONTROL), Code::Space);
            app.global_shortcut().register(shortcut_record).unwrap();

            let shortcut_prompt_mini = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space);
            app.global_shortcut().register(shortcut_prompt_mini).unwrap();

            let shortcut_prompt_full = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space);
            app.global_shortcut().register(shortcut_prompt_full).unwrap();

            let shortcut_paste = Shortcut::new(
                Some(Modifiers::ALT | Modifiers::SHIFT),
                Code::KeyZ
            );
            app.global_shortcut().register(shortcut_paste).unwrap();

            let shortcut_prompt_input = Shortcut::new(Some(Modifiers::CONTROL), Code::KeyB);
            app.global_shortcut().register(shortcut_prompt_input).unwrap();

            let shortcut_tts_toggle = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyS);
            app.global_shortcut().register(shortcut_tts_toggle).unwrap();

            let shortcut_tts_action = Shortcut::new(Some(Modifiers::ALT | Modifiers::SHIFT), Code::KeyS);
            app.global_shortcut().register(shortcut_tts_action).unwrap();

            println!("✅ Dicta is running!");
            println!("📌 Press Ctrl+Space to start/stop recording");
            println!("📌 Press Ctrl+Shift+Space for GPT-4o-mini prompt mode");
            println!("📌 Press Ctrl+Alt+Space for GPT-4.1 prompt mode");
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
