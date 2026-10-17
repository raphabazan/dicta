use serde_json::{json, Value};
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct OpenAIClient {
    api_key: String,
    client: reqwest::Client,
    usage_store: Option<std::sync::Arc<crate::db::Database>>,
    source_id: Option<String>,
}

pub fn api_error(status: u16, value: &Value) -> String {
    let code = value["error"]["code"]
        .as_str()
        .or_else(|| value["error"]["type"].as_str())
        .unwrap_or("api_error");
    let safe: String = code
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(80)
        .collect();
    format!("API error ({status}): {safe}")
}
pub fn retryable(status: u16, value: &Value) -> bool {
    let code = value["error"]["code"].as_str().unwrap_or("");
    (status == 429 && !["insufficient_quota", "billing_hard_limit_reached"].contains(&code))
        || (500..=599).contains(&status)
}
pub fn transcription_text(value: &Value) -> Result<String, String> {
    // The canonical text keeps punctuation and works without word timestamps.
    value["text"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "empty_transcription".into())
}
pub fn response_text(value: &Value) -> Result<String, String> {
    if value["status"].as_str().is_some_and(|s| s != "completed") {
        return Err("incomplete_response".into());
    }
    let blocks: Vec<&str> = value["output"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| v["type"] == "message")
        .flat_map(|v| v["content"].as_array().into_iter().flatten())
        .filter(|v| v["type"] == "output_text")
        .filter_map(|v| v["text"].as_str())
        .collect();
    let text = if blocks.is_empty() {
        value["output_text"].as_str().unwrap_or("").to_string()
    } else {
        blocks.join("\n")
    };
    if text.trim().is_empty() {
        Err("empty_response".into())
    } else {
        Ok(text.trim().into())
    }
}

