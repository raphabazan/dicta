use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Serialize, Deserialize)]
pub struct TranscriptionResponse {
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VerboseTranscriptionResponse {
    pub text: String,
    #[serde(default)]
    pub words: Vec<WordSegment>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WordSegment {
    pub word: String,
    pub start: f64,
    pub end: f64,
    #[serde(default)]
    pub probability: Option<f64>,
}

pub struct OpenAIClient {
    api_key: String,
    client: reqwest::Client,
}

impl OpenAIClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }

    /// Transcribe audio using Whisper API with confidence filtering
    pub async fn transcribe_audio(&self, audio_data: Vec<f32>, sample_rate: u32) -> Result<String, String> {
        println!("🔄 Transcribing audio... ({} samples at {}Hz)", audio_data.len(), sample_rate);

        // Convert f32 audio to WAV format
        let wav_data = self.audio_to_wav(audio_data, sample_rate)?;

        // Call Whisper API with Portuguese language hint and verbose_json for word-level confidence
        let form = reqwest::multipart::Form::new()
            .text("model", "whisper-1")
            .text("language", "pt")
            .text("response_format", "verbose_json")
            .text("timestamp_granularities[]", "word")
            .part(
                "file",
                reqwest::multipart::Part::bytes(wav_data)
                    .file_name("audio.wav")
                    .mime_str("audio/wav")
                    .map_err(|e| format!("Failed to create multipart: {}", e))?,
            );

        let response = self
            .client
            .post("https://api.openai.com/v1/audio/transcriptions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
            .send()
            .await
            .map_err(|e| format!("Failed to send request: {}", e))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(format!("API error: {}", error_text));
        }

        let result: VerboseTranscriptionResponse = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        // Filter words by confidence threshold (0.7 = 70%)
        let confidence_threshold = 0.7;
        let filtered_words: Vec<String> = result.words
            .iter()
            .filter(|w| {
                if let Some(prob) = w.probability {
                    if prob < confidence_threshold {
                        println!("⚠️ Low confidence ({:.2}%): '{}'", prob * 100.0, w.word);
                        false
                    } else {
                        true
                    }
                } else {
                    true // Keep if no probability (fallback)
                }
            })
            .map(|w| w.word.clone())
            .collect();

        let filtered_text = filtered_words.join(" ");

        println!("📊 Original: {} words", result.words.len());
        println!("📊 Filtered: {} words (threshold: {:.0}%)", filtered_words.len(), confidence_threshold * 100.0);
        println!("✅ Transcription: {}", filtered_text);

        Ok(filtered_text)
    }

    /// Post-process text with GPT-4o-mini
    pub async fn post_process(&self, raw_text: &str) -> Result<String, String> {
        println!("🤖 Post-processing with GPT-4o-mini...");

        let prompt = format!(
            "You are a text post-processor. Clean up this voice transcription:\n\
            - Fix grammar and punctuation\n\
            - Remove filler words (um, uh, like, you know)\n\
            - DO NOT change the meaning\n\
            - Output ONLY the cleaned text, nothing else\n\n\
            Raw transcript: {}",
            raw_text
        );

        let body = json!({
            "model": "gpt-4o-mini",
            "messages": [
                {"role": "system", "content": "You are a helpful assistant that cleans up voice transcriptions."},
                {"role": "user", "content": prompt}
            ],
            "temperature": 0.3
        });

        let response = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Failed to send request: {}", e))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(format!("API error: {}", error_text));
        }

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        let processed_text = result["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string();

        println!("✅ Processed text: {}", processed_text);
        Ok(processed_text)
    }

