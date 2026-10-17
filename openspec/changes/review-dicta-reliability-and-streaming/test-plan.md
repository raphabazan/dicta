# Testes antes da implementação e critérios de aceite

## Baseline executado em 17/10/2026

| Verificação | Resultado | Limite da evidência |
|---|---|---|
| `rtk npm run build` | Passou: TypeScript e Vite 6.4.1, 53 módulos | Não testa IPC nem Windows |
| `rtk cargo test --locked --offline` | Passou, zero testes em três suites | Compilação/test harness válidos; nenhuma cobertura de comportamento própria |
| Harness com função real de colagem e adaptadores simulados | Dois contratos falharam conforme esperado; um passou com cleanup da dependência modelado | Reproduz falso positivo e perda de clipboard não textual; não reproduz o episódio real no SO |
| Harness da função local `compact_queue_error` | Panic UTF-8 reproduzido | Alteração local ainda não instalada |
| Sondas Windows read-only | Sem modificadores pressionados ou clipboard aberto na captura | Estado saudável posterior ao episódio; não exclui falha transitória |
| SQLite read-only | Schema 2, fila vazia, modo arquivo | Não houve mutação ou leitura de textos pessoais |
| GET de modelos | Novos candidatos HTTP 200; Realtime antigo HTTP 404 | Sem inferência, áudio enviado ou comprovação de billing |
| `npm audit` | 30 pacotes sinalizados; 5 critical, 15 high, 9 moderate, 1 low | Inclui ferramentas de desenvolvimento e dependências transitivas |
| `npm audit --omit=dev` | Zero vulnerabilidades reportadas | Não cobre crates Rust, binário, sistema operacional ou WebView |

`evidence/reproduce.cjs` extraiu as funções da fonte anterior à implementação, compilou um arquivo Rust independente e executou contratos de comportamento desejado. Na baseline corrigida, um passou e três falharam. O cleanup do fake representa o `Drop` verificado no Enigo 0.2.1, sem provar sucesso da liberação no SO. Esses casos foram portados para os testes dos adaptadores do projeto; o harness antigo não é um gate da fonte nova. Não são bugs observados em produção durante aquele turno.

## 1. Matriz de entrega Windows

Preparar página/campo de teste isolado com contador de eventos e texto fixo sem conteúdo pessoal. Observar valor recebido no destino; retorno de SendInput não basta. Para aplicativo nativo, usar um editor vazio com validação manual do conteúdo final.

| Caso | Procedimento | Aceite |
|---|---|---|
| Caminho básico | Ditado, comando e Alt+Shift+Z em editor e navegador | Texto esperado entra uma vez |
| Teclas mantidas | Manter Alt/Shift/Ctrl além de 1 s | Nenhuma combinação incorreta; recuperação explícita se expirar |
| Erro de injeção | Falhar cada passo de Ctrl/V/release no adaptador | Nenhuma tecla sintética fica presa |
| Foco mudou | Trocar aplicativo durante ASR e imediatamente antes de entregar | Nenhum texto entra no destino inesperado |
| Clipboard ocupado | Outro processo mantém lock por 50/150/500/2.000 ms | Deadline limitado, sem perda; erro ou recuperação visível |
| Clipboard não textual | Imagem, arquivos, HTML/RTF e clipboard vazio | Snapshot compatível preservado ou limitação explícita antes de substituir |
| Cópia concorrente | Usuário copia outro conteúdo durante entrega | O conteúdo mais recente não é sobrescrito por restauração antiga |
| Concorrência | Finalização + Alt+Shift+Z + retry manual/timer | Tentativas serializadas e nenhuma duplicação |
| Privilégios | Editor comum e elevado | Entrega suportada ou limitação explícita; não elevar o Dicta como solução padrão |
| Bloqueio/retomada | Bloquear/desbloquear e retomar em idle, captura, ASR e entrega | Estado coerente e recuperação sem colagem espontânea |
| Atalho em conflito | Outro processo registra a mesma combinação | Erro visível; ação equivalente no tray |
| Uso prolongado | Dez ciclos manuais de retomada e uso por 24–48 h | Nenhuma operação presa, perda ou duplicação |

Sessões de hibernação são manuais e coordenadas. Este turno não executou hibernação, suspensão, reinício, parada do Docker ou encerramento do app. Antes de uma futura hibernação disparada por tarefa, aplicar as instruções globais do host para parar Docker e confirmar o backend encerrado.

## 2. Corpus de áudio e benchmark

Primeiro gravar corpus de teste dedicado e consentido. Não usar automaticamente gravações privadas do histórico. Áudio sintético serve para contrato, não para escolher qualidade do ASR.

