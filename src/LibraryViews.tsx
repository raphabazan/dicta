import { dateTime, duration, errorMessage, modelLabel } from "./format";
import { sessionBusy } from "./session";
import type { Dicta } from "./useDicta";

export function HistoryView({ app }: { app: Dicta }) {
  return (
    <section className="page scroll-area">
      <div className="page-heading">
        <div>
          <h1>Histórico</h1>
          <p>Seus textos, do mais recente ao mais antigo.</p>
        </div>
      </div>
      {!app.history.length ? (
        <p className="empty-state">Os ditados concluídos aparecerão aqui.</p>
      ) : (
        <div className="entry-list">
          {app.history.map((entry) => (
            <details className="text-entry" key={entry.id}>
              <summary>
                <div className="entry-summary">
                  <div className="entry-meta">
                    <time>{dateTime(entry.timestamp)}</time>
                    <span>{modelLabel(entry.model)}</span>
                    {Boolean(entry.duration_ms) && (
                      <span>{duration(entry.duration_ms!)}</span>
                    )}
                  </div>
                  <p className="text-clamp">{entry.text}</p>
                </div>
                <span className="chevron" aria-hidden="true">
                  ⌄
                </span>
              </summary>
              <div className="entry-expanded">
                <p className="preserve-text">{entry.text}</p>
                <button
                  disabled={app.pending.has("copy")}
                  onClick={() => void app.copy(entry.text)}
                >
                  Copiar texto
                </button>
              </div>
            </details>
          ))}
        </div>
      )}
      {app.hasMore && (
        <button
          className="load-more"
          disabled={app.pending.has("history")}
          onClick={() => void app.loadMore()}
        >
          {app.pending.has("history") ? "Carregando…" : "Carregar anteriores"}
        </button>
      )}
    </section>
  );
}
export function QueueView({ app }: { app: Dicta }) {
  const busy = sessionBusy(app.session);
  const count = app.queue.length + app.recoveries.length;
  return (
    <section className="page scroll-area">
      <div className="page-heading">
        <div>
          <h1>Fila</h1>
          <p>
            {count
              ? `${count} ${count === 1 ? "item para retomar" : "itens para retomar"}. O progresso continua salvo.`
              : "Tudo em dia. Nenhum item pendente."}
          </p>
        </div>
      </div>
      {!count && (
        <div className="empty-state">
          <span className="empty-check" aria-hidden="true">
            ✓
          </span>
          <p>
            Os áudios e comandos que precisarem de outra tentativa aparecem
            aqui.
          </p>
        </div>
      )}
      {app.recoveries.length > 0 && (
        <div className="entry-list">
          {app.recoveries.map((item) => (
            <article className="queue-entry" key={item.id}>
              <div className="entry-main">
                <div className="entry-meta">
                  <time>{dateTime(item.created_at)}</time>
                  <span>{duration(item.duration_ms)}</span>
                </div>
                <h2>Sessão preservada</h2>
                <p className="caption muted">
                  {item.status === "incomplete"
                    ? "A captura foi interrompida. Confira o texto ao retomar."
                    : "O áudio ou comando pode ser retomado."}
                </p>
              </div>
              <div className="row gap queue-actions">
                <button
                  disabled={busy || app.pending.has(item.id)}
                  onClick={() =>
                    void app.queueAction(item.id, "retry_session", {
                      id: item.id,
                    })
                  }
                >
                  {app.pending.has(item.id) ? "Retomando…" : "Retomar"}
                </button>
                <button
                  className="quiet danger-text"
                  disabled={busy || app.pending.has(item.id)}
                  onClick={() => {
                    if (
                      confirm(
                        "Descartar o áudio ou comando pendente desta sessão?",
                      )
                    )
                      void app.queueAction(item.id, "discard_session", {
                        id: item.id,
                      });
                  }}
                >
                  Descartar
                </button>
              </div>
            </article>
          ))}
        </div>
      )}
      {app.queue.length > 0 && (
        <div className="entry-list">
          {app.queue.map((item) => (
            <article className="queue-entry" key={item.id}>
              <div className="entry-main">
                <div className="entry-meta">
                  <time>{dateTime(item.created_at)}</time>
                  <span>{modelLabel(item.model)}</span>
                </div>
                <h2>
                  {item.audio_path ? "Áudio pendente" : "Comando pendente"}
                </h2>
                {item.prompt_text && (
                  <details className="inline-details">
                    <summary>Ver texto</summary>
                    <p className="preserve-text">{item.prompt_text}</p>
                  </details>
                )}
                {item.last_error && (
                  <p className="queue-error">{errorMessage(item.last_error)}</p>
                )}
                <p className="caption muted">
                  {item.auto_retry_disabled
                    ? "Aguardando tentativa manual"
                    : "Nova tentativa automática quando possível"}{" "}
                  · {item.retry_count}{" "}
                  {item.retry_count === 1 ? "tentativa" : "tentativas"}
                </p>
              </div>
              <div className="row gap queue-actions">
                <button
                  disabled={busy || app.pending.has(`queue:${item.id}`)}
                  onClick={() =>
                    void app.queueAction(
                      `queue:${item.id}`,
                      "retry_single_queue_item",
                      { id: item.id },
                    )
                  }
                >
                  Tentar novamente
                </button>
                {item.audio_path && (
                  <button
                    className="quiet"
                    onClick={() =>
                      void app.queueAction(
                        `audio:${item.id}`,
                        "play_queue_audio",
                        { audioPath: item.audio_path },
                      )
                    }
                  >
                    Ouvir
                  </button>
                )}
                <button
                  className="quiet danger-text"
                  disabled={busy || app.pending.has(`queue:${item.id}`)}
                  onClick={() => {
                    if (confirm("Excluir este item e o áudio pendente?"))
                      void app.queueAction(
                        `queue:${item.id}`,
                        "delete_single_queue_item",
                        { id: item.id },
                      );
                  }}
                >
                  Excluir
                </button>
              </div>
            </article>
          ))}
        </div>
      )}
      {busy && count > 0 && (
        <p className="caption muted">
          Conclua o ditado atual para retomar ou descartar sessões.
        </p>
      )}
    </section>
  );
}
