import type { SegmentConfig } from "./session";
interface Props {
  config: SegmentConfig;
  model: string;
  disabled: boolean;
  onConfig: (config: SegmentConfig) => Promise<unknown>;
  onModel: (model: string) => Promise<unknown>;
}
export function TranscriptionSettings({
  config,
  model,
  disabled,
  onConfig,
  onModel,
}: Props) {
  return (
    <section className="settings-group">
      <h2>Transcrição</h2>
      <label className="setting-row">
        <span>Modelo de transcrição</span>
        <select
          disabled={disabled}
          value={model}
          onChange={(e) => void onModel(e.target.value)}
        >
          <option value="whisper-1">Whisper</option>
          <option value="gpt-transcribe">GPT-Transcribe</option>
        </select>
      </label>
      <label className="setting-row toggle-row">
        <span>
          <strong>Transcrever enquanto gravo</strong>
          <small>
            Envia trechos nas pausas e reúne o texto uma vez ao terminar.
          </small>
        </span>
        <input
          type="checkbox"
          checked={config.enabled}
          disabled={disabled}
          onChange={(e) =>
            void onConfig({ ...config, enabled: e.target.checked })
          }
        />
      </label>
      {config.enabled && (
        <div className="two-columns setting-inset">
          <label className="field">
            Procurar pausa após
            <select
              disabled={disabled}
              value={config.target_seconds}
              onChange={(e) =>
                void onConfig({
                  ...config,
                  target_seconds: Number(e.target.value),
                })
              }
            >
              {[10, 20, 30].map((value) => (
                <option key={value} value={value}>
                  {value} segundos
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            Duração da pausa
            <select
              disabled={disabled}
              value={config.silence_ms}
              onChange={(e) =>
                void onConfig({ ...config, silence_ms: Number(e.target.value) })
              }
            >
              {Array.from(new Set([300, 500, 700, config.silence_ms]))
                .sort((a, b) => a - b)
                .map((value) => (
                  <option key={value} value={value}>
                    {value} ms
                  </option>
                ))}
            </select>
          </label>
          <p className="caption muted span-columns">
            Se a fala for contínua, o limite por trecho é{" "}
            {config.target_seconds + 10} segundos.
          </p>
        </div>
      )}
    </section>
  );
}
