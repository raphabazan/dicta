//! Explicit, opt-in smoke test. Use a dedicated synthetic WAV, never history audio.
use dicta_lib::openai::OpenAIClient;
use serde_json::json;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = std::env::args().collect();
    let file = arguments
        .get(1)
        .ok_or("Usage: api_smoke <synthetic.wav> [output.json]")?;
    if std::env::var("OPENAI_API_KEY").is_err() {
        let directory = dirs::data_local_dir().ok_or("Missing app data directory")?;
        let _ = dotenv::from_path(directory.join("com.dicta.app/.env"));
    }
    let client =
        OpenAIClient::new(std::env::var("OPENAI_API_KEY").map_err(|_| "API key unavailable")?);
    let reader = hound::WavReader::open(file)?;
    let spec = reader.spec();
    if spec.channels != 1
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
    {
        return Err("Use mono PCM16 test audio".into());
    }
    let audio = reader
        .into_samples::<i16>()
        .map(|s| s.map(|v| v as f32 / i16::MAX as f32))
        .collect::<Result<Vec<_>, _>>()?;
    let mut results = Vec::new();
    for model in ["whisper-1", "gpt-transcribe"] {
        let start = Instant::now();
        let result = client
            .transcribe_with_model(audio.clone(), spec.sample_rate, model, &["pt".into()], None)
            .await;
        results.push(match result{Ok(text)=>json!({"operation":"transcription","model":model,"elapsed_ms":start.elapsed().as_millis(),"text":text}),Err(error)=>json!({"operation":"transcription","model":model,"elapsed_ms":start.elapsed().as_millis(),"error":error})});
    }
    for model in ["gpt-6-luna", "gpt-6.1-sol"] {
        let start = Instant::now();
        let result = client
            .send_prompt_options(
                "Responda apenas com esta frase, sem acrescentar nada: O teste do Dicta terminou.",
                model,
                &[],
                None,
                false,
            )
            .await;
        results.push(match result{Ok(text)=>json!({"operation":"command","model":model,"elapsed_ms":start.elapsed().as_millis(),"text":text}),Err(error)=>json!({"operation":"command","model":model,"elapsed_ms":start.elapsed().as_millis(),"error":error})});
    }
    let output = json!({"scope":"Synthetic smoke only; not an accuracy benchmark or default promotion","duration_ms":audio.len() as u64*1000/spec.sample_rate as u64,"results":results});
    let formatted = serde_json::to_string_pretty(&output)?;
    if let Some(file) = arguments.get(2) {
        std::fs::write(file, &formatted)?;
    }
    println!("{formatted}");
    if output["results"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r.get("error").is_some())
    {
        std::process::exit(1);
    }
    Ok(())
}
