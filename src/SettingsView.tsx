import { useEffect, useState, type ReactNode } from "react";
import { TranscriptionSettings } from "./TranscriptionSettings";
import { sessionBusy } from "./session";
import { call } from "./desktop";
import { commandModels } from "./types";
import type { Dicta } from "./useDicta";

function Toggle({
  title,
  children,
  checked,
  disabled,
  onChange,
}: {
  title: string;
  children: ReactNode;
  checked: boolean;
  disabled: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <label className="setting-row toggle-row">
      <span>
        <strong>{title}</strong>
        <small>{children}</small>
      </span>
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
      />
    </label>
  );
}
export function SettingsView({ app }: { app: Dicta }) {
  const { prefs, pending } = app;
  const disabled = !app.preferencesLoaded || sessionBusy(app.session);
  const [processes, setProcesses] = useState(prefs.processes.join(", "));
  useEffect(() => setProcesses(prefs.processes.join(", ")), [prefs.processes]);
  return (
    <section className="page settings-page scroll-area">
      <div className="page-heading">
        <div>
          <h1>Configurações</h1>
          <p>Ajustes para o seu jeito de ditar.</p>
        </div>
      </div>
      {disabled && (
        <p className="notice">
          {app.preferencesLoaded
            ? "Conclua o ditado para alterar as configurações."
            : "Carregando suas preferências…"}
        </p>
      )}
      <section className="settings-group">
        <h2>Áudio</h2>
        <label className="setting-row">
          <span>Microfone</span>
          <select
            value={prefs.microphone}
            disabled={disabled || pending.has("setting:microphone")}
            onChange={(e) =>
              void app.save(
                "microphone",
                "set_selected_microphone",
                { deviceName: e.target.value },
                e.target.value,
              )
            }
          >
            <option value="">Padrão do sistema</option>
            {prefs.microphones.map((mic) => (
              <option key={mic} value={mic}>
                {mic}
              </option>
            ))}
          </select>
        </label>
        <button
          className="quiet"
          disabled={disabled || pending.has("microphones")}
          onClick={() => void app.action("microphones", app.loadPreferences)}
        >
          Atualizar microfones
        </button>
      </section>
      <TranscriptionSettings
        config={prefs.segments}
        model={prefs.asr}
        disabled={
          disabled ||
          pending.has("setting:segments") ||
          pending.has("setting:asr")
        }
        onConfig={(config) =>
          app.save("segments", "set_segment_config", { config }, config)
        }
        onModel={(model) => app.save("asr", "set_asr_model", { model }, model)}
      />
      <section className="settings-group">
        <h2>Idiomas</h2>
        <p className="caption muted">
          Selecione os idiomas que costuma falar. Sem seleção, a detecção é
          automática.
        </p>
        <div className="language-options">
          {[
            ["pt", "Português"],
            ["en", "Inglês"],
            ["es", "Espanhol"],
            ["fr", "Francês"],
            ["de", "Alemão"],
            ["it", "Italiano"],
          ].map(([code, label]) => (
            <label key={code}>
              <input
                type="checkbox"
                checked={prefs.languages.includes(code)}
                disabled={disabled || pending.has("setting:languages")}
                onChange={() => {
                  const languages = prefs.languages.includes(code)
                    ? prefs.languages.filter((value) => value !== code)
                    : [...prefs.languages, code];
                  void app.save(
                    "languages",
                    "set_transcription_languages",
                    { languages },
                    languages,
                  );
                }}
              />
              {label}
            </label>
          ))}
        </div>
      </section>
      <section className="settings-group">
        <h2>Assistência</h2>
        <label className="setting-row">
          <span>
            Modelo de assistência
            <small>
              Usado com Ctrl + Shift + Espaço e nos comandos escritos.
            </small>
          </span>
          <select
            value={prefs.model}
            disabled={disabled || pending.has("model")}
            onChange={(e) => void app.selectModel(e.target.value)}
          >
            {commandModels.map((model) => (
              <option key={model.value} value={model.value}>
                {model.label}
              </option>
            ))}
          </select>
        </label>
        <Toggle
          title="Permitir busca na web"
          checked={prefs.web}
          disabled={disabled || pending.has("setting:web")}
          onChange={(enabled) =>
            void app.save("web", "set_web_search_enabled", { enabled }, enabled)
          }
        >
          O assistente pode buscar informações quando necessário. A busca tem
          custo adicional.
        </Toggle>
        <Toggle
          title="Ler respostas em voz alta"
          checked={prefs.tts}
          disabled={disabled || pending.has("setting:tts")}
          onChange={(enabled) =>
            void app.save("tts", "set_tts_enabled", { enabled }, enabled)
          }
        >
          Reproduz a resposta dos comandos de assistência.
        </Toggle>
        {prefs.tts && (
          <button
            className="quiet"
            onClick={() =>
              void app.action("tts-stop", () => call("stop_tts_playback"))
            }
          >
            Parar leitura
          </button>
        )}
      </section>
      <section className="settings-group">
        <h2>Modo de jogo</h2>
        <Toggle
          title="Pausar atalhos durante jogos"
          checked={prefs.game}
          disabled={disabled || pending.has("setting:game")}
          onChange={(enabled) =>
            void app.save("game", "set_game_mode_enabled", { enabled }, enabled)
          }
        >
          Bloqueia os atalhos quando um jogo da Steam ou um executável da lista
          estiver em foco.
        </Toggle>
        <form
          className="process-form"
          onSubmit={(event) => {
            event.preventDefault();
            const list = processes
              .split(",")
              .map((value) => value.trim().toLowerCase())
              .filter(Boolean);
            void app.save(
              "processes",
              "set_game_mode_processes",
              { processes: list },
              list,
            );
          }}
        >
          <label className="field">
            Outros executáveis
            <input
              value={processes}
              disabled={disabled}
              onChange={(e) => setProcesses(e.target.value)}
              placeholder="cs2.exe, valorant.exe"
            />
            <span className="caption muted">Separe os nomes por vírgula.</span>
          </label>
          <button
            type="submit"
            disabled={
              disabled ||
              pending.has("setting:processes") ||
              processes === prefs.processes.join(", ")
            }
          >
            Salvar lista
          </button>
        </form>
      </section>
      <details className="help-section">
        <summary>Atalhos e ajuda</summary>
        <dl className="shortcut-list">
          <div>
            <dt>Gravar ou concluir ditado</dt>
            <dd>
              <kbd>Ctrl</kbd> + <kbd>Espaço</kbd>
            </dd>
          </div>
          <div>
            <dt>Ditar com assistência</dt>
            <dd>
              <kbd>Ctrl</kbd> + <kbd>Shift</kbd> + <kbd>Espaço</kbd>
            </dd>
          </div>
          <div>
            <dt>Ditar com GPT-6.1 Sol</dt>
            <dd>
              <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>Espaço</kbd>
            </dd>
          </div>
          <div>
            <dt>Colar o último texto</dt>
            <dd>
              <kbd>Alt</kbd> + <kbd>Shift</kbd> + <kbd>Z</kbd>
            </dd>
          </div>
          <div>
            <dt>Abrir comando escrito</dt>
            <dd>
              <kbd>Ctrl</kbd> + <kbd>B</kbd>
            </dd>
          </div>
          <div>
            <dt>Ativar ou desativar leitura</dt>
            <dd>
              <kbd>Ctrl</kbd> + <kbd>Alt</kbd> + <kbd>S</kbd>
            </dd>
          </div>
        </dl>
        <p className="caption muted">
          Para colar automaticamente, inicie o ditado pelo atalho no campo de
          destino. O texto completo também fica salvo no histórico.
        </p>
        <p className="caption muted">
          Após suspensão ou hibernação, use a recuperação abaixo se os atalhos
          ou a colagem não responderem.
        </p>
        <button
          disabled={disabled || pending.has("runtime")}
          onClick={() =>
            void app.action(
              "runtime",
              () => call("recover_runtime"),
              "Sessão e atalhos reconciliados.",
            )
          }
        >
          Recuperar sessão e atalhos
        </button>
        {prefs.shortcuts.length > 0 && (
          <p className="notice">
            Atalhos indisponíveis: {prefs.shortcuts.join(", ")}
          </p>
        )}
      </details>
      <p className="caption muted build-label">
        Dicta · {prefs.build || "Verificando versão…"}
      </p>
    </section>
  );
}
