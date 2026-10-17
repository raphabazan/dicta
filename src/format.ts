export function duration(ms: number): string {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}
export function money(nanos: number | null): string {
  if (nanos === null) return "Não informado";
  if (nanos > 0 && nanos < 1000) return "< US$ 0,000001";
  return `US$ ${(nanos / 1e9).toLocaleString("pt-BR", { minimumFractionDigits: 2, maximumFractionDigits: 6 })}`;
}
export function dateTime(timestamp: number): string {
  return new Date(timestamp).toLocaleString("pt-BR", {
    day: "2-digit",
    month: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}
export function recordedCost(
  nanos: number,
  requests: number,
  unknown: number,
): string {
  return money(requests > 0 && requests === unknown ? null : nanos);
}
export function modelLabel(model: string | null): string {
  const labels: Record<string, string> = {
    "gpt-6.1-sol": "GPT-6.1 Sol",
    "gpt-6-luna": "GPT-6 Luna",
    "whisper-1": "Whisper",
    "gpt-transcribe": "GPT-Transcribe",
    "tts-1": "Leitura por voz",
  };
  return model ? (labels[model] ?? model) : "Ditado";
}
export function errorMessage(error: unknown): string {
  const text = String(error);
  if (/insufficient_quota|billing_hard_limit/i.test(text))
    return "A chave da OpenAI está sem saldo ou limite disponível.";
  if (/invalid_api_key|401/i.test(text)) return "Verifique a chave da OpenAI.";
  if (/api_timeout|connection|network/i.test(text))
    return "A conexão falhou. O progresso disponível continua salvo na fila.";
  if (/model_not_found|unsupported_prompt_model/i.test(text))
    return "Este modelo não está disponível para a chave configurada.";
  return text;
}
export function rangeStart(range: string, now = new Date()): number {
  const start = new Date(now);
  if (range === "all") return 0;
  if (range === "week") return now.getTime() - 7 * 86400000;
  start.setHours(0, 0, 0, 0);
  if (range === "month") start.setDate(1);
  if (range === "year") start.setMonth(0, 1);
  return start.getTime();
}