impl OpenAIClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            usage_store: None,
            source_id: None,
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(90))
                .build()
                .expect("HTTP client initialization"),
        }
    }
    pub(crate) fn with_usage_store(mut self, store: std::sync::Arc<crate::db::Database>) -> Self {
        self.usage_store = Some(store);
        self
    }
    pub(crate) fn for_source(&self, source: &str) -> Self {
        Self {
            source_id: Some(source.into()),
            ..self.clone()
        }
    }
    fn persist_usage(&self, row: &crate::usage::UsageRecord) -> Result<(), String> {
        if let Some(store) = &self.usage_store {
            store
                .record_usage(row)
                .map_err(|_| "usage_storage".to_string())?;
        }
        Ok(())
    }
    fn begin_usage(&self, model: &str, stage: &str) -> Result<crate::usage::UsageRecord, String> {
        let row = crate::usage::record(model, stage, 0, None, self.source_id.as_deref());
        // Persist before sending. Cancellation or timeout remains an unknown charge.
        self.persist_usage(&row)?;
        Ok(row)
    }
    pub async fn transcribe_audio(
        &self,
        audio: Vec<f32>,
        rate: u32,
        language: Option<&str>,
        hint: Option<&str>,
    ) -> Result<String, String> {
        let languages = language.into_iter().map(str::to_string).collect::<Vec<_>>();
        self.transcribe_with_model(audio, rate, "whisper-1", &languages, hint)
            .await
    }
    pub async fn transcribe_with_model(
        &self,
        audio: Vec<f32>,
        rate: u32,
        model: &str,
        languages: &[String],
        hint: Option<&str>,
    ) -> Result<String, String> {
        if !["whisper-1", "gpt-transcribe"].contains(&model) {
            return Err("unsupported_transcription_model".into());
        }
        if rate == 0 || audio.is_empty() {
            return Err("empty_audio".into());
        }
        let duration_ms = (audio.len() as u64 * 1000 / rate as u64) as i64;
        let wav = self.audio_to_wav(audio, rate)?;
        if wav.len() > 25_000_000 {
            return Err("audio_too_large".into());
        }
        let start = Instant::now();
        for attempt in 0..3 {
            let mut form = reqwest::multipart::Form::new()
                .text("model", model.to_string())
                .text("response_format", "json")
                .part(
                    "file",
                    reqwest::multipart::Part::bytes(wav.clone())
                        .file_name("segment.wav")
                        .mime_str("audio/wav")
                        .map_err(|_| "audio_multipart".to_string())?,
                );
            if model == "gpt-transcribe" {
                for language in languages {
                    form = form.text("languages[]", language.clone());
                }
            } else if languages.len() == 1 {
                form = form.text("language", languages[0].clone());
            }
            if let Some(hint) = hint {
                form = form.text("prompt", hint.to_string());
            }
            let mut usage = self.begin_usage(model, "asr")?;
            usage.audio_ms = duration_ms;
            self.persist_usage(&usage)?;
            let response = self
                .client
                .post("https://api.openai.com/v1/audio/transcriptions")
                .bearer_auth(&self.api_key)
                .multipart(form)
                .send()
                .await
                .map_err(|e| {
                    if e.is_timeout() {
                        "api_timeout".into()
                    } else {
                        "api_connection".to_string()
                    }
                })?;
            let status = response.status().as_u16();
            let request_id = response
                .headers()
                .get("x-request-id")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            usage.status = status;
            usage.provider_request_id = request_id.clone();
            self.persist_usage(&usage)?;
            let value: Value = response
                .json()
                .await
                .map_err(|_| "api_invalid_json".to_string())?;
            self.persist_usage(&crate::usage::audio(usage, duration_ms))?;
            crate::diagnostics::event(
                "asr_request",
                json!({"model":model,"attempt":attempt,"status":status,"request_id":request_id,"elapsed_ms":start.elapsed().as_millis()}),
            );
            if status < 300 {
                return transcription_text(&value);
            }
            if attempt < 2 && retryable(status, &value) {
                tokio::time::sleep(Duration::from_millis(500 * (1 << attempt))).await;
                continue;
            }
            return Err(api_error(status, &value));
        }
        Err("api_retry_exhausted".into())
    }
    pub async fn send_prompt(
        &self,
        prompt: &str,
        model: &str,
        history: &[crate::db::ConversationMessage],
        image: Option<&str>,
    ) -> Result<String, String> {
        self.send_prompt_options(prompt, model, history, image, false)
            .await
    }
    pub async fn send_prompt_options(
        &self,
        prompt: &str,
        model: &str,
        history: &[crate::db::ConversationMessage],
        image: Option<&str>,
        web_search: bool,
    ) -> Result<String, String> {
        let model = crate::models::normalize(model);
        if model == "transcribe-only" {
            return Err("unsupported_prompt_model".into());
        }
        let mut input: Vec<Value> = history
            .iter()
            .map(|msg| json!({"role":msg.role,"content":msg.content}))
            .collect();
        if let Some(image) = image {
            let url = if image.starts_with("data:") {
                image.to_string()
            } else {
                format!("data:image/png;base64,{image}")
            };
            input.push(json!({"role":"user","content":[{"type":"input_text","text":prompt},{"type":"input_image","image_url":url}]}));
        } else {
            input.push(json!({"role":"user","content":prompt}));
        }
        let mut body = json!({"model":model,"store":false,"instructions":"You are a helpful assistant. When asked to write, rewrite, translate, or improve a message, email, or text, respond with ONLY the final text. For questions, answer normally. Never use markdown formatting or em dashes.","input":input});
        body["service_tier"] = json!("default");
        body["reasoning"] = json!({"effort":"low"});
        if web_search {
            body["tools"] = json!([{"type":"web_search"}]);
            body["tool_choice"] = json!("auto");
        }
        let start = Instant::now();
        let mut usage = self.begin_usage(model, "command")?;
        // No blind command retry: an ambiguous response may already have incurred work.
        let response = self
            .client
            .post("https://api.openai.com/v1/responses")
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    "api_timeout".into()
                } else {
                    "api_connection".to_string()
                }
            })?;
        let status = response.status().as_u16();
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        usage.status = status;
        usage.provider_request_id = request_id.clone();
        self.persist_usage(&usage)?;
        let value: Value = response
            .json()
            .await
            .map_err(|_| "api_invalid_json".to_string())?;
        self.persist_usage(&crate::usage::response(usage, &value))?;
        crate::diagnostics::event(
            "prompt_request",
            json!({"model":model,"status":status,"request_id":request_id,"elapsed_ms":start.elapsed().as_millis(),"input_tokens":value["usage"]["input_tokens"],"output_tokens":value["usage"]["output_tokens"]}),
        );
        if status >= 300 {
            return Err(api_error(status, &value));
        }
        response_text(&value)
    }

    /// Generate speech audio from text using OpenAI TTS API
    pub async fn speak_text(&self, text: &str) -> Result<Vec<u8>, String> {
        let body = json!({
            "model": "tts-1",
            "input": text,
            "voice": "nova",
            "response_format": "mp3"
        });

        let mut usage = self.begin_usage("tts-1", "tts")?;
        usage.characters = text.chars().count() as i64;
        self.persist_usage(&usage)?;
        let response = self
            .client
            .post("https://api.openai.com/v1/audio/speech")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|_| "tts_connection".to_string())?;

        usage.status = response.status().as_u16();
        usage.provider_request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        self.persist_usage(&crate::usage::speech(usage, text))?;

        if !response.status().is_success() {
            let status = response.status();
            let error = response.json().await.unwrap_or_default();
            return Err(api_error(status.as_u16(), &error));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read TTS response: {}", e))?;

        println!("✅ TTS audio received: {} bytes", bytes.len());
        Ok(bytes.to_vec())
    }

    fn audio_to_wav(&self, audio_data: Vec<f32>, sample_rate: u32) -> Result<Vec<u8>, String> {
        use std::io::Cursor;

        println!(
            "🎵 Converting audio: {} samples @ {}Hz",
            audio_data.len(),
            sample_rate
        );
        println!(
            "🎵 Duration: {:.2}s",
            audio_data.len() as f32 / sample_rate as f32
        );

        const TARGET_SAMPLE_RATE: u32 = 16_000;

        let (audio_data, wav_sample_rate) = if sample_rate > TARGET_SAMPLE_RATE {
            let resampled = self.resample_audio(&audio_data, sample_rate, TARGET_SAMPLE_RATE);
            println!(
                "Downsampled audio for upload: {}Hz -> {}Hz ({} -> {} samples)",
                sample_rate,
                TARGET_SAMPLE_RATE,
                audio_data.len(),
                resampled.len()
            );
            (resampled, TARGET_SAMPLE_RATE)
        } else {
            (audio_data, sample_rate)
        };

        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: wav_sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = hound::WavWriter::new(&mut cursor, spec)
                .map_err(|e| format!("Failed to create WAV writer: {}", e))?;

            for sample in audio_data {
                let amplitude = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                writer
                    .write_sample(amplitude)
                    .map_err(|e| format!("Failed to write sample: {}", e))?;
            }

            writer
                .finalize()
                .map_err(|e| format!("Failed to finalize WAV: {}", e))?;
        }

        let wav_data = cursor.into_inner();
        println!("✅ WAV file size: {} bytes", wav_data.len());

        Ok(wav_data)
    }

    fn resample_audio(&self, audio: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
        // Simple linear interpolation resampling
        let ratio = from_rate as f64 / to_rate as f64;
        let output_len = (audio.len() as f64 / ratio) as usize;
        let mut output = Vec::with_capacity(output_len);

        for i in 0..output_len {
            let src_idx = i as f64 * ratio;
            let idx = src_idx as usize;

            if idx + 1 < audio.len() {
                let frac = src_idx - idx as f64;
                let sample = audio[idx] * (1.0 - frac as f32) + audio[idx + 1] * frac as f32;
                output.push(sample);
            } else if idx < audio.len() {
                output.push(audio[idx]);
            }
        }

        output
    }
}

