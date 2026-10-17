import { useEffect, useState } from "react";
import { call } from "./desktop";
import {
  dateTime,
  duration,
  errorMessage,
  modelLabel,
  money,
  recordedCost,
  rangeStart,
} from "./format";
import type { Stats, Usage } from "./types";

const stageLabel = (stage: string) =>
  ({ asr: "Transcrição", command: "Assistência", tts: "Leitura por voz" })[
    stage
  ] ?? stage;
export function UsageView() {
  const [range, setRange] = useState("month");
  const [data, setData] = useState<{ stats: Stats; usage: Usage } | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    let disposed = false;
    let sequence = 0;
    setData(null);
    const load = async () => {
      const current = ++sequence;
      const args = { fromTs: rangeStart(range), toTs: Date.now() };
      try {
        const [stats, usage] = await Promise.all([
          call<Stats>("get_statistics", args),
          call<Usage>("get_usage_summary", args),
        ]);
        if (!disposed && current === sequence) {
          setData({ stats, usage });
          setError("");
        }
      } catch (reason) {
        if (!disposed && current === sequence) setError(errorMessage(reason));
      }
    };
    void load();
    const timer = setInterval(() => {
      if (document.visibilityState === "visible") void load();
    }, 10000);
    return () => {
      disposed = true;
      clearInterval(timer);
    };
  }, [range]);
  return (
    <section className="page scroll-area">
      <div className="page-heading">
        <div>
          <h1>Uso e custos</h1>
          <p>Atividade e consumo das chamadas à OpenAI.</p>
        </div>
        <select
          aria-label="Período de uso"
          value={range}
          onChange={(e) => setRange(e.target.value)}
        >
          {[
            ["today", "Hoje"],
            ["week", "Últimos 7 dias"],
            ["month", "Este mês"],
            ["year", "Este ano"],
            ["all", "Todo o período"],
          ].map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </select>
      </div>
      {error && (
        <p role="alert" className="notice">
          {error}
        </p>
      )}
      {!data ? (
        <p className="empty-state">Carregando uso…</p>
      ) : (
        <>
          <div className="metrics">
            <div className="metric cost-metric">
              <span>
                Custo estimado
                {data.usage.unknown_requests > 0 ? " · parcial" : ""}
              </span>
              <strong>
                {recordedCost(
                  data.usage.cost_nanos,
                  data.usage.requests,
                  data.usage.unknown_requests,
                )}
              </strong>
              <small>
                {data.usage.requests.toLocaleString("pt-BR")}{" "}
                {data.usage.requests === 1
                  ? "chamada registrada"
                  : "chamadas registradas"}
              </small>
            </div>
            <div className="metric">
              <span>Palavras</span>
              <strong>{data.stats.total_words.toLocaleString("pt-BR")}</strong>
              <small>
                {data.stats.total_transcriptions}{" "}
                {data.stats.total_transcriptions === 1
                  ? "texto salvo"
                  : "textos salvos"}
              </small>
            </div>
            <div className="metric">
              <span>Tempo de ditado</span>
              <strong>{duration(data.stats.total_duration_ms)}</strong>
              <small>minutos e segundos</small>
            </div>
          </div>
          {(data.usage.unknown_requests > 0 ||
            data.usage.legacy_entries > 0) && (
            <p className="caption accounting-note">
              {data.usage.unknown_requests > 0 &&
                `${data.usage.unknown_requests} ${data.usage.unknown_requests === 1 ? "chamada com custo desconhecido" : "chamadas com custo desconhecido"}. `}
              {data.usage.legacy_entries > 0 &&
                `${data.usage.legacy_entries} ${data.usage.legacy_entries === 1 ? "texto antigo" : "textos antigos"} sem registro completo de uso.`}{" "}
              Esses valores não entram na soma.
            </p>
          )}
          <div className="usage-breakdown">
            {data.usage.by_stage.map((stage) => (
              <div key={stage.stage}>
                <span>
                  {stageLabel(stage.stage)}
                  <small>
                    {stage.requests}{" "}
                    {stage.requests === 1 ? "chamada" : "chamadas"}
                    {stage.unknown_requests > 0 &&
                    stage.unknown_requests < stage.requests
                      ? " · parcial"
                      : ""}
                  </small>
                </span>
                <strong>
                  {recordedCost(
                    stage.cost_nanos,
                    stage.requests,
                    stage.unknown_requests,
                  )}
                </strong>
              </div>
            ))}
          </div>
          <details className="help-section pricing-help">
            <summary>Como o custo é calculado</summary>
            <p>
              As entradas, o contexto e as imagens entram nos tokens informados
              pela API. Tokens em cache e gravações de cache têm tarifas
              próprias. A saída já inclui os tokens de raciocínio. Cada busca na
              web é somada separadamente.
            </p>
            <p>
              A transcrição usa a duração enviada, incluindo sobreposições e
              novas tentativas. A leitura por voz usa a quantidade de
              caracteres. Uma chamada sem dados suficientes aparece como “Não
              informado”.
            </p>
            <p className="caption muted">
              Valores em USD pelas tarifas padrão de 17/10/2026. São estimativas
              de uso; a cobrança final depende da OpenAI. O registro completo
              começa nesta versão.
            </p>
          </details>
          <div className="section-title">
            <h2>Chamadas recentes</h2>
            <span className="caption muted">Até 100 no período</span>
          </div>
          {!data.usage.recent.length ? (
            <p className="empty-state">As novas chamadas aparecerão aqui.</p>
          ) : (
            <div className="entry-list">
              {data.usage.recent.map((item) => (
                <details className="usage-entry" key={item.request_id}>
                  <summary>
                    <div className="entry-summary">
                      <div className="entry-meta">
                        <time>{dateTime(item.timestamp)}</time>
                        <span>{stageLabel(item.stage)}</span>
                        {item.status === 0 ? (
                          <span>Sem resposta registrada</span>
                        ) : item.status >= 300 ? (
                          <span>Falha · {item.status}</span>
                        ) : null}
                      </div>
                      <span>{modelLabel(item.model)}</span>
                    </div>
                    <strong className="usage-cost">
                      {money(item.cost_nanos)}
                    </strong>
                    <span className="chevron" aria-hidden="true">
                      ⌄
                    </span>
                  </summary>
                  <dl className="usage-details">
                    {item.stage === "command" && (
                      <>
                        <div>
                          <dt>Tokens de entrada</dt>
                          <dd>{item.input_tokens.toLocaleString("pt-BR")}</dd>
                        </div>
                        <div>
                          <dt>Em cache</dt>
                          <dd>{item.cached_tokens.toLocaleString("pt-BR")}</dd>
                        </div>
                        <div>
                          <dt>Gravação de cache</dt>
                          <dd>
                            {item.cache_write_tokens.toLocaleString("pt-BR")}
                          </dd>
                        </div>
                        <div>
                          <dt>Tokens de saída</dt>
                          <dd>{item.output_tokens.toLocaleString("pt-BR")}</dd>
                        </div>
                        <div>
                          <dt>Raciocínio, incluído na saída</dt>
                          <dd>
                            {item.reasoning_tokens.toLocaleString("pt-BR")}
                          </dd>
                        </div>
                        <div>
                          <dt>Buscas na web</dt>
                          <dd>{item.search_calls}</dd>
                        </div>
                      </>
                    )}
                    {item.stage === "asr" && (
                      <div>
                        <dt>Áudio enviado</dt>
                        <dd>
                          {(item.audio_ms / 1000).toLocaleString("pt-BR", {
                            maximumFractionDigits: 3,
                          })}{" "}
                          s
                        </dd>
                      </div>
                    )}
                    {item.stage === "tts" && (
                      <div>
                        <dt>Caracteres enviados</dt>
                        <dd>{item.characters.toLocaleString("pt-BR")}</dd>
                      </div>
                    )}
                    {item.provider_request_id && (
                      <div>
                        <dt>Identificador da chamada</dt>
                        <dd className="request-id">
                          {item.provider_request_id}
                        </dd>
                      </div>
                    )}
                  </dl>
                </details>
              ))}
            </div>
          )}
        </>
      )}
    </section>
  );
}
