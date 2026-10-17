## Context

O Dicta é um aplicativo Windows local. Tauri/Rust captura áudio, usa SQLite e chama a OpenAI diretamente. O React controla gravação e interface por comandos/eventos. O instalado é 0.4.3; há alterações locais de fila ainda não publicadas. Esta proposta mantém essa arquitetura e separa entrega, sessão e provedores.

## Goals / Non-Goals

Objetivos: entrega recuperável, menor espera depois de parar gravações longas, um texto final íntegro, troca de modelos verificável e diagnóstico suficiente para distinguir falhas de teclado/foco/API.

Fora desta entrega: servidor multiusuário, contas, sincronização em nuvem, novo design completo, publicação automática, teste automático que suspenda o Windows ou migração indiscriminada de dependências.

## Decisions

### 1. Corrigir entrega antes de paralelizar transcrição

Uma fila de entrega será compartilhada por gravação, comandos e Alt+Shift+Z. Terá uma tentativa ativa por vez. Retry de processamento não entrega automaticamente no aplicativo em primeiro plano. A entrega terá identificador próprio e estado separado do resultado da transcrição.

O contexto de destino será capturado no início da intenção de ditado e revalidado ao entregar. Se o usuário mudar de aplicativo, bloquear a sessão ou o destino deixar de existir, o resultado fica pronto para recuperação. Não se deve roubar foco silenciosamente nem tentar colar em desktop bloqueado.

Substituir esperas cegas por condições observáveis e deadlines curtos: modificadores soltos, sessão interativa, destino compatível e acesso ao clipboard. Usar tecla virtual adequada para Ctrl+V, verificar quantidade de eventos injetados e garantir liberação apenas das teclas sintéticas que o Dicta pressionou, inclusive em erro/cancelamento. Não soltar indiscriminadamente teclas físicas do usuário.

Não há confirmação universal de que outro aplicativo consumiu Ctrl+V. O produto deve distinguir “atalho enviado” de “texto confirmado”, usando confirmação apenas em adaptadores/destinos que a suportem. Nos demais, não inferir sucesso pela igualdade do clipboard. Não repetir automaticamente uma injeção ambígua: ela pode duplicar texto.

A política inicial de preservação será conservadora: restaurar somente quando a rotina possuir um snapshot restaurável e o clipboard ainda pertencer à tentativa. Não substituir imagem/arquivos por vazio. Se o clipboard original tiver formatos que não possam ser preservados, manter o resultado recuperável e informar a limitação antes de substituir dados. Não capturar conteúdo do clipboard nos logs. A escolha de snapshot OLE completo versus fallback sem substituição será fechada no spike Windows da fase 1.

### 2. Retomada do Windows como reconciliação

Ouvir notificações nativas de energia e sessão. Ao retomar: invalidar tentativas ligadas ao contexto anterior, encerrar recursos transitórios com cleanup, reconciliar flags com o estado real, verificar áudio e entrega quando a sessão voltar a ser interativa. Inicialização de TTS não pode ficar presa para sempre ao dispositivo que existia no startup.

Os atalhos continuam sendo registrados pelo mecanismo normal do plugin. Revalidação não significa registrar todos novamente em qualquer retomada. Inspecionar o que o plugin permite consultar; remover/recriar apenas registros próprios quando houver evidência de necessidade. Expor falhas e oferecer recuperação pelo tray. O evento automático de retomada não dispara colagem.

### 3. Transcrição incremental por segmentos como primeiro experimento

Enviar segmentos delimitados enquanto o usuário fala. O intervalo é um alvo de duração, não um corte rígido: ao atingir o alvo, procurar uma pausa de fala para fechar e enviar o trecho, sem interromper a captura. Ponto inicial de benchmark: alvos de 10, 20 e 30 s com tolerância máxima adicional de 10 s; candidato inicial de produto com alvo de 20 s e teto de 30 s. Não fixar os valores finais antes da medição. Para gravações curtas, enviar somente a cauda no stop.

Detectar atividade de voz localmente e começar avaliando uma pausa de 500 ms, comparada com 300 e 700 ms. Pausa acústica é uma aproximação: não garante fim de frase ou fronteira de palavra, e pequenas interrupções de energia dentro da fala não devem provocar cortes. Preservar as margens de áudio de ambos os lados, sem lacunas. Ao atingir o teto sem pausa elegível, escolher a região de menor atividade próxima do limite e usar a sobreposição validada para proteger palavras. Silêncio prolongado sozinho não gera novos uploads. No stop, enviar a cauda imediatamente, sem aguardar uma nova pausa.