- Durações de 10, 60, 180 e 360 s; fala contínua, pausas e stop no meio da frase.
- PT-BR, inglês e mistura dos dois; Brio 500 e um segundo microfone.
- Nomes, e-mails, URLs faladas, datas, valores, números de telefone, siglas e vocabulário técnico.
- Correções faladas (“vinte, não, trinta”), negação e repetição intencional nas bordas.
- Silêncio, ruído, fala baixa e dispositivos em 16/44,1/48 kHz; mono/estéreo e formatos suportados.
- Pausa pouco antes/depois do alvo, pausas curtas dentro da fala, respiração e fala contínua até o teto: comparar detecção local de pausas de 300/500/700 ms e confirmar ausência de lacunas nas bordas.
- Silêncio prolongado após um trecho enviado não gera uploads vazios; o stop envia a cauda sem esperar silêncio adicional.
- Rede lenta, sem rede, queda no upload e após aceite remoto, timeout, 429 transitório, quota e modelo inexistente.

Comparar arquivo inteiro no baseline, segmentos com alvo de 10/20/30 s e Realtime dedicado atual. Na rota segmentada, procurar pausa após o alvo com tolerância adicional máxima de 10 s; registrar duração real e motivo do corte (pausa, teto ou stop). Usar pelo menos três repetições por arquivo/rota, ordem alternada e resultados pareados. Para estimar p95 de espera em áudios longos, reunir pelo menos 30 observações por rota; amostras menores devem ser identificadas como exploratórias.

Registrar: duração real, primeiro preview, primeira unidade confirmada, stop, áudio final enviado, último ASR confirmado, comando concluído e tentativa de entrega. Coletar WER/CER, acurácia dos campos críticos, omissões/duplicações nas bordas, uso/custo e máximo de memória. GET de modelo e duração histórica da gravação não são latência de inferência.

### Gates propostos para promoção

São metas de aceite, ainda não resultados observados:

- Zero perda/duplicação de segmentos e nenhuma resposta de sessão antiga em uma nova.
- Zero falhas de nomes/números/negações introduzidas pela montagem no conjunto de regressão anotado.
- WER mediano no corpus sem piora maior que 1 ponto percentual absoluto em relação ao baseline; avaliar cada idioma separadamente.
- Em rede estável, áudios de 180/360 s com p95 de espera stop→texto final ≤ 5 s e redução de pelo menos 50% frente ao baseline. Medir ASR puro e comandos separadamente.
- Nenhuma alegação de garantia desse SLA sob rate limit ou rede degradada; nesses casos, progresso e recuperação claros.
- No máximo duas requisições ASR simultâneas no experimento e sem crescimento de buffer proporcional a toda a sessão quando o spool está saudável.
- Despesa observada até 1,25× o custo nominal dos segmentos, excluído o comando textual, com desvio explicado por overlap/retry. Realtime, se escolhido, terá seu custo maior explicitamente aceito com base no ganho medido.

## 3. Contratos de protocolo e modelos

- Fixtures de `text` sem `words`, lista vazia, palavra sem probabilidade e Unicode/pontuação.
- Realtime com delta revisado, completed sem delta, itens fora de ordem, replay, desconexão e confirmação final tardia.
- HTTP com body inválido, streaming interrompido, recusa, resposta incompleta, múltiplos blocos de saída e falha de ferramenta.
- Smoke de GPT-6.1 Sol em redação, tradução, pergunta, imagem e busca quando habilitada; conferir parâmetros e texto completo.
- Comparar custo/mediana/p95 de comandos com GPT-5.4 sem incluir mudança simultânea de ASR.
- Falha permanente não causa retry infinito. Falha transitória usa limite/backoff e não repete colagem.

## 4. Persistência e interface

- Migrações a partir de schemas 0/1/2/3 em cópias do banco, com interrupção entre etapas e nova execução.
- Retry manual/automático concorrente, lease expirada, reinício após ASR mas antes de persistir e após persistir antes de entregar.
- Falta de espaço, arquivo ausente e formato de WAV incompatível: erro recuperável, sem remover o único áudio disponível.
- Preview não aparece como final; parar não mostra “Ready” prematuramente; atalhos e botões convergem ao mesmo estado.
- Ao reabrir janela, snapshot substitui estado obsoleto; evento antigo não ressuscita sessão cancelada.
- Histórico com milhares de itens não bloqueia recuperação da última transcrição.
- Logs exportados não contêm texto, áudio, clipboard, chave ou título de janela.
- Build de frontend, teste/lint Rust e smoke de instalação/update/rollback em perfil de teste. Só publicar depois do aceite operacional.
