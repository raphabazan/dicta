export interface SessionSnapshot {
  id: string;
  generation: number;
  sequence: number;
  phase: string;
  total: number;
  completed: number;
  duration_ms: number;
  text: string;
  error: string | null;
  segmented: boolean;
}
export interface SegmentConfig { enabled: boolean; target_seconds: number; silence_ms: number }
export interface RecoverySession { id: string; status: string; created_at: number; duration_ms: number }
export const initialSession: SessionSnapshot = {
  id: "", generation: 0, sequence: 0, phase: "idle", total: 0, completed: 0,
  duration_ms: 0, text: "", error: null, segmented: false,
};
export function reconcileSession(current: SessionSnapshot, next: SessionSnapshot): SessionSnapshot {
  if (next.generation < current.generation) return current;
  if (next.generation === current.generation && next.sequence <= current.sequence) return current;
  return next;
}
export function sessionBusy(session: SessionSnapshot): boolean {
  return ["starting", "recording", "finalizing", "transcribing", "processing_command"].includes(session.phase);
}
export function sessionLabel(session: SessionSnapshot): string {
  const labels: Record<string, string> = {
    idle: "Pronto para gravar", starting: "Iniciando microfone…", recording: "Gravando",
    finalizing: "Fechando o último trecho…", transcribing: "Transcrevendo trechos pendentes…",
    processing_command: "Processando comando…", ready: "Texto pronto", recoverable: "Resultado recuperável",
    incomplete: "Captura incompleta — áudio preservado", cancelled: "Gravação cancelada", failed: "Não foi possível iniciar",
  };
  return labels[session.phase] ?? "Verificando sessão…";
}
export function deliveryMessage(code: string): string {
  const messages: Record<string, string> = {
    shortcut_sent: "Atalho de colagem enviado. Se o texto não entrou, use Alt+Shift+Z.",
    destination_changed: "O destino mudou. Texto salvo; use Alt+Shift+Z no campo desejado.",
    destination_unavailable: "Texto pronto. Use Alt+Shift+Z no campo desejado ou copie abaixo.",
    modifiers_held: "Solte Ctrl, Alt e Shift e use Alt+Shift+Z para recuperar o texto.",
    clipboard_busy: "Área de transferência ocupada. Texto salvo para recuperação.",
    clipboard_format_not_restorable: "O conteúdo atual do clipboard foi preservado. Copie o texto quando desejar substituí-lo.",
    keyboard_injection_failed: "O Windows não aceitou toda a sequência de colagem. Confira o destino antes de tentar novamente.",
    clipboard_changed: "O clipboard mudou durante a tentativa. O conteúdo novo foi preservado.",
    clipboard_restore: "Confira o campo de destino: o atalho pode ter sido enviado, mas a restauração do clipboard falhou.",
  };
  return messages[code] ?? "Não foi possível concluir a entrega. O texto continua salvo no histórico.";
}