Essa detecção pertence ao segmentador local da rota por arquivos, não depende de VAD do servidor. A rota Realtime deve respeitar as capacidades do modelo: a documentação atual exige omitir turn detection ou usar null em gpt-live-transcribe. [Contrato de VAD](https://developers.openai.com/api/docs/guides/realtime-vad).

Manter PCM com sample rate/canais/formato reais e converter explicitamente no adaptador. Um buffer limitado alimenta spool local e fila de upload limitada; até duas requisições simultâneas no experimento. Guardar `session_id`, `generation`, índice, intervalo de amostras, hash, tentativa e confirmação de cada segmento. Respostas fora de ordem não mudam a ordem do texto.

Uma pequena sobreposição, inicialmente 300–800 ms, será comparada com corte por pausa sem sobreposição. A reconciliação usa intervalos e índices; alinhamento textual só trata a região sobreposta. Não remover frases repetidas intencionalmente nem juntar transcrições por igualdade global de texto. Contexto anterior pode ser enviado como hint limitado, sem reenviar todo o áudio acumulado.

Ao parar: congelar a sessão, enviar a cauda, aguardar todos os segmentos confirmados e montar o texto final. Trechos incompletos ficam explicitamente recuperáveis. Nenhuma transcrição parcial é colada automaticamente. Modo de comandos recebe o texto inteiro uma única vez após fechar o ASR, preservando contexto e intenção global.

### 4. Realtime atual como alternativa, não reuso do código legado

Criar uma sessão de transcrição dedicada conforme protocolo atual, sem iniciar um assistente de voz. Comparar `gpt-live-transcribe` com `gpt-transcribe` em turnos confirmados onde suportado. Reconciliar itens por identidade/ordem, separar preview e texto final e aguardar acknowledgements, em vez de sleeps fixos ou apenas um novo evento.

Reconexão recupera somente trechos não confirmados, usando o spool e os identificadores locais. Se não for possível retomar exatamente, oferecer retranscrição por arquivo antes de publicar qualquer texto final. Canal de áudio limitado impede consumo ilimitado de memória durante rede lenta.

### 5. Modelos por função e capacidade

Registro tipado com três papéis: ASR de arquivo, ASR ao vivo e comandos. Cada adaptador declara endpoint, formato de áudio, hints de idioma, streaming, formato de resposta e campos disponíveis. Não reutilizar o parser de `verbose_json.words` para um modelo que retorna texto/eventos diferentes.

Candidatos: `gpt-transcribe` para arquivos/segmentos, `gpt-live-transcribe` para áudio ao vivo e `gpt-6.1-sol` para comandos. GPT-5.4 permanece baseline/fallback de comandos durante rollout. Whisper serve como baseline temporário até a migração, sem depender dele após a data anunciada de retirada.

Na avaliação de GPT-6.1 Sol, usar Responses com esforço `low` como início. Medir variantes sem busca para redação/tradução e com busca somente quando solicitada/necessária ao modo selecionado. Preservar contrato de texto final e extrair todos os blocos textuais válidos. Falha, recusa e resposta incompleta não equivalem a texto vazio bem-sucedido. A disponibilidade GET não substitui smoke de inferência e validação de ferramentas.

### 6. Sessão durável e frontend orientado a eventos

Estados propostos: `idle → recording → finalizing → transcribing → ready → delivery-requested → delivery-sent`, com saídas `recoverable`, `cancelled` e `failed`. Transcrição parcial pode ocorrer em paralelo a `recording`; contadores por segmento indicam progresso. Geração da sessão impede callbacks antigos de alterar uma nova gravação.

Eventos possuem `session_id`, sequência e versão. Ao abrir/retomar a interface, consultar snapshot do backend antes de aplicar eventos. O frontend não escreve diretamente no clipboard como parte da entrega; o backend concentra a operação.

No widget: duração, gravação ativa, segmentos confirmados/pendentes e ação de parar. Na conclusão: texto único, copiar, tentar colar e erro acionável. A tela não mostra “Ready” enquanto o backend ainda trabalha. Mostrar “recuperando” depois de retomada somente enquanto houver trabalho real.

SQLite: migrações transacionais e idempotentes, backup/rollback validado em cópia do banco. Claim da fila antes de processar, lease expirada recuperável e chave única por etapa/segmento. Persistir resultado antes de retirar áudio; remover gravações bem-sucedidas após fechamento durável. Pendências não são apagadas sem conclusão ou descarte explícito. Reinício recupera trabalho, sem colar sozinho.

### 7. Observabilidade e manutenção proporcional

Logs estruturados locais com rotação: build SHA, sessão, tentativa, transições, request ID, erros e tempos de captura/upload/ASR/comando/entrega. Sem texto, áudio, clipboard, tokens ou títulos de janela. A exportação é explícita e redigida. Histórico continua separado dos logs.

Atualizar dependências em grupos pequenos, começando por ferramentas vulneráveis/removíveis. Validar Tauri e plugins Rust/JS em conjunto. Extrair módulos durante correções funcionais para não misturar uma refatoração extensa com mudanças Windows. Acrescentar CI de PR e testes de protocolo; release permanece etapa separada.

## Risks / Trade-offs

- Segmentação reduz contexto do ASR e pode duplicar/omitir bordas; comparar com texto de referência e preservar áudio recuperável.
- Realtime pode reduzir espera, mas custa mais e depende de conexão persistente; não assumir vitória antes do benchmark.
- Não há confirmação universal da colagem entre aplicativos; comunicar envio e manter recuperação sem retry automático ambíguo.
- Retomada envolve Windows, áudio, sessão e foco; registro de atalho pode continuar válido e não deve ser culpado sem evidência.
- O instalado e o checkout diferem; inserir SHA do build antes de atribuir toda observação de produção à fonte atual.

## Rollout / Rollback

Fase 0: baseline e testes vermelhos. Fase 1: entrega/diagnóstico, com release experimental separada. Fase 2: benchmark sem mudar padrão. Fase 3: transcrição incremental atrás de configuração, mantendo rota de arquivo completo. Fase 4: escolha medida do padrão e avaliação separada do modelo de comandos. Fase 5: interface e manutenção restante.

Rollback de modo/modelo não deve rebaixar schema destrutivamente nem apagar sessões. Atualização do aplicativo instalado e publicação exigem uma etapa posterior ao aceite do plano.

## Open Questions

- Aplicativos e condições exatas em que o episódio reaparece; se gravação/atalhos continuam funcionando no mesmo instante.
- Janela de áudio e uso de sobreposição que melhor preservam PT-BR/inglês, nomes, números e correções faladas.
- Limitação do clipboard preferida quando o snapshot possui formatos não restauráveis, após demonstrar as opções no spike.
- Ganho real de qualidade/espera de GPT-6.1 Sol para comandos frequentes do Rapha.
