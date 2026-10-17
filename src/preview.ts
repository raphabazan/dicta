import { initialSession, type SessionSnapshot } from "./session";
import type { UsageRecord } from "./types";

const now = Date.now();
const scenario = new URLSearchParams(location.search).get("preview");
const empty = scenario === "empty";
const sample =
  "Vamos organizar a reunião de amanhã. Primeiro, revisar os resultados da semana e separar as decisões que precisam de acompanhamento. Depois, confirmar os responsáveis e os prazos de cada entrega.";
let session: SessionSnapshot =
  new URLSearchParams(location.search).get("preview") === "recording"
    ? {
        ...initialSession,
        id: "preview",
        generation: 1,
        sequence: 1,
        phase: "recording",
        text: sample,
        duration_ms: 142000,
        completed: 6,
        total: 7,
        segmented: true,
      }
    : { ...initialSession };
const settings: Record<string, unknown> = {
  get_segment_config: { enabled: true, target_seconds: 20, silence_ms: 500 },
  get_transcription_languages: ["pt", "en"],
  get_game_mode_enabled: true,
  get_game_mode_processes: ["cs2.exe", "valorant.exe"],
  get_asr_model: "whisper-1",
  get_web_search_enabled: true,
  get_tts_enabled: false,
  get_user_prompt_model: "gpt-6.1-sol",
  get_selected_prompt_model: "transcribe-only",
  get_build_info: "0.5.1 · prévia de desenvolvimento",
  list_microphones: ["Microfone do sistema", "Microfone USB"],
  get_selected_microphone: "Microfone do sistema",
  get_shortcut_errors: [],
};
const history = Array.from({ length: empty ? 0 : 120 }, (_, i) => ({
  id: 120 - i,
  text: i === 1 ? sample.repeat(12) : sample,
  timestamp: now - i * 3600000,
  duration_ms: 45000,
  model: i % 2 ? "gpt-6.1-sol" : "whisper-1",
  mode: i % 2 ? "prompt" : "transcription",
}));
let recoveries = empty
  ? []
  : [
      {
        id: "preview-recovery",
        status: "recoverable",
        created_at: now - 3600000,
        duration_ms: 68000,
      },
    ];
let queue = empty
  ? []
  : [
      {
        id: 1,
        mode: "text-prompt",
        audio_path: null,
        prompt_text: sample.repeat(8),
        model: "gpt-6-luna",
        created_at: now - 7200000,
        retry_count: 1,
        last_error: "API error (429): insufficient_quota",
        auto_retry_disabled: true,
      },
    ];
const usage: UsageRecord[] = empty
  ? []
  : [
      {
        request_id: "preview-1",
        provider_request_id: "req-preview",
        source_id: "preview",
        timestamp: now,
        stage: "command",
        model: "gpt-6.1-sol",
        status: 200,
        input_tokens: 3210,
        cached_tokens: 1024,
        cache_write_tokens: 0,
        output_tokens: 360,
        reasoning_tokens: 100,
        audio_ms: 0,
        characters: 0,
        search_calls: 1,
        cost_nanos: 18074400,
        basis: "provider_tokens",
        price_version: "openai-standard-2026-10-17",
      },
      {
        request_id: "preview-2",
        provider_request_id: null,
        source_id: "preview",
        timestamp: now - 15000,
        stage: "asr",
        model: "whisper-1",
        status: 200,
        input_tokens: 0,
        cached_tokens: 0,
        cache_write_tokens: 0,
        output_tokens: 0,
        reasoning_tokens: 0,
        audio_ms: 20500,
        characters: 0,
        search_calls: 0,
        cost_nanos: 2050000,
        basis: "audio_duration",
        price_version: "openai-standard-2026-10-17",
      },
    ];
if (scenario === "unknown") {
  usage[1].status = 0;
  usage[1].cost_nanos = null;
  usage[1].basis = "unknown";
}
const listeners = new Map<string, Set<(value: never) => void>>();
export function on<T>(name: string, handler: (value: T) => void) {
  const set = listeners.get(name) ?? new Set();
  set.add(handler as (value: never) => void);
  listeners.set(name, set);
  return () => {
    set.delete(handler as (value: never) => void);
  };
}
function emit(name: string, payload: unknown) {
  listeners.get(name)?.forEach((handler) => handler(payload as never));
}
export async function call(
  command: string,
  args: Record<string, unknown> = {},
): Promise<unknown> {
  if (command in settings) return settings[command];
  if (command === "get_session_state") return session;
  if (command === "get_transcription_history")
    return history.slice(
      Number(args.offset ?? 0),
      Number(args.offset ?? 0) + Number(args.limit ?? 100),
    );
  if (command === "get_recoverable_sessions") return recoveries;
  if (command === "get_queue_items") return queue;
  if (command === "get_statistics")
    return {
      total_words: 12740,
      total_transcriptions: 120,
      total_duration_ms: 5400000,
    };
  if (command === "get_usage_summary")
    return {
      requests: usage.length,
      unknown_requests: usage.filter((r) => r.cost_nanos === null).length,
      failed_requests: usage.filter((r) => r.status === 0).length,
      input_tokens: 3210,
      cached_tokens: 1024,
      output_tokens: 360,
      audio_ms: 20500,
      characters: 0,
      search_calls: 1,
      cost_nanos: usage.reduce((sum, r) => sum + (r.cost_nanos ?? 0), 0),
      legacy_entries: empty ? 0 : 118,
      by_stage: usage.map((r) => ({
        stage: r.stage,
        requests: 1,
        cost_nanos: r.cost_nanos ?? 0,
        unknown_requests: r.cost_nanos === null ? 1 : 0,
      })),
      recent: usage,
    };
  if (command === "start_recording_audio")
    session = {
      ...initialSession,
      id: "preview",
      generation: session.generation + 1,
      sequence: 1,
      phase: "recording",
      segmented: true,
    };
  else if (command === "stop_recording_audio")
    session = {
      ...session,
      sequence: session.sequence + 1,
      phase: "ready",
      text: session.text || sample,
      duration_ms: 142000,
    };
  else if (command === "cancel_recording")
    session = {
      ...session,
      sequence: session.sequence + 1,
      phase: "cancelled",
    };
  else if (command.startsWith("set_")) {
    const getter = command.replace("set_", "get_");
    settings[getter] =
      args.config ??
      args.model ??
      args.deviceName ??
      args.languages ??
      args.processes ??
      args.enabled;
    if (
      command === "set_selected_prompt_model" &&
      args.saveAsDefault &&
      args.model !== "transcribe-only"
    )
      settings.get_user_prompt_model = args.model;
    return settings[getter];
  } else if (command === "discard_session" || command === "retry_session") {
    recoveries = [];
    emit("sessions-updated", null);
  } else if (
    command === "delete_single_queue_item" ||
    command === "retry_single_queue_item"
  ) {
    queue = [];
    emit("queue-updated", 0);
  } else if (command === "copy_to_clipboard") return undefined;
  else if (command === "recover_runtime" || command === "stop_tts_playback")
    return undefined;
  else throw new Error(`Prévia sem ação: ${command}`);
  emit("session-state", session);
}
