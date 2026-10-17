import { duration } from "./format";
import { sessionBusy, sessionLabel } from "./session";
import { call } from "./desktop";
import type { Dicta } from "./useDicta";
import { commandModels } from "./types";

export function SessionPanel({ app }: { app: Dicta }) {
  const { session, prefs, pending } = app;
  const busy = sessionBusy(session);
  const recording = session.phase === "recording";
  const last = !session.id ? app.history[0] : null;
  const text = session.text || last?.text || "";
  const ready = session.phase === "ready" || Boolean(last);
  return (
    <section className="preview-panel" aria-label="Prévia do ditado">
      <div className="preview-header">
        <div className="row gap">
          <span
            className={`status-dot ${recording ? "recording" : busy ? "working" : ""}`}
          />
          <span role="status">{sessionLabel(session)}</span>
        </div>
        <span className="mono muted">
          {duration(session.duration_ms || last?.duration_ms || 0)}
        </span>
      </div>
      <div className="preview-body scroll-area" tabIndex={0}>
        {text ? (
          <>
            <p className="eyebrow">
              {last ? "Último texto" : ready ? "Texto final" : "Prévia"}
            </p>
            <p className="dictation-text">{text}</p>
            {busy && (
              <p className="caption muted preview-note">
                O texto será reunido ao terminar.
              </p>
            )}
          </>
        ) : (
          <div className="preview-empty">
            <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
              <rect x="9" y="3" width="6" height="12" rx="3" />
              <path d="M5 11a7 7 0 0 0 14 0M12 18v3M8 21h8" />
            </svg>
            <h1>{busy ? "Pode falar." : "Sua voz, em texto."}</h1>
            <p>
              {busy
                ? "A prévia aparece quando os primeiros trechos ficarem prontos."
                : "Comece um ditado. O texto aparece aqui enquanto você grava."}
            </p>
          </div>
        )}
        {session.duration_ms >= 300000 && recording && (
          <p className="notice">A gravação será encerrada aos 6 minutos.</p>
        )}
        {session.error && (
          <p className="notice">
            O progresso disponível foi preservado. Confira a fila para
            continuar.
          </p>
        )}
      </div>
      <div className="preview-controls">
        <label className="mode-picker">
          <span className="caption muted">Ao terminar</span>
          <select
            aria-label="Ação ao terminar"
            value={prefs.selectedModel}
            disabled={
              !app.preferencesLoaded ||
              (busy && !recording) ||
              pending.has("model")
            }
            onChange={(e) => void app.selectModel(e.target.value)}
          >
            <option value="transcribe-only">Somente transcrever</option>
            {commandModels.map((model) => (
              <option key={model.value} value={model.value}>
                {model.label}
              </option>
            ))}
          </select>
        </label>
        <div className="row gap control-actions">
          {text && ready && (
            <button
              disabled={pending.has("copy")}
              onClick={() => void app.copy(text)}
            >
              Copiar texto
            </button>
          )}
          {recording && (
            <button
              className="quiet"
              disabled={pending.has("record")}
              onClick={() =>
                void app.action("record", () => call("cancel_recording"))
              }
            >
              Cancelar
            </button>
          )}
          <button
            className={`primary ${recording ? "stop" : ""}`}
            disabled={
              !app.preferencesLoaded ||
              pending.has("record") ||
              (busy && !recording)
            }
            onClick={() =>
              void app.action("record", () =>
                call(
                  recording ? "stop_recording_audio" : "start_recording_audio",
                ),
              )
            }
          >
            <span aria-hidden="true">{recording ? "■" : "●"}</span>{" "}
            {recording ? "Concluir ditado" : busy ? "Processando…" : "Gravar"}
          </button>
        </div>
      </div>
    </section>
  );
}
