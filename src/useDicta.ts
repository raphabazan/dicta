import { useCallback, useEffect, useRef, useState } from "react";
import { call, on } from "./desktop";
import {
  initialSession,
  reconcileSession,
  deliveryMessage,
  type RecoverySession,
  type SegmentConfig,
  type SessionSnapshot,
} from "./session";
import { errorMessage } from "./format";
import {
  playStartSound,
  playStopSound,
  playCancelSound,
  playResponseSound,
} from "./sounds";
import type { HistoryEntry, QueueItem } from "./types";

export function useDicta() {
  const [session, setSession] = useState(initialSession);
  const sessionRef = useRef(initialSession);
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const [pending, setPending] = useState<Set<string>>(new Set());
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [hasMore, setHasMore] = useState(false);
  const historyGeneration = useRef(0);
  const paging = useRef(false);
  const [queue, setQueue] = useState<QueueItem[]>([]);
  const [recoveries, setRecoveries] = useState<RecoverySession[]>([]);
  const [preferencesLoaded, setPreferencesLoaded] = useState(false);
  const [prefs, setPrefs] = useState({
    segments: {
      enabled: true,
      target_seconds: 20,
      silence_ms: 500,
    } as SegmentConfig,
    asr: "whisper-1",
    languages: ["pt", "en"],
    game: true,
    processes: [] as string[],
    web: true,
    tts: false,
    model: "gpt-6.1-sol",
    selectedModel: "transcribe-only",
    microphone: "",
    microphones: [] as string[],
    build: "",
    shortcuts: [] as string[],
  });
  const action = async <T>(
    key: string,
    operation: () => Promise<T>,
    success?: string,
  ): Promise<T | undefined> => {
    setPending((previous) => new Set(previous).add(key));
    setError("");
    try {
      const result = await operation();
      if (success) setMessage(success);
      return result;
    } catch (reason) {
      setError(errorMessage(reason));
      return undefined;
    } finally {
      setPending((previous) => {
        const next = new Set(previous);
        next.delete(key);
        return next;
      });
    }
  };
  const loadHistory = useCallback(async () => {
    const generation = ++historyGeneration.current;
    const items = await call<HistoryEntry[]>("get_transcription_history", {
      limit: 100,
      offset: 0,
    });
    if (generation === historyGeneration.current) {
      setHistory(items);
      setHasMore(items.length === 100);
    }
  }, []);
  const loadQueue = useCallback(async () => {
    const [items, sessions] = await Promise.all([
      call<QueueItem[]>("get_queue_items"),
      call<RecoverySession[]>("get_recoverable_sessions"),
    ]);
    setQueue(items);
    setRecoveries(sessions);
  }, []);
  const receiveSession = useCallback((next: SessionSnapshot) => {
    const accepted = reconcileSession(sessionRef.current, next);
    sessionRef.current = accepted;
    setSession(accepted);
  }, []);
  const loadPreferences = useCallback(async () => {
    const [
      segments,
      asr,
      languages,
      game,
      processes,
      web,
      tts,
      model,
      selectedModel,
      microphone,
      microphones,
      build,
      shortcuts,
    ] = await Promise.all([
      call<SegmentConfig>("get_segment_config"),
      call<string>("get_asr_model"),
      call<string[]>("get_transcription_languages"),
      call<boolean>("get_game_mode_enabled"),
      call<string[]>("get_game_mode_processes"),
      call<boolean>("get_web_search_enabled"),
      call<boolean>("get_tts_enabled"),
      call<string>("get_user_prompt_model"),
      call<string>("get_selected_prompt_model"),
      call<string | null>("get_selected_microphone"),
      call<string[]>("list_microphones"),
      call<string>("get_build_info"),
      call<string[]>("get_shortcut_errors"),
    ]);
    setPrefs({
      segments,
      asr,
      languages,
      game,
      processes,
      web,
      tts,
      model,
      selectedModel,
      microphone: microphone ?? "",
      microphones,
      build,
      shortcuts,
    });
    setPreferencesLoaded(true);
  }, []);
  useEffect(() => {
    let disposed = false;
    const safe = (operation: () => Promise<unknown>) => {
      if (!disposed)
        void operation().catch((reason) => {
          if (!disposed) setError(errorMessage(reason));
        });
    };
    safe(loadHistory);
    safe(loadQueue);
    safe(loadPreferences);
    const subscriptions = [
      on<SessionSnapshot>("session-state", (next) => {
        if (!disposed) receiveSession(next);
      }),
      on("history-updated", () => safe(loadHistory)),
      on("queue-updated", () => safe(loadQueue)),
      on("sessions-updated", () => safe(loadQueue)),
      on("runtime-resumed", () => {
        safe(async () =>
          receiveSession(await call<SessionSnapshot>("get_session_state")),
        );
        safe(loadQueue);
      }),
      on<{ code: string }>("delivery-result", (result) => {
        if (!disposed) setMessage(deliveryMessage(result.code));
      }),
      on<boolean>("tts-toggled", (tts) => {
        if (!disposed) setPrefs((previous) => ({ ...previous, tts }));
      }),
      on<string>("model-selected", (selectedModel) => {
        if (!disposed) {
          setPrefs((previous) => ({ ...previous, selectedModel }));
          safe(async () => {
            const model = await call<string>("get_user_prompt_model");
            if (!disposed) setPrefs((previous) => ({ ...previous, model }));
          });
        }
      }),
      on<string>("recording-error", (reason) => {
        if (!disposed) setError(errorMessage(reason));
      }),
      on("recording-sound-start", () => {
        if (!disposed) playStartSound();
      }),
      on("recording-sound-stop", () => {
        if (!disposed) playStopSound();
      }),
      on("recording-cancelled", () => {
        if (!disposed) playCancelSound();
      }),
      on("response-ready", () => {
        if (!disposed) playResponseSound();
      }),
    ];
    void subscriptions[0]
      .then(() => {
        if (!disposed)
          safe(async () =>
            receiveSession(await call<SessionSnapshot>("get_session_state")),
          );
      })
      .catch(() => {});
    return () => {
      disposed = true;
      subscriptions.forEach((subscription) => {
        void subscription.then((unlisten) => unlisten()).catch(() => {});
      });
    };
  }, [loadHistory, loadQueue, loadPreferences, receiveSession]);

  const save = <K extends keyof typeof prefs>(
    key: K,
    command: string,
    args: Record<string, unknown>,
    value: (typeof prefs)[K],
  ) =>
    action(
      `setting:${key}`,
      async () => {
        await call(command, args);
        setPrefs((previous) => ({ ...previous, [key]: value }));
      },
      "Configuração salva.",
    );
  const selectModel = (model: string) =>
    action("model", async () => {
      await call("set_selected_prompt_model", {
        model,
        saveAsDefault: model !== "transcribe-only",
      });
      setPrefs((previous) => ({
        ...previous,
        selectedModel: model,
        ...(model !== "transcribe-only" ? { model } : {}),
      }));
    });
  const copy = (text: string) =>
    action("copy", () => call("copy_to_clipboard", { text }), "Texto copiado.");
  const loadMore = () =>
    action("history", async () => {
      if (paging.current || !hasMore) return;
      paging.current = true;
      const generation = historyGeneration.current;
      try {
        const page = await call<HistoryEntry[]>("get_transcription_history", {
          limit: 100,
          offset: history.length,
        });
        if (generation !== historyGeneration.current) return;
        setHistory((previous) => {
          const seen = new Set(previous.map((item) => item.id));
          return [...previous, ...page.filter((item) => !seen.has(item.id))];
        });
        setHasMore(page.length === 100);
      } finally {
        paging.current = false;
      }
    });
  const queueAction = (
    key: string,
    command: string,
    args: Record<string, unknown>,
  ) =>
    action(key, async () => {
      await call(command, args);
      await loadQueue();
    });
  return {
    session,
    prefs,
    preferencesLoaded,
    history,
    hasMore,
    queue,
    recoveries,
    pending,
    message,
    error,
    setError,
    setMessage,
    action,
    save,
    selectModel,
    copy,
    loadMore,
    queueAction,
    loadPreferences,
  };
}
export type Dicta = ReturnType<typeof useDicta>;
