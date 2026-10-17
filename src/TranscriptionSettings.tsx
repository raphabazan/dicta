import type { SegmentConfig } from "./session";

interface Props {
  config: SegmentConfig;
  model: string;
  disabled: boolean;
  onConfig: (config: SegmentConfig) => Promise<void>;
  onModel: (model: string) => Promise<void>;
}
export function TranscriptionSettings({ config, model, disabled, onConfig, onModel }: Props) {
  return <div className="space-y-3 border-b border-gray-700 pb-4">
    <div className="flex flex-wrap items-center justify-between gap-3">
      <label className="text-sm text-gray-300" htmlFor="asr-model">Modelo de transcrição</label>
      <select id="asr-model" disabled={disabled} value={model} onChange={e => void onModel(e.target.value)} className="rounded bg-gray-700 px-3 py-2 text-sm disabled:opacity-50">
        <option value="whisper-1">Whisper — referência atual</option>
        <option value="gpt-transcribe">GPT-Transcribe — avaliação</option>
      </select>
    </div>
    <label className="flex items-center gap-3 text-sm cursor-pointer">
      <input type="checkbox" checked={config.enabled} disabled={disabled} onChange={e => void onConfig({ ...config, enabled: e.target.checked })} />
      Enviar trechos nas pausas enquanto gravo
    </label>
    <p className="text-xs text-gray-400">Experimental. O texto é reunido e colado uma vez ao terminar. Desligado, envia a gravação inteira após parar.</p>
    {config.enabled && <div className="flex flex-wrap gap-4 text-sm">
      <label>Procurar pausa após <select disabled={disabled} value={config.target_seconds} onChange={e => void onConfig({ ...config, target_seconds: Number(e.target.value) })} className="ml-2 rounded bg-gray-700 p-1">
        {[10, 20, 30].map(value => <option key={value} value={value}>{value} s</option>)}
      </select></label>
      <label>Pausa de <select disabled={disabled} value={config.silence_ms} onChange={e => void onConfig({ ...config, silence_ms: Number(e.target.value) })} className="ml-2 rounded bg-gray-700 p-1">
        {[300, 500, 700].map(value => <option key={value} value={value}>{value} ms</option>)}
      </select></label>
      <span className="text-xs text-gray-400 self-center">Limite de {config.target_seconds + 10} s por trecho.</span>
    </div>}
  </div>;
}
