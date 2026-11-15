import { useEffect, useState } from "react";
import { useDicta } from "./useDicta";
import { SessionPanel } from "./SessionPanel";
import { HistoryView, QueueView } from "./LibraryViews";
import { SettingsView } from "./SettingsView";
import { UsageView } from "./UsageView";
import { call, on, isPreview } from "./desktop";
import { updateMessage, type UpdateStatus } from "./updates";
import { sessionBusy, sessionLabel } from "./session";

const views = [
  {
    id: "home",
    label: "Início",
    path: "M3 10 12 3l9 7v10a1 1 0 0 1-1 1h-5v-7H9v7H4a1 1 0 0 1-1-1Z",
  },
  { id: "history", label: "Histórico", path: "M4 5h16M4 12h16M4 19h10" },
  { id: "queue", label: "Fila", path: "M4 4h16v16H4ZM4 13h5l1 3h4l1-3h5" },
  { id: "usage", label: "Uso", path: "M5 20V10m7 10V4m7 16v-7" },
  { id: "settings", label: "Ajustes", path: "M4 7h16M4 17h16M8 4v6m8 4v6" },
] as const;
type View = (typeof views)[number]["id"];
export default function App() {
  const app = useDicta();
  const [view, setView] = useState<View>("home");
  const [update, setUpdate] = useState<UpdateStatus>({ sequence: 0, phase: "", version: "" });
  const updateText = updateMessage(update);
  const count = app.queue.length + app.recoveries.length;
  const busy = sessionBusy(app.session);
  useEffect(() => {
    if (isPreview) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const receive = (next: UpdateStatus) => {
      if (!disposed) setUpdate((current) => next.sequence >= current.sequence ? next : current);
    };
    void on<UpdateStatus>("update-state", receive).then(async (stop) => {
      if (disposed) { stop(); return; }
      unlisten = stop;
      receive(await call<UpdateStatus>("get_update_status"));
    }).catch(() => {
      if (!disposed) setUpdate({ sequence: 0, phase: "error", version: "" });
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);
  return (
    <div className="app-shell">
      <header className="app-header">
        <a
          className="brand"
          href="#"
          onClick={(event) => {
            event.preventDefault();
            setView("home");
          }}
          aria-label="Dicta, início"
        >
          <svg viewBox="0 0 28 28" fill="none" aria-hidden="true">
            <path d="M5 7v14M11 3v22M17 8v12M23 11v6" />
          </svg>
          <span>Dicta</span>
        </a>
        <button className="quiet header-status" onClick={() => setView("home")}>
          <span
            className={`status-dot ${app.session.phase === "recording" ? "recording" : busy ? "working" : ""}`}
          />
          {sessionLabel(app.session)}
        </button>
      </header>
      <nav className="main-nav" aria-label="Navegação principal">
        {views.map((item) => (
          <button
            key={item.id}
            className={view === item.id ? "active" : ""}
            aria-current={view === item.id ? "page" : undefined}
            onClick={() => setView(item.id)}
          >
            <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
              <path d={item.path} />
            </svg>
            <span>{item.label}</span>
            {item.id === "queue" && count > 0 && (
              <span className="count-badge">{count}</span>
            )}
          </button>
        ))}
      </nav>
      {updateText && (
        <div className="update-banner">
          <p role="status">{updateText}</p>
          {update.phase === "error" && (
            <button onClick={() => void call("check_updates").catch(() => app.setError("Não foi possível solicitar a atualização."))}>
              Tentar agora
            </button>
          )}
        </div>
      )}
      {(app.error || app.message) && (
        <div className={`feedback ${app.error ? "feedback-error" : ""}`}>
          <p role={app.error ? "alert" : "status"}>
            {app.error || app.message}
          </p>
          <button
            className="quiet"
            aria-label="Fechar aviso"
            onClick={() => {
              app.setError("");
              app.setMessage("");
            }}
          >
            ×
          </button>
        </div>
      )}
      <main className="workspace" id="main-content">
        {view === "home" && (
          <div className="home-view">
            <SessionPanel app={app} />
            <div className="home-footer">
              <span>
                <kbd>Ctrl</kbd> + <kbd>Espaço</kbd> para ditar
              </span>
              <span>
                <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>Z</kbd> para colar o
                último texto
              </span>
            </div>
          </div>
        )}
        {view === "history" && <HistoryView app={app} />}
        {view === "queue" && <QueueView app={app} />}
        {view === "usage" && <UsageView />}
        {view === "settings" && <SettingsView app={app} />}
      </main>
    </div>
  );
}
