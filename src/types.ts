export interface HistoryEntry {
  id: number;
  text: string;
  timestamp: number;
  duration_ms: number | null;
  model: string | null;
  mode: string | null;
}
export interface QueueItem {
  id: number;
  mode: string;
  audio_path: string | null;
  prompt_text: string | null;
  model: string;
  created_at: number;
  retry_count: number;
  last_error: string | null;
  auto_retry_disabled: boolean;
}
export interface Stats {
  total_words: number;
  total_transcriptions: number;
  total_duration_ms: number;
}
export interface UsageRecord {
  request_id: string;
  provider_request_id: string | null;
  source_id: string | null;
  timestamp: number;
  stage: string;
  model: string;
  status: number;
  input_tokens: number;
  cached_tokens: number;
  cache_write_tokens: number;
  output_tokens: number;
  reasoning_tokens: number;
  audio_ms: number;
  characters: number;
  search_calls: number;
  cost_nanos: number | null;
  basis: string;
  price_version: string;
}
export interface Usage {
  requests: number;
  failed_requests: number;
  unknown_requests: number;
  input_tokens: number;
  cached_tokens: number;
  output_tokens: number;
  audio_ms: number;
  characters: number;
  search_calls: number;
  cost_nanos: number;
  legacy_entries: number;
  by_stage: {
    stage: string;
    requests: number;
    cost_nanos: number;
    unknown_requests: number;
  }[];
  recent: UsageRecord[];
}
export const commandModels = [
  { value: "gpt-6.1-sol", label: "GPT-6.1 Sol" },
  { value: "gpt-6-luna", label: "GPT-6 Luna · leve" },
];
