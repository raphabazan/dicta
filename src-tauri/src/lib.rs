mod audio;
mod openai;
mod realtime;
mod db;

use tauri::{Emitter, Manager, State, AppHandle};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, GlobalShortcutExt};
use tauri_plugin_clipboard_manager::ClipboardExt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use enigo::{Enigo, Key, Keyboard, Settings};

// Re-export TranscriptionEntry from db module
use db::TranscriptionEntry;

fn auto_paste_text(app: &AppHandle, text: &str) -> Result<(), String> {
    println!("🔄 Auto-pasting text...");

    // 1. Read current clipboard
    let original_clipboard = app.clipboard().read_text()
        .map_err(|e| format!("Failed to read clipboard: {}", e))?;

    println!("💾 Saved original clipboard: '{}'",
        if original_clipboard.len() > 30 {
            format!("{}...", &original_clipboard[..30])
        } else {
            original_clipboard.clone()
        }
    );

    // 2. Write transcribed text to clipboard
    app.clipboard().write_text(text)
        .map_err(|e| format!("Failed to write to clipboard: {}", e))?;

    println!("📋 Transcription in clipboard");

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
    current_session_transcript: Arc<Mutex<String>>, // Accumulate transcript for current session
    last_transcription: Arc<Mutex<Option<String>>>,
    paste_in_progress: Arc<Mutex<bool>>,
}

