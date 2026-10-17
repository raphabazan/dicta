import { TranscriptionSettings } from "./TranscriptionSettings";
import { SessionPanel } from "./SessionPanel";
import { initialSession, reconcileSession, sessionBusy, sessionLabel, deliveryMessage, type SessionSnapshot, type SegmentConfig, type RecoverySession } from "./session";
import { useState, useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { playStartSound, playStopSound, playCancelSound, playResponseSound } from "./sounds";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

interface TranscriptionEntry {
  id: number;
  text: string;
  timestamp: number;
}

interface StatsData {
  total_words: number;
  total_transcriptions: number;
  total_duration_ms: number;
  total_cost_cents: number;
}

interface PendingQueueItem {
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

const transcriptionLanguageOptions = [
  { code: "pt", label: "Português" },
  { code: "en", label: "English" },
  { code: "es", label: "Español" },
  { code: "fr", label: "Français" },
  { code: "de", label: "Deutsch" },
  { code: "it", label: "Italiano" },
];

function App() {
  const [isRecording, setIsRecording] = useState(false);
  const [status, setStatus] = useState("Ready");
  const [currentView, setCurrentView] = useState<"home" | "history" | "queue" | "stats" | "settings">("home");
  const [availableMicrophones, setAvailableMicrophones] = useState<string[]>([]);
  const [selectedMicrophone, setSelectedMicrophone] = useState<string>("");
  const [transcriptionHistory, setTranscriptionHistory] = useState<TranscriptionEntry[]>([]);
  const historyLoadingRef = useRef(false);
  const historyGenerationRef = useRef(0);
  const [historyHasMore, setHistoryHasMore] = useState(true);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [shortcutErrors, setShortcutErrors] = useState<string[]>([]);
  const [segmentConfig, setSegmentConfig] = useState<SegmentConfig>({ enabled: false, target_seconds: 20, silence_ms: 500 });
  const [asrModel, setAsrModel] = useState("whisper-1");
  const [session, setSession] = useState<SessionSnapshot>(initialSession);
  const sessionRef = useRef(initialSession);
  const [recoverySessions, setRecoverySessions] = useState<RecoverySession[]>([]);
  const [buildId, setBuildId] = useState("");
  const [webSearch, setWebSearch] = useState(false);
  const [transcriptionLanguages, setTranscriptionLanguages] = useState<string[]>([]);
  const [gameModeEnabled, setGameModeEnabled] = useState(false);
  const [gameModeProcessesText, setGameModeProcessesText] = useState("");

  const [statsData, setStatsData] = useState<StatsData | null>(null);
  const [statsRange, setStatsRange] = useState<"today" | "7days" | "month" | "year" | "all">("month");
  const [showStatsDetails, setShowStatsDetails] = useState(false);
  const [ttsEnabled, setTtsEnabled] = useState(false);
  const [queueCount, setQueueCount] = useState(0);
  const pendingCount = queueCount + recoverySessions.length;
  const [queueRetrying, setQueueRetrying] = useState(false);
  const [queueItems, setQueueItems] = useState<PendingQueueItem[]>([]);
  const [updateAvailable, setUpdateAvailable] = useState(false);
  const [updateVersion, setUpdateVersion] = useState("");
  const [isUpdating, setIsUpdating] = useState(false);
  const [updateProgress, setUpdateProgress] = useState("");

  const loadTranscriptionHistory = async () => {
    const generation = ++historyGenerationRef.current;
    try {
      const history = await invoke<TranscriptionEntry[]>("get_transcription_history");
      if (generation !== historyGenerationRef.current) return;
      setTranscriptionHistory(history); // Already ordered by backend (most recent first)
      setHistoryHasMore(history.length === 100);
    } catch (error) {
      console.error("Failed to load history:", error);
    }
  };

  const loadQueueItems = async () => {
    try {
      const items = await invoke<PendingQueueItem[]>("get_queue_items");
      setQueueItems(items);
      setQueueCount(items.length);
    } catch (error) {
      console.error("Failed to load queue items:", error);
    }
  };

  const copyToClipboard = async (text: string) => {
    try {
      await invoke("copy_to_clipboard", { text });
      setStatus("✅ Copied to clipboard!");
      setTimeout(() => setStatus("Ready"), 2000);
    } catch (error) {
      console.error("Failed to copy:", error);
      setStatus("❌ Failed to copy");
    }
  };

  const loadMicrophones = async () => {
    try {
      const mics = await invoke<string[]>("list_microphones");
      setAvailableMicrophones(mics);

      const selected = await invoke<string | null>("get_selected_microphone");
      if (selected) {
        setSelectedMicrophone(selected);
      } else if (mics.length > 0) {
        setSelectedMicrophone(mics[0]);
      }
    } catch (error) {
      console.error("Failed to load microphones:", error);
    }
  };

  const selectMicrophone = async (deviceName: string) => {
    try {
      await invoke("set_selected_microphone", { deviceName });
      setSelectedMicrophone(deviceName);
      setStatus("🎤 Microphone selected!");
      setTimeout(() => setStatus("Ready"), 2000);
    } catch (error) {
      console.error("Failed to select microphone:", error);
      setStatus("❌ Failed to select microphone");
    }
  };

  const loadTranscriptionPreferences = async () => {
    try {
      const [config, languages, gameMode, gameProcesses, asr, build, search, shortcuts] = await Promise.all([
        invoke<SegmentConfig>("get_segment_config"),
        invoke<string[]>("get_transcription_languages"),
        invoke<boolean>("get_game_mode_enabled"),
        invoke<string[]>("get_game_mode_processes"),
        invoke<string>("get_asr_model"),
        invoke<string>("get_build_info"),
        invoke<boolean>("get_web_search_enabled"),
        invoke<string[]>("get_shortcut_errors"),
      ]);
      setSegmentConfig(config);
      setAsrModel(asr);
      setBuildId(build);
      setWebSearch(search);
      setShortcutErrors(shortcuts);
      setTranscriptionLanguages(languages);
      setGameModeEnabled(gameMode);
      setGameModeProcessesText(gameProcesses.join(", "));
    } catch (error) {
    }
  };

  const toggleTranscriptionLanguage = async (language: string) => {
    const nextLanguages = transcriptionLanguages.includes(language)
      ? transcriptionLanguages.filter((value) => value !== language)
      : [...transcriptionLanguages, language];

    try {
      const saved = await invoke<string[]>("set_transcription_languages", { languages: nextLanguages });
      setTranscriptionLanguages(saved);
      setStatus("Idiomas de transcrição atualizados");
      setTimeout(() => setStatus("Ready"), 2000);
    } catch (error) {
      setStatus("❌ Failed to update languages");
    }
  };

  const saveGameModeProcesses = async (rawValue: string) => {
    const processes = rawValue
      .split(",")
      .map((value) => value.trim().toLowerCase())
      .filter(Boolean);

    try {
      const saved = await invoke<string[]>("set_game_mode_processes", { processes });
      setGameModeProcessesText(saved.join(", "));
      setStatus("Modo de jogo atualizado");
      setTimeout(() => setStatus("Ready"), 2000);
    } catch (error) {
      console.error("Failed to save game mode processes:", error);
      setStatus("❌ Failed to update game mode");
    }
  };

  const saveSegmentConfig = async (config: SegmentConfig) => {
    try { await invoke("set_segment_config", { config }); setSegmentConfig(config); }
    catch (error) { setStatus(String(error)); }
  };
  const saveAsrModel = async (model: string) => {
    try { await invoke("set_asr_model", { model }); setAsrModel(model); }
    catch (error) { setStatus(String(error)); }
  };
  const recoverSession = async (id: string) => {
    try { await invoke("retry_session", { id }); }
    catch (error) { setStatus(String(error)); }
    finally { setRecoverySessions(await invoke<RecoverySession[]>("get_recoverable_sessions")); }
  };
  const discardSession = async (id: string) => {
    if (!window.confirm("Descartar o áudio ou comando pendente desta sessão?")) return;
    try { await invoke("discard_session", { id }); setRecoverySessions(await invoke<RecoverySession[]>("get_recoverable_sessions")); }
    catch (error) { setStatus(String(error)); }
  };
  const loadMoreHistory = async () => {
    if (historyLoadingRef.current || !historyHasMore) return;
    historyLoadingRef.current = true;
    setHistoryLoading(true);
    const generation = historyGenerationRef.current;
    try {
      const page = await invoke<TranscriptionEntry[]>("get_transcription_history", { limit: 100, offset: transcriptionHistory.length });
      if (generation !== historyGenerationRef.current) return;
      setTranscriptionHistory(previous => {
        const seen = new Set(previous.map(entry => entry.id));
        return [...previous, ...page.filter(entry => !seen.has(entry.id))];
      });
      setHistoryHasMore(page.length === 100);
    }
    catch (error) { setStatus(String(error)); }
    finally { historyLoadingRef.current = false; setHistoryLoading(false); }
  };

  const loadStats = async (range: typeof statsRange) => {
    const now = Date.now();
    const startOfToday = new Date();
    startOfToday.setHours(0, 0, 0, 0);

    let fromTs: number;
    switch (range) {
      case "today":
        fromTs = startOfToday.getTime();
        break;
      case "7days":
        fromTs = now - 7 * 24 * 60 * 60 * 1000;
        break;
      case "month": {
        const d = new Date();
        d.setDate(1);
        d.setHours(0, 0, 0, 0);
        fromTs = d.getTime();
        break;
      }
      case "year": {
        const d = new Date();
        d.setMonth(0, 1);
        d.setHours(0, 0, 0, 0);
        fromTs = d.getTime();
        break;
      }
      case "all":
        fromTs = 0;
        break;
    }

    try {
      const data = await invoke<StatsData>("get_statistics", { fromTs, toTs: now });
      setStatsData(data);
    } catch (error) {
      console.error("Failed to load stats:", error);
    }
  };

  useEffect(() => {
    if (currentView === "stats") {
      loadStats(statsRange);
    }
    if (currentView === "queue") {
      loadQueueItems();
    }
  }, [currentView, statsRange]);

  useEffect(() => {
    // Load history and microphones on mount
    loadTranscriptionHistory();
    loadMicrophones();
    loadTranscriptionPreferences();
    invoke<boolean>("get_tts_enabled").then((v) => setTtsEnabled(v)).catch(() => {});
    invoke<number>("get_queue_count").then((v) => setQueueCount(v)).catch(() => {});

    // Check for updates
    check().then((update) => {
      if (update?.available) {
        setUpdateAvailable(true);
        setUpdateVersion(update.version);
        console.log("🔄 Update available:", update.version);
      }
    }).catch((e) => {
      // Don't block app if update check fails (offline, etc.)
      console.log("Update check skipped:", e);
    });
  }, []);

  useEffect(() => {
    console.log("🎧 Setting up event listeners");

    const unlistenStartSound = listen("recording-sound-start", playStartSound);
    const unlistenStopSound = listen("recording-sound-stop", playStopSound);

    // Listen for widget cancel event
    const unlistenWidgetCancel = listen("recording-cancelled", async () => {
      console.log("❌ Widget cancel event received");
      playCancelSound();
    });

    // Listen for history updates
    const unlistenHistory = listen("history-updated", async () => {
      console.log("📚 History updated, reloading...");
      await loadTranscriptionHistory();
    });

    let disposed = false;
    const receiveSession = (next: SessionSnapshot) => {
      if (disposed) return;
      const accepted = reconcileSession(sessionRef.current, next);
      if (accepted === sessionRef.current) return;
      sessionRef.current = accepted;
      setSession(accepted);
      const recording = accepted.phase === "recording";
      setIsRecording(recording);
      setStatus(sessionLabel(accepted));
    };
    const unlistenSession = listen<SessionSnapshot>("session-state", event => receiveSession(event.payload));
    // Register before requesting the snapshot; sequence/generation reject late replies.
    void unlistenSession.then(() => invoke<SessionSnapshot>("get_session_state")).then(receiveSession).catch(() => {});
    const unlistenDelivery = listen<{status:string;code:string}>("delivery-result", event => setStatus(deliveryMessage(event.payload.code)));
    const refreshRecovery = () => invoke<RecoverySession[]>("get_recoverable_sessions").then(items => { if (!disposed) setRecoverySessions(items); }).catch(() => {});
    void refreshRecovery();
    const unlistenSessions = listen("sessions-updated", refreshRecovery);
    const unlistenResumed = listen("runtime-resumed", () => {
      void invoke<SessionSnapshot>("get_session_state").then(receiveSession).catch(() => {});
      void refreshRecovery();
    });

    // Listen for response-ready (play notification sound)
    const unlistenResponse = listen("response-ready", () => {
      console.log("🔔 Response ready, playing notification sound");
      playResponseSound();
    });

    // Listen for TTS toggle events (from Ctrl+Alt+S hotkey)
    const unlistenTts = listen<boolean>("tts-toggled", (event) => {
      setTtsEnabled(event.payload);
    });

    // Listen for queue events
    const unlistenQueueUpdated = listen<number>("queue-updated", (event) => {
      setQueueCount(event.payload);
      invoke<PendingQueueItem[]>("get_queue_items")
        .then(items => setQueueItems(items))
        .catch(() => {});
    });

    const unlistenQueueFull = listen("queue-full", () => {
      console.log("⚠️ Queue full");
      setStatus("Fila offline cheia (max 3)");
      setTimeout(() => setStatus("Ready"), 3000);
    });

    const unlistenQueueCompleted = listen("queue-item-completed", () => {
      console.log("✅ Queue item completed");
      invoke<number>("get_queue_count").then((v) => setQueueCount(v)).catch(() => {});
    });

    // Listen for offline recording (no internet, mic still recording locally)
    const unlistenRecordingOffline = listen<string>("recording-offline", () => {
      setStatus("Sem conexão — gravando localmente");
    });

    // Listen for recording errors (connection drops, API failures)
    const unlistenRecordingError = listen<string>("recording-error", (event) => {
      void invoke<SessionSnapshot>("get_session_state").then(receiveSession).catch(() => {});
      setStatus(`Erro: ${event.payload}`);
    });

    return () => {
      unlistenStartSound.then(fn => fn());
      unlistenStopSound.then(fn => fn());
      unlistenWidgetCancel.then((fn) => fn());
      unlistenHistory.then((fn) => fn());
      disposed = true;
      unlistenSession.then(fn => fn());
      unlistenDelivery.then(fn => fn());
      unlistenSessions.then(fn => fn());
      unlistenResumed.then(fn => fn());
      unlistenResponse.then((fn) => fn());
      unlistenTts.then((fn) => fn());
      unlistenQueueUpdated.then((fn) => fn());
      unlistenQueueFull.then((fn) => fn());
      unlistenQueueCompleted.then((fn) => fn());
      unlistenRecordingOffline.then((fn) => fn());
      unlistenRecordingError.then((fn) => fn());
    };
  }, []); // Empty deps - refs always have current values, no need to re-register

  const formatDate = (timestamp: number) => {
    const date = new Date(timestamp);
    const today = new Date();
    const yesterday = new Date(today);
    yesterday.setDate(yesterday.getDate() - 1);

    const isToday = date.toDateString() === today.toDateString();
    const isYesterday = date.toDateString() === yesterday.toDateString();

    const time = date.toLocaleTimeString("pt-BR", { hour: "2-digit", minute: "2-digit" });

    if (isToday) {
      return `Hoje às ${time}`;
    } else if (isYesterday) {
      return `Ontem às ${time}`;
    } else {
      return date.toLocaleString("pt-BR", {
        day: "2-digit",
        month: "2-digit",
        year: "numeric",
        hour: "2-digit",
        minute: "2-digit"
      });
    }
  };

  const formatQueueError = (error: string | null) => {
    if (!error) return null;

    const lower = error.toLowerCase();
    if (lower.includes("insufficient_quota")) {
      return "OpenAI sem quota ou sem billing disponível para essa chave.";
    }
    if (lower.includes("invalid_api_key") || lower.includes("incorrect api key")) {
      return "A chave da OpenAI parece inválida.";
    }
    if (lower.includes("account_deactivated")) {
      return "A conta da OpenAI usada pelo app está desativada.";
    }

    return error;
  };

  const applyUpdate = async () => {
    setIsUpdating(true);
    setUpdateProgress("Baixando...");
    try {
      const update = await check();
      if (update?.available) {
        await update.downloadAndInstall((event) => {
          if (event.event === "Started" && event.data.contentLength) {
            setUpdateProgress(`Baixando... (${Math.round(event.data.contentLength / 1024)}KB)`);
          } else if (event.event === "Finished") {
            setUpdateProgress("Instalando...");
          }
        });
        await relaunch();
      }
    } catch (e) {
      console.error("Update failed:", e);
      setUpdateProgress("Falha na atualização. Tente novamente.");
      setIsUpdating(false);
    }
  };

  return (
    <div className="min-h-screen bg-gray-900 text-white flex flex-col items-center justify-center p-8">
      {/* Update Required Overlay */}
      {updateAvailable && (
        <div className="fixed inset-0 z-50 bg-black/90 flex flex-col items-center justify-center">
          <div className="text-center space-y-6 max-w-md">
            <h2 className="text-3xl font-bold">Atualização Disponível</h2>
            <p className="text-gray-300">
              Versão <span className="text-blue-400 font-semibold">{updateVersion}</span> está disponível.
              Você precisa atualizar para continuar usando o Dicta.
            </p>
            {isUpdating ? (
              <div className="space-y-2">
                <div className="w-48 h-2 bg-gray-700 rounded-full mx-auto overflow-hidden">
                  <div className="h-full bg-blue-500 rounded-full animate-pulse" style={{ width: "60%" }}></div>
                </div>
                <p className="text-sm text-gray-400">{updateProgress}</p>
              </div>
            ) : (
              <button
                onClick={applyUpdate}
                className="px-8 py-3 bg-blue-600 hover:bg-blue-500 rounded-lg text-lg font-semibold transition-colors"
              >
                Atualizar Agora
              </button>
            )}
          </div>
        </div>
      )}

      <div className="max-w-2xl w-full space-y-8">
        <div className="text-center">
          <h1 className="text-4xl font-bold mb-2">🎤 Dicta</h1>
          <p className="text-gray-400">Smart Voice-to-Text Dictation</p>
        </div>

        {/* Navigation */}
        <div className="flex gap-2 bg-gray-800 rounded-lg p-2">
          <button
            onClick={() => setCurrentView("home")}
            className={`flex-1 py-2 px-4 rounded transition-colors ${
              currentView === "home"
                ? "bg-blue-600 text-white"
                : "text-gray-400 hover:bg-gray-700"
            }`}
          >
            Home
          </button>
          <button
            onClick={() => setCurrentView("history")}
            className={`flex-1 py-2 px-4 rounded transition-colors ${
              currentView === "history"
                ? "bg-blue-600 text-white"
                : "text-gray-400 hover:bg-gray-700"
            }`}
          >
            Histórico ({transcriptionHistory.length})
          </button>
          <button
            onClick={() => setCurrentView("queue")}
            className={`flex-1 py-2 px-4 rounded transition-colors ${
              currentView === "queue"
                ? "bg-blue-600 text-white"
                : "text-gray-400 hover:bg-gray-700"
            }`}
          >
            Fila{pendingCount > 0 ? ` (${pendingCount})` : ""}
          </button>
          <button
            onClick={() => setCurrentView("stats")}
            className={`flex-1 py-2 px-4 rounded transition-colors ${
              currentView === "stats"
                ? "bg-blue-600 text-white"
                : "text-gray-400 hover:bg-gray-700"
            }`}
          >
            Stats
          </button>
          <button
            onClick={() => setCurrentView("settings")}
            className={`flex-1 py-2 px-4 rounded transition-colors ${
              currentView === "settings"
                ? "bg-blue-600 text-white"
                : "text-gray-400 hover:bg-gray-700"
            }`}
          >
            ⚙️ Config
          </button>
        </div>

        {currentView === "home" ? (
          <>
            <div className="bg-gray-800 rounded-lg p-6 space-y-4">
              <TranscriptionSettings config={segmentConfig} model={asrModel} disabled={sessionBusy(session)} onConfig={saveSegmentConfig} onModel={saveAsrModel} />

              {/* TTS Toggle */}
              <div className="flex items-center justify-between border-b border-gray-700 pb-4">
                <span className="text-sm text-gray-400">Text-to-Speech:</span>
                <button
                  onClick={async () => {
                    const newVal = !ttsEnabled;
                    setTtsEnabled(newVal);
                    await invoke("set_tts_enabled", { enabled: newVal });
                  }}
                  className={`px-3 py-1 rounded text-xs transition-colors ${
                    ttsEnabled
                      ? "bg-green-600 text-white"
                      : "bg-gray-700 text-gray-300 hover:bg-gray-600"
                  }`}
                >
                  {ttsEnabled ? "ON" : "OFF"}
                </button>
              </div>

              {/* Offline Queue Indicator */}
              {pendingCount > 0 && (
                <div className="flex items-center justify-between border-b border-gray-700 pb-4">
                  <span className="text-sm text-yellow-400">
                    {pendingCount} {pendingCount === 1 ? "item" : "itens"} pendente{pendingCount > 1 ? "s" : ""} na fila
                  </span>
                  <button
                    onClick={() => setCurrentView("queue")}
                    className="px-3 py-1 rounded text-xs bg-yellow-600 text-white hover:bg-yellow-500 transition-colors"
                  >
                    Ver Fila
                  </button>
                </div>
              )}

              <div className="flex items-center justify-between">
                <span className="text-sm text-gray-400">Status:</span>
                <span className={`font-medium ${isRecording ? "text-red-500 animate-pulse" : "text-green-400"}`}>
                  {sessionBusy(session) ? sessionLabel(session) : status}
                </span>
              </div>

              {isRecording && (
                <div className="bg-red-900/30 border border-red-500/50 rounded p-4">
                  <div className="flex items-center space-x-3">
                    <div className="w-3 h-3 bg-red-500 rounded-full animate-pulse"></div>
                    <span className="text-sm text-red-300">Recording in progress...</span>
                  </div>


                </div>
              )}

              <SessionPanel session={session} onCopy={copyToClipboard} />
              {shortcutErrors.length > 0 && <p role="alert" className="rounded border border-amber-600 p-3 text-sm text-amber-200">O Windows não liberou estes atalhos para o Dicta: {shortcutErrors.join(", ")}. Feche outra instância ou o aplicativo que está usando esses atalhos e reinicie o Dicta.</p>}

              <div className="border-t border-gray-700 pt-4">
                <h2 className="text-lg font-semibold mb-2">Como usar</h2>
                <ul className="text-sm text-gray-300 space-y-2">
                  <li>• Press <kbd className="px-2 py-1 bg-gray-700 rounded">Ctrl+Space</kbd> to start recording</li>
                  <li>• Speak naturally</li>
                  <li>• Press <kbd className="px-2 py-1 bg-gray-700 rounded">Ctrl+Space</kbd> again to stop</li>
                  <li>• Ao terminar, o Dicta tenta colar no mesmo aplicativo; o texto permanece no histórico</li>
                  <li>• Press <kbd className="px-2 py-1 bg-gray-700 rounded">Alt+Shift+Z</kbd> to paste last transcription</li>
                </ul>
              </div>
            </div>

            <div className="text-center text-xs text-gray-500">
              Build {buildId || "carregando…"} · Ctrl+Espaço
            </div>
          </>
        ) : currentView === "history" ? (
          <div className="bg-gray-800 rounded-lg p-6">
            <h2 className="text-xl font-semibold mb-4">Histórico de Transcrições</h2>

            {transcriptionHistory.length === 0 ? (
              <p className="text-gray-400 text-center py-8">Nenhuma transcrição ainda</p>
            ) : (
              <div className="space-y-3 max-h-[500px] overflow-y-auto">
                {transcriptionHistory.map((entry, index) => (
                  <div
                    key={index}
                    className="bg-gray-700/50 rounded-lg p-4 hover:bg-gray-700 transition-colors"
                  >
                    <div className="flex items-start justify-between gap-4">
                      <div className="flex-1">
                        <p className="text-sm text-gray-400 mb-2">{formatDate(entry.timestamp)}</p>
                        <p className="text-white">{entry.text}</p>
                      </div>
                      <button
                        onClick={() => copyToClipboard(entry.text)}
                        className="px-3 py-1 bg-blue-600 hover:bg-blue-700 rounded text-sm transition-colors flex-shrink-0"
                      >
                        📋 Copiar
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            )}
            {historyHasMore && transcriptionHistory.length >= 100 && <button disabled={historyLoading} className="mt-4 rounded bg-gray-700 px-4 py-2 text-sm disabled:opacity-50" onClick={() => void loadMoreHistory()}>{historyLoading ? "Carregando…" : "Carregar mais"}</button>}
          </div>
        ) : currentView === "queue" ? (
          <div className="bg-gray-800 rounded-lg p-6">
            <div className="flex items-center justify-between mb-4">
              <h2 className="text-xl font-semibold">Fila de Pendentes</h2>
              {queueItems.length > 0 && (
                <button
                  onClick={async () => {
                    setQueueRetrying(true);
                    try {
                      await invoke("retry_pending_queue");
                    } catch (e) {
                      console.error("Retry all failed:", e);
                    }
                    setTimeout(() => setQueueRetrying(false), 2000);
                  }}
                  className="px-3 py-1 bg-yellow-600 hover:bg-yellow-500 rounded text-sm transition-colors disabled:opacity-50"
                  disabled={queueRetrying}
                >
                  {queueRetrying ? "Processando..." : "Retry Todos"}
                </button>
              )}
            </div>

            {recoverySessions.length > 0 && <section aria-label="Sessões recuperáveis" className="space-y-3 mb-6">
              <h3 className="font-medium">Áudio e comandos preservados</h3>
              <p className="text-xs text-gray-400">Recuperar conclui o processamento e salva o texto. A fila não cola automaticamente.</p>
              {recoverySessions.map(item => <div key={item.id} className="rounded border border-amber-700/50 p-3 flex flex-wrap justify-between gap-3">
                <div className="text-sm"><p>{new Date(item.created_at).toLocaleString("pt-BR")}</p><p className="text-xs text-gray-400">{Math.floor(item.duration_ms / 1000)} s · {item.status === "incomplete" ? "Captura incompleta; revisão necessária" : "Pendente"}</p></div>
                <div className="flex gap-2">
                  <button disabled={sessionBusy(session) || item.status === "incomplete"} className="rounded bg-blue-600 px-3 py-1 text-sm disabled:opacity-40" onClick={() => void recoverSession(item.id)}>Recuperar</button>
                  <button disabled={sessionBusy(session)} className="rounded bg-gray-700 px-3 py-1 text-sm" onClick={() => void discardSession(item.id)}>Descartar</button>
                </div>
              </div>)}
            </section>}

            {queueItems.length === 0 ? (
              <div className="text-center py-12">
                <p className="text-4xl mb-3">&#10003;</p>
                <p className="text-gray-400">Nenhum item da fila anterior</p>
                <p className="text-gray-500 text-sm mt-1">
                  Itens aparecem aqui quando falham por problemas de conexao
                </p>
              </div>
            ) : (
              <div className="space-y-3">
                {queueItems.map((item) => (
                  <div
                    key={item.id}
                    className="bg-gray-700/50 rounded-lg p-4 hover:bg-gray-700 transition-colors"
                  >
                    <div className="flex items-start justify-between gap-4">
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2 mb-2">
                          <span className={`px-2 py-0.5 rounded text-xs font-medium ${
                            item.mode.includes("audio") || item.mode.includes("transcribe")
                              ? "bg-purple-600/30 text-purple-300"
                              : "bg-blue-600/30 text-blue-300"
                          }`}>
                            {item.mode === "realtime-audio" ? "Realtime Audio"
                             : item.mode === "whisper-transcribe" ? "Whisper"
                             : item.mode === "whisper-prompt" ? "Whisper + Prompt"
                             : item.mode === "realtime-prompt" ? "Realtime + Prompt"
                             : item.mode === "text-prompt" ? "Texto"
                             : item.mode}
                          </span>
                          <span className="text-xs text-gray-500">{item.model}</span>
                          {item.auto_retry_disabled && (
                            <span className="px-2 py-0.5 rounded text-xs font-medium bg-red-600/30 text-red-300">
                              Auto retry pausado
                            </span>
                          )}
                        </div>
                        <p className="text-sm text-gray-400">
                          {formatDate(item.created_at)}
                        </p>
                        {item.prompt_text && (
                          <p className="text-sm text-gray-300 mt-1 truncate">
                            {item.prompt_text.substring(0, 120)}
                            {item.prompt_text.length > 120 ? "..." : ""}
                          </p>
                        )}
                        {item.audio_path && !item.prompt_text && (
                          <p className="text-xs text-gray-500 mt-1">
                            Audio salvo em disco
                          </p>
                        )}
                        {item.retry_count > 0 && (
                          <p className="text-xs text-yellow-500 mt-1">
                            {item.retry_count} tentativa{item.retry_count > 1 ? "s" : ""}
                          </p>
                        )}
                        {item.last_error && (
                          <p className="text-xs text-red-300 mt-2 break-words">
                            {formatQueueError(item.last_error)}
                          </p>
                        )}
                      </div>
                      <div className="flex gap-2 flex-shrink-0">
                        {item.audio_path && (
                          <button
                            onClick={async () => {
                              try {
                                await invoke("play_queue_audio", { audioPath: item.audio_path });
                              } catch (e) {
                                console.error("Play failed:", e);
                              }
                            }}
                            className="px-3 py-1 bg-green-600/80 hover:bg-green-500 rounded text-sm transition-colors"
                          >
                            Play
                          </button>
                        )}
                        <button
                          onClick={async () => {
                            try {
                              await invoke("retry_single_queue_item", { id: item.id });
                            } catch (e) {
                              console.error("Retry failed:", e);
                            }
                          }}
                          className="px-3 py-1 bg-yellow-600 hover:bg-yellow-500 rounded text-sm transition-colors"
                        >
                          Retry
                        </button>
                        <button
                          onClick={async () => {
                            try {
                              await invoke("delete_single_queue_item", { id: item.id });
                            } catch (e) {
                              console.error("Delete failed:", e);
                            }
                          }}
                          className="px-3 py-1 bg-red-600/80 hover:bg-red-500 rounded text-sm transition-colors"
                        >
                          Excluir
                        </button>
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>
        ) : currentView === "stats" ? (
          <div className="bg-gray-800 rounded-lg p-6 space-y-6">
            <div className="flex items-center justify-between">
              <h2 className="text-xl font-semibold">Estatísticas</h2>
              <div className="flex gap-1">
                {([
                  ["today", "Hoje"],
                  ["7days", "7 Dias"],
                  ["month", "Mês"],
                  ["year", "Ano"],
                  ["all", "Tudo"],
                ] as const).map(([key, label]) => (
                  <button
                    key={key}
                    onClick={() => setStatsRange(key)}
                    className={`px-3 py-1 rounded text-xs transition-colors ${
                      statsRange === key
                        ? "bg-blue-600 text-white"
                        : "bg-gray-700 text-gray-300 hover:bg-gray-600"
                    }`}
                  >
                    {label}
                  </button>
                ))}
              </div>
            </div>

            {statsData ? (
              <>
                <div className="grid grid-cols-2 gap-4">
                  <div className="bg-gray-700/50 rounded-lg p-4 text-center">
                    <p className="text-2xl font-bold text-white">{statsData.total_words.toLocaleString("pt-BR")}</p>
                    <p className="text-xs text-gray-400 mt-1">Palavras Ditadas</p>
                  </div>
                  <div className="bg-gray-700/50 rounded-lg p-4 text-center">
                    <p className="text-2xl font-bold text-white">{statsData.total_transcriptions}</p>
                    <p className="text-xs text-gray-400 mt-1">Sessões</p>
                  </div>
                  <div className="bg-gray-700/50 rounded-lg p-4 text-center">
                    <p className="text-2xl font-bold text-white">
                      {statsData.total_duration_ms > 0
                        ? Math.round(statsData.total_words / (statsData.total_duration_ms / 60000))
                        : 0}
                    </p>
                    <p className="text-xs text-gray-400 mt-1">Média WPM</p>
                  </div>
                  <div className="bg-gray-700/50 rounded-lg p-4 text-center">
                    <p className="text-2xl font-bold text-white">
                      ${(statsData.total_cost_cents / 10000).toFixed(4)}
                    </p>
                    <p className="text-xs text-gray-400 mt-1">Custo Est.</p>
                  </div>
                </div>

                <button
                  onClick={() => setShowStatsDetails(!showStatsDetails)}
                  className="text-sm text-gray-400 hover:text-gray-200 transition-colors"
                >
                  {showStatsDetails ? "Ocultar Detalhes ▲" : "Mostrar Detalhes ▼"}
                </button>

                {showStatsDetails && (
                  <div className="bg-gray-700/30 rounded-lg p-4 space-y-3 text-sm">
                    <div className="flex justify-between">
                      <span className="text-gray-400">Tempo Total de Gravação</span>
                      <span className="text-white">
                        {Math.floor(statsData.total_duration_ms / 60000)}m {Math.floor((statsData.total_duration_ms % 60000) / 1000)}s
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-gray-400">Média por Sessão</span>
                      <span className="text-white">
                        {statsData.total_transcriptions > 0
                          ? Math.round(statsData.total_words / statsData.total_transcriptions)
                          : 0} palavras
                      </span>
                    </div>
                    <div className="flex justify-between">
                      <span className="text-gray-400">Duração Média</span>
                      <span className="text-white">
                        {statsData.total_transcriptions > 0
                          ? Math.round(statsData.total_duration_ms / statsData.total_transcriptions / 1000)
                          : 0}s
                      </span>
                    </div>
                  </div>
                )}
              </>
            ) : (
              <p className="text-gray-400 text-center py-8">Carregando...</p>
            )}
          </div>
        ) : (
          <div className="bg-gray-800 rounded-lg p-6">
            <h2 className="text-xl font-semibold mb-4">⚙️ Configurações</h2>

            <label className="flex items-center gap-3 text-sm mb-4">
              <input type="checkbox" checked={webSearch} onChange={async e => { const enabled = e.target.checked; try { await invoke("set_web_search_enabled", { enabled }); setWebSearch(enabled); } catch (error) { setStatus(String(error)); } }} />
              Permitir busca na web nos comandos de assistência
            </label>
            <button className="mb-4 rounded bg-gray-700 px-3 py-2 text-sm" onClick={() => void invoke("recover_runtime").catch(error => setStatus(String(error)))}>Reconciliar sessão após retomada</button>
            {/* Microphone Selector */}
            <div className="space-y-4">
              <TranscriptionSettings config={segmentConfig} model={asrModel} disabled={sessionBusy(session)} onConfig={saveSegmentConfig} onModel={saveAsrModel} />

              <div className="rounded-lg border border-gray-700 bg-gray-700/20 p-4 space-y-3">
                <div>
                  <p className="text-sm font-medium text-white">Idiomas de transcrição</p>
                  <p className="text-xs text-gray-400">
                    Um idioma fixa a transcrição. Vários idiomas viram um hint multilíngue para ajudar em áudio misto.
                  </p>
                </div>
                <div className="grid grid-cols-2 gap-2">
                  {transcriptionLanguageOptions.map((option) => (
                    <label
                      key={option.code}
                      className="flex items-center gap-2 rounded border border-gray-700 bg-gray-800/70 px-3 py-2 text-sm text-gray-200"
                    >
                      <input
                        type="checkbox"
                        checked={transcriptionLanguages.includes(option.code)}
                        onChange={() => toggleTranscriptionLanguage(option.code)}
                        className="h-4 w-4 accent-blue-500"
                      />
                      <span>{option.label}</span>
                    </label>
                  ))}
                </div>
              </div>

              <div className="rounded-lg border border-gray-700 bg-gray-700/20 p-4 space-y-3">
                <div className="flex items-center justify-between gap-4">
                  <div>
                    <p className="text-sm font-medium text-white">Modo de jogo</p>
                    <p className="text-xs text-gray-400">
                      Quando ativado, os hotkeys do Dicta ficam bloqueados se um jogo da Steam estiver em foco
                      ou se o executÃ¡vel ativo estiver na lista abaixo.
                    </p>
                  </div>
                  <button
                    onClick={async () => {
                      const nextValue = !gameModeEnabled;
                      setGameModeEnabled(nextValue);
                      await invoke("set_game_mode_enabled", { enabled: nextValue });
                    }}
                    className={`px-3 py-1 rounded text-xs transition-colors ${
                      gameModeEnabled
                        ? "bg-green-600 text-white"
                        : "bg-gray-700 text-gray-300 hover:bg-gray-600"
                    }`}
                  >
                    {gameModeEnabled ? "ON" : "OFF"}
                  </button>
                </div>

                <div>
                  <label className="block text-sm text-gray-400 mb-2">
                    Processos extras bloqueados
                  </label>
                  <input
                    value={gameModeProcessesText}
                    onChange={(e) => setGameModeProcessesText(e.target.value)}
                    onBlur={() => saveGameModeProcesses(gameModeProcessesText)}
                    placeholder="cs2.exe, valorant.exe"
                    className="w-full px-4 py-2 bg-gray-700 text-white rounded border border-gray-600 focus:border-blue-500 focus:outline-none"
                  />
                  <p className="text-xs text-gray-500 mt-1">
                    Use nomes separados por vÃ­rgula. Jogos da Steam jÃ¡ sÃ£o bloqueados automaticamente pelo caminho do executÃ¡vel.
                  </p>
                </div>
              </div>

              <div>
                <label className="block text-sm text-gray-400 mb-2">
                  Microfone
                </label>
                <select
                  value={selectedMicrophone}
                  onChange={(e) => selectMicrophone(e.target.value)}
                  className="w-full px-4 py-2 bg-gray-700 text-white rounded border border-gray-600 focus:border-blue-500 focus:outline-none"
                  disabled={isRecording}
                >
                  {availableMicrophones.map((mic) => (
                    <option key={mic} value={mic}>
                      {mic}
                    </option>
                  ))}
                </select>
                {isRecording && (
                  <p className="text-xs text-gray-500 mt-1">
                    Pare a gravação para trocar de microfone
                  </p>
                )}
              </div>

              <button
                onClick={loadMicrophones}
                className="px-4 py-2 bg-gray-700 hover:bg-gray-600 rounded text-sm transition-colors"
              >
                🔄 Recarregar Microfones
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

export default App;
