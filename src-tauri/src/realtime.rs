use tokio_tungstenite::{connect_async, tungstenite::protocol::Message, tungstenite::client::IntoClientRequest};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;
use base64::{Engine as _, engine::general_purpose};

const REALTIME_API_URL: &str = "wss://api.openai.com/v1/realtime?model=gpt-4o-realtime-preview-2024-12-17";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionDelta {
    pub item_id: String,
    pub delta: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionCompleted {
    pub item_id: String,
    pub transcript: String,
}

pub struct RealtimeClient {
    api_key: String,
}

impl RealtimeClient {
    pub fn new(api_key: String) -> Self {
        Self { api_key }
    }

    pub async fn connect(&self) -> Result<RealtimeSession, String> {
        println!("🔌 Connecting to OpenAI Realtime API...");

        // Create a proper WebSocket request
        let mut request = REALTIME_API_URL.into_client_request()
            .map_err(|e| format!("Failed to create request: {}", e))?;

        // Add authorization header
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", self.api_key)
                .parse()
                .map_err(|e| format!("Failed to parse auth header: {}", e))?
        );

        request.headers_mut().insert(
            "OpenAI-Beta",
            "realtime=v1"
                .parse()
                .map_err(|e| format!("Failed to parse beta header: {}", e))?
        );

        let (ws_stream, _) = connect_async(request)
            .await
            .map_err(|e| format!("Failed to connect: {}", e))?;

        println!("✅ Connected to Realtime API");

        let (write, read) = ws_stream.split();

        Ok(RealtimeSession {
            write: Arc::new(Mutex::new(write)),
            read: Arc::new(Mutex::new(read)),
        })
    }
}

pub struct RealtimeSession {
    write: Arc<Mutex<futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, Message>>>,
    read: Arc<Mutex<futures_util::stream::SplitStream<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>>>,
}

impl RealtimeSession {
    /// Configure the session for transcription-only mode
    pub async fn configure_transcription(&self) -> Result<(), String> {
        println!("⚙️ Configuring transcription session...");

        let config = json!({
            "type": "session.update",
            "session": {
                "modalities": ["text"], // Only text, no audio output
                "input_audio_format": "pcm16",
                "input_audio_transcription": {
                    "model": "whisper-1"
                },
                "turn_detection": {
                    "type": "server_vad",
                    "threshold": 0.5,
                    "prefix_padding_ms": 300,
                    "silence_duration_ms": 500
                }
            }
        });

        let mut write = self.write.lock().await;
        write.send(Message::Text(config.to_string()))
            .await
            .map_err(|e| format!("Failed to send config: {}", e))?;

        println!("✅ Session configured for transcription-only");
        Ok(())
    }

    /// Send audio data to the API
    pub async fn send_audio(&self, audio_data: &[u8]) -> Result<(), String> {
        let audio_base64 = general_purpose::STANDARD.encode(audio_data);

        let message = json!({
            "type": "input_audio_buffer.append",
            "audio": audio_base64
        });

        let mut write = self.write.lock().await;
        write.send(Message::Text(message.to_string()))
            .await
            .map_err(|e| format!("Failed to send audio: {}", e))?;

        Ok(())
    }

    /// Commit the audio buffer (trigger transcription)
    pub async fn commit_audio(&self) -> Result<(), String> {
        let message = json!({
            "type": "input_audio_buffer.commit"
        });

        let mut write = self.write.lock().await;
        write.send(Message::Text(message.to_string()))
            .await
            .map_err(|e| format!("Failed to commit audio: {}", e))?;

        println!("✅ Audio buffer committed");
        Ok(())
    }

    /// Listen for transcription events
    pub async fn listen_for_events<F>(&self, mut on_event: F) -> Result<(), String>
    where
        F: FnMut(TranscriptionEvent),
    {
        let mut read = self.read.lock().await;

        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(event) = serde_json::from_str::<serde_json::Value>(&text) {
                        let event_type = event["type"].as_str().unwrap_or("");

                        match event_type {
                            "conversation.item.input_audio_transcription.delta" => {
                                if let Some(delta) = event["delta"].as_str() {
                                    println!("📝 Transcription delta: {}", delta);
                                    on_event(TranscriptionEvent::Delta(TranscriptionDelta {
                                        item_id: event["item_id"].as_str().unwrap_or("").to_string(),
                                        delta: delta.to_string(),
                                    }));
                                }
                            }
                            "conversation.item.input_audio_transcription.completed" => {
                                if let Some(transcript) = event["transcript"].as_str() {
                                    println!("✅ Transcription completed: {}", transcript);
                                    on_event(TranscriptionEvent::Completed(TranscriptionCompleted {
                                        item_id: event["item_id"].as_str().unwrap_or("").to_string(),
                                        transcript: transcript.to_string(),
                                    }));
                                }
                            }
                            "error" => {
                                let error_msg = event["error"]["message"].as_str().unwrap_or("Unknown error");
                                eprintln!("❌ API Error: {}", error_msg);
                                eprintln!("Full error event: {}", serde_json::to_string_pretty(&event).unwrap_or_default());
                            }
                            "session.created" | "session.updated" => {
                                println!("📥 Session event: {}", event_type);
                            }
                            "input_audio_buffer.speech_started" => {
                                println!("🎤 Speech detected");
                            }
                            "input_audio_buffer.speech_stopped" => {
                                println!("🤫 Speech stopped");
                            }
                            "input_audio_buffer.committed" => {
                                println!("✅ Audio buffer committed");
                            }
                            _ => {
                                // Log other events for debugging
                                println!("📥 Event: {}", event_type);
                            }
                        }
                    }
                }
                Ok(Message::Close(_)) => {
                    println!("🔌 WebSocket closed");
                    break;
                }
                Err(e) => {
                    eprintln!("❌ WebSocket error: {}", e);
                    return Err(format!("WebSocket error: {}", e));
                }
                _ => {}
            }
        }

        Ok(())
    }

    /// Close the WebSocket connection
    pub async fn close(&self) -> Result<(), String> {
        let mut write = self.write.lock().await;
        write.send(Message::Close(None))
            .await
            .map_err(|e| format!("Failed to close: {}", e))?;
        println!("🔌 WebSocket connection closed");
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub enum TranscriptionEvent {
    Delta(TranscriptionDelta),
    Completed(TranscriptionCompleted),
}