#[tauri::command]
async fn start_recording_audio(state: State<'_, AppState>) -> Result<String, String> {
    let mut is_recording = state.is_recording.lock().unwrap();
    if *is_recording {
        return Err("Already recording".to_string());
    }

    println!("🎤 Starting audio recording...");

    // Get selected microphone from settings
    let selected_mic = state.database.load_setting("selected_microphone")
        .ok()
        .flatten();

    let recorder = state.audio_recorder.lock().unwrap();
    recorder.start_recording(selected_mic)?;
    *is_recording = true;

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

    if audio_data.is_empty() {
        return Err("No audio recorded".to_string());
    }

    // Transcribe (without post-processing for speed)
    let openai = state.openai_client.clone();
    let last_transcription = state.last_transcription.clone();
    let database = state.database.clone();
    let app_handle = app.clone();
    tokio::spawn(async move {
        match openai.transcribe_audio(audio_data, 48000).await {
            Ok(transcribed_text) => {
                println!("✨ Transcribed: {}", transcribed_text);

                // Save last transcription
                *last_transcription.lock().unwrap() = Some(transcribed_text.clone());

                // Save to database
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as i64;

                if let Err(e) = database.save_transcription(&transcribed_text, timestamp) {
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
async fn start_realtime_recording(state: State<'_, AppState>, app: AppHandle) -> Result<String, String> {
    let mut is_recording = state.is_recording.lock().unwrap();
    if *is_recording {
        return Err("Already recording".to_string());
    }

    println!("🎤 Starting realtime transcription...");
    *is_recording = true;
    drop(is_recording);

    // Reset current session transcript
    *state.current_session_transcript.lock().unwrap() = String::new();

    // Get selected microphone from settings
    let selected_mic = state.database.load_setting("selected_microphone")
        .ok()
        .flatten();

    println!("🔍 DEBUG: selected_mic from DB = {:?}", selected_mic);

    let realtime_client = state.realtime_client.clone();
    let current_session_transcript = state.current_session_transcript.clone();
    let is_recording_flag = state.is_recording.clone();
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

                // Listen for transcription events with periodic stop check
                let listen_task = tokio::spawn(async move {
                    let _ = session_clone
                        .listen_for_events(|event| match event {
                            realtime::TranscriptionEvent::Delta(delta) => {
                                println!("📝 Delta: {}", delta.delta);

                                // Accumulate in session transcript
                                current_session_transcript.lock().unwrap().push_str(&delta.delta);

                                // Emit delta to frontend for live display
                                if let Some(window) = app_handle.get_webview_window("main") {
                                    let _ = window.emit("transcription-delta", delta.delta.clone());
                                }
                            }
                            realtime::TranscriptionEvent::Completed(_completed) => {
                                // Don't auto-paste on each VAD completion - wait for user to stop
                                println!("✨ Turn completed (VAD detected pause)");
                            }
                        })
                        .await;
                });

                // Poll for stop signal
                println!("👀 Monitoring for stop signal...");
                loop {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                    let still_recording = *is_recording_flag_check.lock().unwrap();

                    if !still_recording {
                        println!("🛑 Stop signal detected (is_recording = false), waiting for last transcriptions...");
                        break;
                    }

                    if listen_task.is_finished() {
                        println!("🛑 Listen task finished unexpectedly");
                        break;
                    }
                }

                // Stop sending new audio (audio thread will detect is_recording=false and stop)
                // But keep listening for transcription events for a bit longer
                println!("⏳ Waiting 2 seconds for final transcription events...");
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

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

    // Wait for the recording task to clean up (it waits 2 seconds for final events)
    println!("⏳ Waiting 3 seconds for recording task to finish cleanup...");
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    // Get accumulated transcript
    println!("📝 Getting accumulated transcript...");
    let transcript = state.current_session_transcript.lock().unwrap().clone();
    println!("📝 Transcript length: {} characters", transcript.len());

    if !transcript.is_empty() {
        // Save to database (single entry for entire session)
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        if let Err(e) = state.database.save_transcription(&transcript, timestamp) {
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
        std::thread::spawn(move || {
            match auto_paste_text(&app_clone, &text_clone) {
                Ok(_) => println!("✅ Session transcript auto-pasted"),
                Err(e) => eprintln!("⚠️ Auto-paste failed: {}", e),
            }
        });
    }

    Ok("Realtime recording stopped".to_string())
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

    // Initialize app state
    let app_state = AppState {
        audio_recorder: Arc::new(Mutex::new(audio::AudioRecorder::new())),
        openai_client: Arc::new(openai::OpenAIClient::new(api_key.clone())),
        realtime_client: Arc::new(realtime::RealtimeClient::new(api_key)),
        database,
        is_recording: Arc::new(Mutex::new(false)),
        use_realtime: Arc::new(Mutex::new(true)), // Default to Realtime API
        current_session_transcript: Arc::new(Mutex::new(String::new())),
        last_transcription: Arc::new(Mutex::new(None)),
        paste_in_progress: Arc::new(Mutex::new(false)),
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

                    if shortcut_str.contains("Space") {
                        // Ctrl+Space: Toggle recording (with minimal debounce for safety)
                        let mut last = last_recording_trigger_clone.lock().unwrap();
                        let now = Instant::now();

                        // Only trigger if 100ms have passed (minimal debounce, since we filter Pressed)
                        if now.duration_since(*last) > Duration::from_millis(100) {
                            *last = now;
                            println!("🔥 Hotkey pressed: Ctrl+Space");

                            // Emit event to frontend
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.emit("toggle-recording", ());
                            }
                        } else {
                            println!("⏭️ Ctrl+Space ignored (debounce - too fast)");
                        }
                    } else if shortcut_str.contains("KeyZ") {
                        // Alt+Shift+Z: Get last transcription from history and paste it
                        println!("🔥 Hotkey pressed: Alt+Shift+Z");

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
            get_last_transcription,
            get_transcription_history,
            copy_to_clipboard,
            start_realtime_recording,
            stop_realtime_recording,
            set_use_realtime,
            get_use_realtime,
            list_microphones,
            set_selected_microphone,
            get_selected_microphone
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

            // Register global hotkeys
            let shortcut_record = Shortcut::new(Some(Modifiers::CONTROL), Code::Space);
            app.global_shortcut().register(shortcut_record).unwrap();

            let shortcut_paste = Shortcut::new(
                Some(Modifiers::ALT | Modifiers::SHIFT),
                Code::KeyZ
            );
            app.global_shortcut().register(shortcut_paste).unwrap();

            println!("✅ Dicta is running!");
            println!("📌 Press Ctrl+Space to start/stop recording");
            println!("📌 Press Alt+Shift+Z to paste last transcription");
            println!("🔑 OpenAI API key loaded");

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
