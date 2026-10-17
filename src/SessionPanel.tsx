import { sessionLabel, type SessionSnapshot } from "./session";

export function SessionPanel({ session, onCopy }: { session: SessionSnapshot; onCopy: (text: string) => Promise<void> }) {
  if (!session.id) return null;
  const final = session.phase === "ready";
  return <section aria-live="polite" aria-label="Sessão de ditado" className="rounded-lg border border-gray-700 bg-gray-900/40 p-4 space-y-3">
    <div className="flex justify-between gap-4 text-sm">
      <span>{sessionLabel(session)}</span>
      <span className="text-gray-400">{Math.floor(session.duration_ms / 1000)} s · {session.completed}/{session.total} trechos</span>
    </div>
    {session.phase === "recording" && session.duration_ms >= 300000 && <p className="text-xs text-amber-300">Esta gravação será encerrada automaticamente aos 6 minutos.</p>}
    {session.error && <p className="text-xs text-amber-300">{session.error.startsWith("API error") ? session.error : "O progresso disponível foi preservado. Confira as sessões na fila."}</p>}
    {session.text && <>
      <p className="text-xs text-gray-400">{final ? "Texto final" : "Prévia — pode mudar até a conclusão"}</p>
      <p className="whitespace-pre-wrap break-words text-sm max-h-64 overflow-y-auto">{session.text}</p>
      {final && <button className="rounded bg-blue-600 px-3 py-2 text-sm hover:bg-blue-500" onClick={() => void onCopy(session.text)}>Copiar texto completo</button>}
    </>}
  </section>;
}