    /// Send prompt to GPT model and get response with web search enabled
    /// history: previous (user, assistant) pairs in chronological order
    pub async fn send_prompt(&self, prompt: &str, model: &str, history: &[crate::db::ConversationMessage], image_data: Option<&str>) -> Result<String, String> {
        println!("🤖 Sending prompt to {} (history: {} messages, image: {})...", model, history.len(), image_data.is_some());
        println!("📝 Prompt: {}", prompt);

        // Map model names to their correct identifiers
        let api_model = match model {
            "gpt-4o-mini" => "gpt-4o-mini",
            "gpt-4o" => "gpt-4.1",
            "gpt-4.1" => "gpt-4.1",
            _ => model
        };

        let system_prompt = "You are a helpful assistant. When the user asks you to write, rewrite, translate, or improve a message, email, or text, respond with ONLY the final text, no introduction, no explanation. If the request is a question or needs an explanation, answer normally. Never use markdown formatting in your responses. Never use em dashes in your responses.";

        // Build input array: history messages + current prompt
        let mut input: Vec<serde_json::Value> = history.iter().map(|msg| {
            json!({
                "role": msg.role,
                "content": msg.content
            })
        }).collect();

        // Build user message: multimodal if image attached, plain text otherwise
        if let Some(img) = image_data {
            let mut content = vec![
                json!({"type": "input_text", "text": prompt})
            ];
            // Add image with data URI
            let image_url = if img.starts_with("data:") {
                img.to_string()
            } else {
                format!("data:image/png;base64,{}", img)
            };
            content.push(json!({"type": "input_image", "image_url": image_url}));
            input.push(json!({
                "role": "user",
                "content": content
            }));
            println!("🖼️ Image attached ({} bytes base64)", img.len());
        } else {
            input.push(json!({
                "role": "user",
                "content": prompt
            }));
        }

        let body = json!({
            "model": api_model,
            "tools": [
                {"type": "web_search"}
            ],
            "tool_choice": "auto",
            "instructions": system_prompt,
            "input": input
        });

        let response = self
            .client
            .post("https://api.openai.com/v1/responses")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Failed to send request: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(format!("API error ({}): {}", status, error_text));
        }

        let result: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        // Log if web search was used
        if let Some(outputs) = result["output"].as_array() {
            for output in outputs {
                if output["type"] == "web_search_call" {
                    println!("🌐 Web search was used for this query");
                    if let Some(action) = output.get("action") {
                        println!("🔍 Search action: {:?}", action);
                    }
                }
            }
        }

        // Extract output_text from Responses API response
        let response_text = result["output_text"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string();

        if response_text.is_empty() {
            // Fallback: try to extract from output array
            if let Some(outputs) = result["output"].as_array() {
                for output in outputs {
                    if output["type"] == "message" {
                        if let Some(content) = output["content"].as_array() {
                            for item in content {
                                if item["type"] == "output_text" {
                                    let text = item["text"].as_str().unwrap_or("").trim();
                                    if !text.is_empty() {
                                        println!("✅ Response from {} (web search): {}", model, text);
                                        return Ok(text.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            return Err("No response text found in API response".to_string());
        }

        println!("✅ Response from {} (web search): {}", model, response_text);
        Ok(response_text)
    }

    /// Generate speech audio from text using OpenAI TTS API
    pub async fn speak_text(&self, text: &str) -> Result<Vec<u8>, String> {
        println!("🔊 Generating TTS for: {}...", &text[..text.len().min(80)]);

        let body = json!({
            "model": "tts-1",
            "input": text,
            "voice": "nova",
            "response_format": "mp3"
        });

        let response = self
            .client
            .post("https://api.openai.com/v1/audio/speech")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("TTS request failed: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(format!("TTS API error ({}): {}", status, error_text));
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

        println!("🎵 Converting audio: {} samples @ {}Hz", audio_data.len(), sample_rate);
        println!("🎵 Duration: {:.2}s", audio_data.len() as f32 / sample_rate as f32);

        // Keep original sample rate - Whisper handles various rates well
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
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

    println!("📝 TTS chunks: {} chunks from {} chars", chunks.len(), text.len());
    for (i, c) in chunks.iter().enumerate() {
        println!("  Chunk {}: {} chars", i + 1, c.len());
    }

    chunks
}
