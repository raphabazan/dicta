import { useState, useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { playStartSound, playStopSound, playCancelSound, playResponseSound } from "./sounds";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

interface TranscriptionEntry {
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
}

function App() {
  const [isRecording, setIsRecording] = useState(false);
  const [status, setStatus] = useState("Ready");
  const [currentView, setCurrentView] = useState<"home" | "history" | "queue" | "stats" | "settings">("home");
  const [availableMicrophones, setAvailableMicrophones] = useState<string[]>([]);
  const [selectedMicrophone, setSelectedMicrophone] = useState<string>("");
  const [transcriptionHistory, setTranscriptionHistory] = useState<TranscriptionEntry[]>([]);
  const [useRealtimeAPI, setUseRealtimeAPI] = useState(true); // Toggle between Whisper and Realtime
  const [currentTranscript, setCurrentTranscript] = useState(""); // Real-time transcript display
  const [isStarting, setIsStarting] = useState(false); // Prevent duplicate start calls
  const [isStopping, setIsStopping] = useState(false); // Prevent duplicate stop calls
  const [statsData, setStatsData] = useState<StatsData | null>(null);
  const [statsRange, setStatsRange] = useState<"today" | "7days" | "month" | "year" | "all">("month");
  const [showStatsDetails, setShowStatsDetails] = useState(false);
  const [ttsEnabled, setTtsEnabled] = useState(false);
  const [queueCount, setQueueCount] = useState(0);
  const [queueRetrying, setQueueRetrying] = useState(false);
  const [queueItems, setQueueItems] = useState<PendingQueueItem[]>([]);
  const [updateAvailable, setUpdateAvailable] = useState(false);
  const [updateVersion, setUpdateVersion] = useState("");
  const [isUpdating, setIsUpdating] = useState(false);
  const [updateProgress, setUpdateProgress] = useState("");

  // Refs to always have current values inside event listener closures
  const isRecordingRef = useRef(false);
  const isStartingRef = useRef(false);
  const isStoppingRef = useRef(false);

  const startRecording = async () => {
    if (isStartingRef.current || isRecordingRef.current || isStoppingRef.current) {
      console.log(`⏭️ Blocked: starting=${isStartingRef.current} recording=${isRecordingRef.current} stopping=${isStoppingRef.current}`);
      return;
    }

    try {
      isStartingRef.current = true;
      setIsStarting(true);
      setCurrentTranscript(""); // Reset transcript

      // Play start sound and wait for it to finish before backend mutes system audio
      playStartSound();
      await new Promise(resolve => setTimeout(resolve, 200));

      // Query backend for current mode (source of truth)
      const useRealtime = await invoke<boolean>("get_use_realtime");
      console.log(`🎯 Backend mode: ${useRealtime ? 'Realtime' : 'Whisper'}`);

      if (useRealtime) {
        await invoke("start_realtime_recording");
        setStatus("🎤 Recording (Realtime)...");
      } else {
        await invoke("start_recording_audio");
        setStatus("🎤 Recording (Whisper)...");
      }

      isRecordingRef.current = true;
      setIsRecording(true);
      console.log(`🔴 Recording started (${useRealtime ? 'Realtime' : 'Whisper'})`);
    } catch (error) {
      console.error("Failed to start recording:", error);
      setStatus("Error starting recording");
      isRecordingRef.current = false;
    } finally {
      isStartingRef.current = false;
      setTimeout(() => setIsStarting(false), 300);
    }
  };

  const stopRecording = async () => {
    console.log(`🛑 stopRecording called (stopping=${isStoppingRef.current}, recording=${isRecordingRef.current})`);

    if (isStoppingRef.current || !isRecordingRef.current) {
      console.log("⏭️ Blocked: already stopping or not recording");
      return;
    }

    try {
      isStoppingRef.current = true;
      setIsStopping(true);
      setStatus("⏳ Processing...");

      // Play stop sound
      playStopSound();

      // Query backend for current mode (source of truth)
      const useRealtime = await invoke<boolean>("get_use_realtime");
      console.log(`🎯 Backend mode for stop: ${useRealtime ? 'Realtime' : 'Whisper'}`);

      if (useRealtime) {
        console.log("📞 Calling stop_realtime_recording...");
        await invoke("stop_realtime_recording");
        console.log("✅ stop_realtime_recording returned");
      } else {
        console.log("📞 Calling stop_recording_audio...");
        await invoke("stop_recording_audio");
        console.log("✅ stop_recording_audio returned");
      }

      isRecordingRef.current = false;
      setIsRecording(false);
      setStatus("Ready");
      console.log("⚪ Recording stopped");
      // Refresh history after recording
      loadTranscriptionHistory();
    } catch (error) {
      console.error("Failed to stop recording:", error);
      setStatus("Error stopping recording");
      isRecordingRef.current = false;
      setIsRecording(false);
    } finally {
      isStoppingRef.current = false;
      setIsStopping(false);
    }
  };

  const loadTranscriptionHistory = async () => {
    try {
      const history = await invoke<TranscriptionEntry[]>("get_transcription_history");
      setTranscriptionHistory(history); // Already ordered by backend (most recent first)
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

    // Listen for hotkey events - uses refs to avoid stale closure values
    const unlistenHotkey = listen("toggle-recording", async () => {
      console.log(`✅ Hotkey event received (recording=${isRecordingRef.current}, stopping=${isStoppingRef.current}, starting=${isStartingRef.current})`);

      if (isRecordingRef.current) {
        console.log("➡️ Calling stopRecording()");
        await stopRecording();
      } else if (!isStoppingRef.current) {
        console.log("➡️ Calling startRecording()");
        await startRecording();
      } else {
        console.log("⏭️ Hotkey ignored: stop in progress");
      }
    });

    // Listen for widget stop event
    const unlistenWidgetStop = listen("widget-stop-recording", async () => {
      console.log("🛑 Widget stop event received");
      await stopRecording();
    });

    // Listen for widget cancel event
    const unlistenWidgetCancel = listen("recording-cancelled", async () => {
      console.log("❌ Widget cancel event received");
      playCancelSound();
      setIsRecording(false);
      setStatus("Cancelled");
      setTimeout(() => setStatus("Ready"), 2000);
    });

    // Listen for auto-stop event (6 minute limit)
    const unlistenAutoStop = listen("auto-stop-recording", async () => {
      console.log("⏰ Auto-stop event received (6 minute limit)");
      await stopRecording();
    });

    // Listen for history updates
    const unlistenHistory = listen("history-updated", async () => {
      console.log("📚 History updated, reloading...");
      await loadTranscriptionHistory();
    });

    // Listen for realtime transcription deltas
    const unlistenDelta = listen<string>("transcription-delta", (event) => {
      console.log("📝 Delta received:", event.payload);
      setCurrentTranscript((prev) => prev + event.payload);
    });

    // Listen for transcription completion
    const unlistenTranscription = listen<string>("transcription-complete", async (event) => {
      const text = event.payload;
      console.log("📋 Transcription complete:", text);

      try {
        // 1. Save current clipboard content
        let originalClipboard = "";
        try {
          originalClipboard = await navigator.clipboard.readText();
          console.log("💾 Saved original clipboard");
        } catch (e) {
          console.warn("Could not read clipboard:", e);
        }

        // 2. Put transcribed text in clipboard
        await navigator.clipboard.writeText(text);
        console.log("📋 Transcription in clipboard");

        // 3. Try to paste automatically (simulate Ctrl+V)
        // This works by briefly focusing a hidden input, pasting, then restoring focus
        setStatus("✅ Pasted! (Press Alt+Shift+Z if paste failed)");

        // 4. Restore original clipboard after a short delay
        setTimeout(async () => {
          if (originalClipboard) {
            await navigator.clipboard.writeText(originalClipboard);
            console.log("♻️ Restored original clipboard");
          }
          setStatus("Ready");
        }, 1000);

      } catch (error) {
        console.error("Auto-paste failed:", error);
        setStatus("❌ Failed - Press Alt+Shift+Z to paste");
      }
    });

    // Listen for response-ready (play notification sound)
    const unlistenResponse = listen("response-ready", () => {
      console.log("🔔 Response ready, playing notification sound");
      playResponseSound();
    });

    // Listen for TTS toggle events (from Ctrl+Alt+S hotkey)
    const unlistenTts = listen<boolean>("tts-toggled", (event) => {
      console.log("🔊 TTS toggled:", event.payload);
      setTtsEnabled(event.payload);
    });

    // Listen for queue events
    const unlistenQueueUpdated = listen<number>("queue-updated", (event) => {
      console.log("📋 Queue updated:", event.payload);
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

    // Listen for recording errors (connection drops, API failures)
    const unlistenRecordingError = listen<string>("recording-error", (event) => {
      console.error("❌ Recording error:", event.payload);
      isRecordingRef.current = false;
      isStartingRef.current = false;
      isStoppingRef.current = false;
      setIsRecording(false);
      setIsStarting(false);
      setIsStopping(false);
      setStatus(`Erro: ${event.payload}`);
      setCurrentTranscript("");
      playCancelSound();
      setTimeout(() => setStatus("Ready"), 5000);
    });

    return () => {
      unlistenHotkey.then((fn) => fn());
      unlistenWidgetStop.then((fn) => fn());
      unlistenWidgetCancel.then((fn) => fn());
      unlistenAutoStop.then((fn) => fn());
      unlistenHistory.then((fn) => fn());
      unlistenDelta.then((fn) => fn());
      unlistenTranscription.then((fn) => fn());
      unlistenResponse.then((fn) => fn());
      unlistenTts.then((fn) => fn());
      unlistenQueueUpdated.then((fn) => fn());
      unlistenQueueFull.then((fn) => fn());
      unlistenQueueCompleted.then((fn) => fn());
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
            Fila{queueCount > 0 ? ` (${queueCount})` : ""}
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
              {/* API Toggle */}
              <div className="flex items-center justify-between border-b border-gray-700 pb-4">
                <span className="text-sm text-gray-400">Transcription API:</span>
                <div className="flex items-center gap-2">
                  <button
                    onClick={async () => {
                      setUseRealtimeAPI(false);
                      await invoke("set_use_realtime", { useRealtime: false });
                    }}
                    disabled={isRecording}
                    className={`px-3 py-1 rounded text-xs transition-colors ${
                      !useRealtimeAPI
                        ? "bg-blue-600 text-white"
                        : "bg-gray-700 text-gray-300 hover:bg-gray-600"
                    } ${isRecording ? "opacity-50 cursor-not-allowed" : ""}`}
                  >
                    Whisper
                  </button>
                  <button
                    onClick={async () => {
                      setUseRealtimeAPI(true);
                      await invoke("set_use_realtime", { useRealtime: true });
                    }}
                    disabled={isRecording}
                    className={`px-3 py-1 rounded text-xs transition-colors ${
                      useRealtimeAPI
                        ? "bg-blue-600 text-white"
                        : "bg-gray-700 text-gray-300 hover:bg-gray-600"
                    } ${isRecording ? "opacity-50 cursor-not-allowed" : ""}`}
                  >
                    Realtime ⚡
                  </button>
                </div>
              </div>

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
              {queueCount > 0 && (
                <div className="flex items-center justify-between border-b border-gray-700 pb-4">
                  <span className="text-sm text-yellow-400">
                    {queueCount} {queueCount === 1 ? "item" : "itens"} pendente{queueCount > 1 ? "s" : ""} na fila
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
                  {status}
                </span>
              </div>

              {isRecording && (
                <div className="bg-red-900/30 border border-red-500/50 rounded p-4">
                  <div className="flex items-center space-x-3">
                    <div className="w-3 h-3 bg-red-500 rounded-full animate-pulse"></div>
                    <span className="text-sm text-red-300">Recording in progress...</span>
                  </div>

                  {/* Real-time transcript display */}
                  {useRealtimeAPI && currentTranscript && (
                    <div className="mt-3 pt-3 border-t border-red-500/30">
                      <p className="text-xs text-gray-400 mb-1">Live Transcript:</p>
                      <p className="text-sm text-white">{currentTranscript}</p>
                    </div>
                  )}
                </div>
              )}

              <div className="border-t border-gray-700 pt-4">
                <h2 className="text-lg font-semibold mb-2">Quick Start</h2>
                <ul className="text-sm text-gray-300 space-y-2">
                  <li>• Press <kbd className="px-2 py-1 bg-gray-700 rounded">Ctrl+Space</kbd> to start recording</li>
                  <li>• Speak naturally</li>
                  <li>• Press <kbd className="px-2 py-1 bg-gray-700 rounded">Ctrl+Space</kbd> again to stop</li>
                  <li>• Your text will be automatically pasted</li>
                  <li>• Press <kbd className="px-2 py-1 bg-gray-700 rounded">Alt+Shift+Z</kbd> to paste last transcription</li>
                </ul>
              </div>
            </div>

            <div className="text-center text-xs text-gray-500">
              Phase 0 - Local MVP • Hotkey: Ctrl+Space {isRecording ? "🔴" : "⚪"}
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

            {queueItems.length === 0 ? (
              <div className="text-center py-12">
                <p className="text-4xl mb-3">&#10003;</p>
                <p className="text-gray-400">Nenhum item pendente na fila</p>
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

            {/* Microphone Selector */}
            <div className="space-y-4">
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