/// Split text into chunks at sentence boundaries for chunked TTS playback.
/// Groups short sentences together (target ~200 chars, max ~400).
pub fn split_into_tts_chunks(text: &str) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return vec![];
    }

    // Split at sentence-ending punctuation followed by space or end
    let mut sentences: Vec<String> = Vec::new();
    let mut current = String::new();

    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();

    for i in 0..len {
        current.push(chars[i]);
        let is_sentence_end = (chars[i] == '.' || chars[i] == '!' || chars[i] == '?')
            && (i + 1 >= len || chars[i + 1] == ' ' || chars[i + 1] == '\n');

        if is_sentence_end {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                sentences.push(trimmed);
            }
            current.clear();
        }
    }
    // Remaining text (no final punctuation)
    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        sentences.push(trimmed);
    }

    // Group sentences into chunks (~200 chars target, ~400 max)
    let mut chunks: Vec<String> = Vec::new();
    let mut chunk = String::new();

    for sentence in sentences {
        if chunk.is_empty() {
            chunk = sentence;
        } else if chunk.len() + sentence.len() + 1 <= 400 {
            chunk.push(' ');
            chunk.push_str(&sentence);
        } else {
            chunks.push(chunk);
            chunk = sentence;
        }
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }

    // If no sentences were found (no punctuation), just return the whole text
    if chunks.is_empty() {
        chunks.push(text.to_string());
    }

    println!(
        "📝 TTS chunks: {} chunks from {} chars",
        chunks.len(),
        text.len()
    );
    for (i, c) in chunks.iter().enumerate() {
        println!("  Chunk {}: {} chars", i + 1, c.len());
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "Sends two small, paid synthetic commands using the configured API key"]
    async fn live_command_models_record_provider_usage() {
        if std::env::var("OPENAI_API_KEY").is_err() {
            let directory = dirs::data_local_dir().unwrap();
            dotenv::from_path(directory.join("com.dicta.app/.env")).ok();
        }
        let store = std::sync::Arc::new(crate::db::Database::new(":memory:".into()).unwrap());
        let client =
            OpenAIClient::new(std::env::var("OPENAI_API_KEY").expect("API key unavailable"))
                .with_usage_store(store.clone())
                .for_source("synthetic-model-smoke");
        for model in ["gpt-6-luna", "gpt-6.1-sol"] {
            let text = client
                .send_prompt_options("Responda apenas: Teste concluído.", model, &[], None, false)
                .await
                .unwrap();
            assert!(!text.is_empty());
        }
        let summary = store.usage_summary(0, i64::MAX).unwrap();
        println!("{}", serde_json::to_string(&summary).unwrap());
        assert_eq!(summary.requests, 2);
        assert!(summary.input_tokens > 0 && summary.output_tokens > 0);
        assert_eq!(summary.unknown_requests, 0);
    }
    #[test]
    fn text_without_words_keeps_punctuation() {
        assert_eq!(
            transcription_text(&json!({"text":"Olá, Rapha. Não, trinta!"})).unwrap(),
            "Olá, Rapha. Não, trinta!"
        );
    }
    #[test]
    fn empty_transcripts_are_failures() {
        assert!(transcription_text(&json!({"text":" "})).is_err());
    }
    #[test]
    fn all_output_blocks_are_preserved() {
        assert_eq!(response_text(&json!({"status":"completed","output":[{"type":"reasoning","content":[]},{"type":"message","content":[{"type":"output_text","text":"Primeiro."},{"type":"output_text","text":"Segundo."}]}]})).unwrap(),"Primeiro.\nSegundo.");
    }
    #[test]
    fn incomplete_response_is_not_delivered() {
        assert!(response_text(&json!({"status":"incomplete","output_text":"partial"})).is_err());
    }
    #[test]
    fn quota_and_auth_are_not_retried() {
        assert!(!retryable(
            429,
            &json!({"error":{"code":"insufficient_quota"}})
        ));
        assert!(!retryable(401, &json!({})));
        assert!(retryable(503, &json!({})));
    }
    #[test]
    fn api_errors_never_include_secret_messages() {
        let e = api_error(
            401,
            &json!({"error":{"code":"invalid_api_key","message":"secret token"}}),
        );
        assert!(!e.contains("secret"));
    }
    #[test]
    fn wav_resampling_preserves_duration() {
        let client = OpenAIClient::new("test".into());
        for rate in [16000, 44100, 48000] {
            let bytes = client.audio_to_wav(vec![0.1; rate as usize], rate).unwrap();
            let reader = hound::WavReader::new(std::io::Cursor::new(bytes)).unwrap();
            assert_eq!(reader.spec().sample_rate, 16000);
            assert!((reader.duration() as i64 - 16000).abs() <= 1);
        }
    }
}
