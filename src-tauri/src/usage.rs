//! No content is stored here. Monetary amounts are integer billionths of USD.
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PRICE_VERSION: &str = "openai-standard-2026-10-17";
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UsageRecord {
    pub request_id: String,
    pub provider_request_id: Option<String>,
    pub source_id: Option<String>,
    pub timestamp: i64,
    pub stage: String,
    pub model: String,
    pub status: u16,
    pub input_tokens: i64,
    pub cached_tokens: i64,
    pub cache_write_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub audio_ms: i64,
    pub characters: i64,
    pub search_calls: i64,
    pub cost_nanos: Option<i64>,
    pub basis: String,
    pub price_version: String,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct UsageSummary {
    pub requests: i64,
    pub failed_requests: i64,
    pub unknown_requests: i64,
    pub input_tokens: i64,
    pub cached_tokens: i64,
    pub output_tokens: i64,
    pub audio_ms: i64,
    pub characters: i64,
    pub search_calls: i64,
    pub cost_nanos: i64,
    pub legacy_entries: i64,
    pub by_stage: Vec<UsageStage>,
    pub recent: Vec<UsageRecord>,
}
#[derive(Clone, Debug, Serialize)]
pub struct UsageStage {
    pub stage: String,
    pub requests: i64,
    pub cost_nanos: i64,
    pub unknown_requests: i64,
}

fn number(value: &Value) -> i64 {
    value.as_i64().unwrap_or(0).max(0)
}
pub fn record(
    model: &str,
    stage: &str,
    status: u16,
    request_id: Option<String>,
    source: Option<&str>,
) -> UsageRecord {
    UsageRecord {
        model: model.into(),
        stage: stage.into(),
        status,
        request_id: request_id.unwrap_or_else(crate::dictation::new_id),
        source_id: source.map(str::to_string),
        timestamp: crate::now_ms(),
        price_version: PRICE_VERSION.into(),
        basis: "unknown".into(),
        ..UsageRecord::default()
    }
}

pub fn response(mut row: UsageRecord, value: &Value) -> UsageRecord {
    let usage = &value["usage"];
    row.input_tokens = number(&usage["input_tokens"]);
    row.cached_tokens = number(&usage["input_tokens_details"]["cached_tokens"]);
    row.cache_write_tokens = number(&usage["input_tokens_details"]["cache_write_tokens"]);
    row.output_tokens = number(&usage["output_tokens"]);
    row.reasoning_tokens = number(&usage["output_tokens_details"]["reasoning_tokens"]);
    row.search_calls = value["output"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| v["type"] == "web_search_call")
        .count() as i64;
    let valid = [
        &usage["input_tokens"],
        &usage["output_tokens"],
        &usage["input_tokens_details"]["cached_tokens"],
        &usage["input_tokens_details"]["cache_write_tokens"],
    ]
    .iter()
    .all(|v| v.as_i64().is_some_and(|n| n >= 0));
    if valid && row.cached_tokens + row.cache_write_tokens <= row.input_tokens {
        row.basis = "provider_tokens_unpriced".into();
        let rates = match row.model.as_str() {
            "gpt-6.1-sol" => Some((2000, 100, 2500, 10000)),
            "gpt-6-luna" => Some((100, 10, 125, 500)),
            _ => None,
        };
        // The app requests Standard processing. A different returned tier is explicit unknown.
        let tier = value["service_tier"].as_str().unwrap_or("default");
        if let Some((mut input, mut cached, mut writes, mut output)) =
            rates.filter(|_| ["default", "standard"].contains(&tier))
        {
            if row.input_tokens > 272_000 {
                input *= 2;
                cached *= 2;
                writes *= 2;
                output = output * 3 / 2;
            }
            row.cost_nanos = Some(
                (row.input_tokens - row.cached_tokens - row.cache_write_tokens) * input
                    + row.cached_tokens * cached
                    + row.cache_write_tokens * writes
                    + row.output_tokens * output
                    + row.search_calls * 10_000_000,
            );
            row.basis = "provider_tokens".into();
        }
    }
    row
}
pub fn audio(mut row: UsageRecord, milliseconds: i64) -> UsageRecord {
    row.audio_ms = milliseconds.max(0);
    if (200..300).contains(&row.status) {
        row.cost_nanos = match row.model.as_str() {
            "whisper-1" => Some(row.audio_ms * 100),
            "gpt-transcribe" => Some(row.audio_ms * 75),
            _ => None,
        };
        row.basis = "audio_duration".into();
    }
    row
}
pub fn speech(mut row: UsageRecord, text: &str) -> UsageRecord {
    row.characters = text.chars().count() as i64;
    if (200..300).contains(&row.status) && row.model == "tts-1" {
        row.cost_nanos = Some(row.characters * 15_000);
        row.basis = "characters".into();
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn counts_full_input_cache_output_and_search_without_double_counting_reasoning() {
        let row = response(
            record("gpt-6.1-sol", "command", 200, None, None),
            &json!({"usage":{"input_tokens":1000,"input_tokens_details":{"cached_tokens":400,"cache_write_tokens":0},"output_tokens":100,"output_tokens_details":{"reasoning_tokens":50}},"output":[{"type":"web_search_call"}]}),
        );
        assert_eq!(row.cost_nanos, Some(12_240_000));
        assert_eq!(row.reasoning_tokens, 50);
    }
    #[test]
    fn long_context_prices_apply_to_the_entire_request() {
        let row = response(
            record("gpt-6-luna", "command", 200, None, None),
            &json!({"usage":{"input_tokens":300000,"input_tokens_details":{"cached_tokens":0,"cache_write_tokens":0},"output_tokens":1000}}),
        );
        assert_eq!(row.cost_nanos, Some(60_750_000));
    }
    #[test]
    fn tiny_luna_requests_do_not_round_down_to_zero() {
        let row = response(
            record("gpt-6-luna", "command", 200, None, None),
            &json!({"usage":{"input_tokens":1,"input_tokens_details":{"cached_tokens":0,"cache_write_tokens":0},"output_tokens":1}}),
        );
        assert_eq!(row.cost_nanos, Some(600));
    }
    #[test]
    fn missing_usage_and_unknown_tiers_are_not_zero_cost() {
        assert_eq!(
            response(record("gpt-6-luna", "command", 200, None, None), &json!({})).cost_nanos,
            None
        );
        assert_eq!(
            response(
                record("gpt-6-luna", "command", 200, None, None),
                &json!({"service_tier":"priority","usage":{"input_tokens":100,"input_tokens_details":{"cached_tokens":0,"cache_write_tokens":0},"output_tokens":10}})
            )
            .cost_nanos,
            None
        );
    }
    #[test]
    fn audio_counts_each_uploaded_millisecond_including_overlap() {
        assert_eq!(
            audio(record("whisper-1", "asr", 200, None, None), 60_500).cost_nanos,
            Some(6_050_000)
        );
        assert_eq!(
            audio(record("gpt-transcribe", "asr", 200, None, None), 60_000).cost_nanos,
            Some(4_500_000)
        );
        assert_eq!(
            audio(record("gpt-transcribe", "asr", 503, None, None), 60_000).cost_nanos,
            None
        );
    }
    #[test]
    fn speech_uses_unicode_characters_instead_of_utf8_bytes() {
        assert_eq!(
            speech(record("tts-1", "tts", 200, None, None), "Olá 🎙").cost_nanos,
            Some(75_000)
        );
    }
    #[test]
    fn cache_writes_are_priced_separately_and_invalid_usage_stays_unknown() {
        let mut value = json!({"usage":{"input_tokens":100,"input_tokens_details":{"cached_tokens":30,"cache_write_tokens":20},"output_tokens":10}});
        assert_eq!(
            response(record("gpt-6.1-sol", "command", 200, None, None), &value).cost_nanos,
            Some(253_000)
        );
        for details in [
            json!({"cached_tokens":30}),
            json!({"cached_tokens":-1,"cache_write_tokens":0}),
            json!({"cached_tokens":99,"cache_write_tokens":20}),
        ] {
            value["usage"]["input_tokens_details"] = details;
            assert_eq!(
                response(record("gpt-6.1-sol", "command", 200, None, None), &value).cost_nanos,
                None
            );
        }
    }
}
