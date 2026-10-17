# Revisão do Dicta em 17/10/2026

## Escopo e evidência

Inspeção do código local, processo instalado, configurações selecionadas e metadados do SQLite em modo somente leitura. Build de frontend, teste Rust do projeto, testes isolados de regressão e consultas de metadados de modelos. Não foram lidos textos do histórico nem enviados áudios pessoais. O processo instalado não foi reiniciado e o computador não foi suspenso.

O teste da falha intermitente ocorreu depois de Rapha informar que a colagem tinha voltado. Portanto, não foi possível capturar o estado defeituoso. Os testes isolados exercitam funções copiadas literalmente da fonte, com teclado e clipboard simulados; não são testes de integração do Windows.

### Estado confirmado

| Item | Evidência |
|---|---|
| Aplicativo instalado | `C:\Users\rapha\AppData\Local\Dicta\dicta.exe`, versão 0.4.3; processo aberto desde 04/10/2026 às 17:58, horário de São Paulo |
| Fonte auditada | `C:\Users\rapha\dicta\dicta-app`, HEAD `b8e769b`, 21/05/2026 |
| Alterações locais anteriores | `src-tauri/src/db.rs`, `src-tauri/src/lib.rs`, `src/App.tsx`: tratamento de falhas da fila ainda não commitado; preservado |
| Modo em uso | `use_realtime=false`, `selected_prompt_model=transcribe-only`, modelo de comandos salvo `gpt-5.4` |
| Microfone salvo | Brio 500; a taxa efetiva e o formato da captura não foram medidos nesta sessão |
| Banco instalado | Schema 2, 1.927 transcrições no total; fila vazia no instante da consulta |
| Última transcrição na captura inicial | 17/10/2026 às 10:01:31, duração registrada de 153,098 s; esta duração não mede espera da API |
| Histórico de ASR | 869 registros `whisper`, 792 `realtime`; os restantes são comandos |
| Retomada Windows | Nos últimos sete dias, a consulta encontrou um par de eventos 42/107 em 11/10; não prova relação com o episódio de hoje |
| Sonda atual | Chrome em primeiro plano; Ctrl, Alt, Shift e Win soltos; nenhum HWND mantendo o clipboard aberto naquele instante |
| Logs duráveis | Nenhum arquivo de log encontrado nos diretórios usuais do aplicativo; o código usa principalmente stdout/stderr, ocultos no release |

Os executáveis instalado e de build local possuem hashes diferentes. A versão 0.4.3 sozinha não identifica o mesmo artefato. O binário instalado contém as mensagens distintivas da rotina de colagem auditada, o nome GPT-5.4 e o identificador Realtime antigo. Isso aproxima a investigação do runtime, mas não substitui um build reproduzível com SHA do commit. O schema 3 está somente nas alterações locais anteriores e não deve ser considerado instalado.

## Colagem: defeitos e hipóteses

### Achados verificados na fonte e na dependência

1. **Sucesso sem comprovação.** `auto_paste_text`, em `src-tauri/src/lib.rs:47`, espera 1.000 ms, simula Ctrl+V, espera 150 ms e usa a igualdade do clipboard como sinal de sucesso. Colar normalmente não altera o clipboard. Essa observação não distingue entrega, foco errado ou ausência de campo editável. A função retorna `Ok` inclusive no teste em que nenhum destino recebe texto.
2. **Cleanup de teclado depende da biblioteca.** Se V falhar, o código sai antes da liberação explícita de Ctrl. Entretanto, Enigo 0.2.1 usa `release_keys_when_dropped=true` por padrão e tenta liberar teclas em `Drop`. O primeiro simulador não representava isso e foi corrigido. Não há reprodução válida de Ctrl preso; eventual falha da liberação nativa exige observação no Windows.
3. **Clipboard não textual perdido.** Se a leitura de texto falhar, a função assume uma string vazia. Depois pode restaurar vazio por cima de imagem ou outro formato. Reproduzido com clipboard simulado contendo imagem.
4. **Concorrência incompleta.** `paste_in_progress` protege apenas Alt+Shift+Z. Resultados da gravação e da fila chamam a mesma função sem passar por essa trava. Duas operações podem sobrescrever o clipboard uma da outra.
5. **Fila cola sem confirmar contexto.** `process_single_queue_item`, em `src-tauri/src/lib.rs:2088`, tenta colar ao concluir qualquer recuperação. Depois o item é removido mesmo que a colagem tenha falhado, pois seu erro é ignorado. Pode entregar no aplicativo errado e produzir duplicação caso o usuário tente recuperar em paralelo.
6. **Foco e retomada não modelados.** Não há identificação do destino da sessão, espera real pela soltura de modificadores ou tratamento explícito de suspensão/desbloqueio. Os atalhos são registrados no startup; falhas são apenas impressas. Isso não demonstra que o Windows perde os registros após hibernar.
7. **Erro pouco visível.** O backend emite `paste-failed` em alguns caminhos, mas não há listener correspondente no frontend. Outros caminhos só escrevem stderr. O aviso sonoro pode ocorrer mesmo sem entrega.

O atalho atual para recuperar a última transcrição é **Alt+Shift+Z**, e não Ctrl+Alt+Z. Seu handler lê o histórico completo para obter o primeiro item; uma consulta limitada seria suficiente.

### Hipóteses ainda abertas para o episódio relatado

| Hipótese | Por que é plausível | Como separar |
|---|---|---|
| Modificador ainda pressionado ou estado sintético inconsistente | Espera fixa; cleanup da biblioteca não confirma entrega nativa | Registrar somente estado de modificadores antes/depois e retorno da injeção |
| Foco mudou ou widget recebeu foco | Cola no primeiro plano atual, sem contexto de sessão | Registrar PID/HWND, sem título ou conteúdo, ao iniciar e ao entregar |
| Clipboard ocupado temporariamente | Somente três tentativas espaçadas de 50 ms | Registrar tentativas, duração, código de erro e dono do bloqueio |
| Destino elevado | Windows restringe injeção entre níveis de integridade | Matriz com editor comum/elevado e diagnóstico de limitação |
| Atalho não entregue | Registro pode falhar; falta observabilidade | Correlacionar evento de atalho com clique manual de recuperação |
| Sessão suspensa ou processamento concorrente | Estado espalhado por booleanos e tarefas | Linha do tempo por sessão, geração e tentativa de entrega |

A documentação da Microsoft confirma interferência de teclas já pressionadas e restrições de integridade em [SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput). A retomada possui um [evento próprio](https://learn.microsoft.com/en-us/windows/win32/power/pbt-apmresumeautomatic); recebê-lo não implica presença do usuário. A recuperação proposta deve aguardar sessão interativa e revalidar recursos, sem colar automaticamente ao acordar.

## Transcrição e modelos

### Caminho atual

No modo em uso, `AudioRecorder` acumula um `Vec<f32>` inteiro. Ao parar, copia o áudio, reduz a taxa declarada para 16 kHz e gera WAV mono PCM16 antes de enviar uma única requisição para `/v1/audio/transcriptions`, com `whisper-1`, `verbose_json` e timestamps por palavra. Portanto, já existe redução do áudio para upload; falta envio durante a gravação. `post_process` com GPT-4o-mini existe, mas não é chamado no caminho normal de ditado. Comandos seguem para `/v1/responses` depois do ASR.

O chamador assume entrada a 48 kHz antes da conversão, e a fila também assume 48 kHz, embora a captura use a configuração nativa do dispositivo. Um dispositivo em 44,1 kHz ou 16 kHz pode produzir duração e velocidade erradas nessa conversão. A captura normal exige f32, e o streaming exige i16, sem selecionar o tipo efetivo do dispositivo nesses caminhos. Há helpers de outros formatos que não estão conectados à captura normal. Deve-se testar troca de dispositivo e retomada antes de atribuir falhas a rede ou modelo.

O parser ignora `result.text` e reconstrói a saída por `words.join(" ")`. O campo `probability` é opcional; sua ausência mantém todas as palavras. Portanto, o filtro anunciado como 70% não pode ser tratado como confiança calibrada. Uma resposta com `text` e sem `words` gera texto vazio. Trocar apenas o nome do modelo preservando esse contrato seria incorreto.

O Realtime existente usa `gpt-4o-realtime-preview-2024-12-17`, header beta e `whisper-1`. Acumula deltas em uma string e ignora o texto dos eventos `completed`. A finalização depende de janelas fixas de 3,5/4,5 s. É necessário reconciliar por item e aguardar conclusão dos itens pendentes; deltas podem ser revisados ou chegar fora de ordem.

### Pesquisa oficial e consulta da conta

Consulta GET aos modelos, sem inferência, em 17/10/2026 às 10:08:

| Papel | Modelo | Consulta da conta | Decisão proposta |
|---|---|---|---|
| ASR atual | `whisper-1` | HTTP 200 | Baseline temporário; aposentadoria anunciada para 26/02/2027 |
| ASR por arquivo | `gpt-transcribe` | HTTP 200 | Candidato principal para segmentos e recuperação |
| ASR ao vivo | `gpt-live-transcribe` | HTTP 200 | Comparar com segmentos; não ativar a integração antiga |
| Comandos atuais | `gpt-5.4` | HTTP 200 | Baseline e fallback enquanto o substituto é avaliado |
| Comandos novos | `gpt-6.1-sol` | HTTP 200 | Testar qualidade, custo e latência; não transcreve áudio |
| Realtime antigo | `gpt-4o-realtime-preview-2024-12-17` | HTTP 404, `model_not_found` | Substituir a integração; modelo retirado em 07/05/2026 |

GET 200 prova visibilidade de metadados na conta. Não prova billing, inferência, qualidade ou acesso a todas as modalidades. Nenhum benchmark de precisão ou latência de inferência foi executado nesta etapa.

A documentação distingue [áudio gravado e áudio ao vivo](https://developers.openai.com/api/docs/guides/transcription). Para os novos modelos, há `languages` plural; o adaptador atual usa `language` singular e prompt livre. Timestamps por palavra continuam sendo uma capacidade específica que deve ser negociada, não presumida. [Transcrição de arquivos](https://developers.openai.com/api/docs/guides/speech-to-text).

Segundo as páginas dos modelos, [GPT-Transcribe](https://developers.openai.com/api/docs/models/gpt-transcribe) custa US$ 0,0045/min e [GPT-Live-Transcribe](https://developers.openai.com/api/docs/models/gpt-live-transcribe) custa US$ 0,017/min nesta consulta. São aproximadamente US$ 0,27 e US$ 1,02 por hora de áudio, antes de sobreposição/reenvio e de qualquer comando textual. Realtime é cerca de 3,8 vezes o preço; isso justifica comparar as duas rotas.

[GPT-6.1 Sol](https://developers.openai.com/api/docs/models/gpt-6.1-sol) é candidato para comandos. Não aceita áudio. Sua documentação exige esforço de raciocínio a partir de `low`; `none` e `minimal` não são suportados. O Dicta já usa Responses nesse fluxo, mas precisa testar parâmetros, ferramentas e extração completa da resposta. [Datas de retirada](https://developers.openai.com/api/docs/deprecations).

## Revisão geral de programação e interface

A interface foi revisada pela fonte e pelo build; a validação visual e funcional do aplicativo nativo permanece na matriz de aceite.

| Prioridade | Achado | Proposta |
|---|---|---|
| P1 | Colagem, fila e foco não possuem contrato único | Coordenador de entrega e resultado recuperável; corrigir antes de acelerar áudio |
| P1 | ASR antigo e modelo Realtime indisponível | Adaptadores por capacidade; benchmark antes de alterar padrão |
| P1 | Dados de captura não carregam sample rate/formato real | Buffer com metadados e conversão explícita para formatos suportados |
| P1 | Estado assíncrono sem identidade de sessão | Máquina de estados e geração por gravação; rejeitar resultados atrasados |
| P1 | HTTP sem deadlines explícitos por etapa, backoff ou classificação consistente | Limites para conexão/upload/resposta, cancelamento e retry somente quando elegível |
| P1 | Fila manual e automática sem claim atômico | Claim/lease transacional e idempotência; separar obtenção do resultado e entrega |
| P2 | Frontend mostra `Ready` logo após disparar processamento no backend | Estados “gravando”, “transcrevendo”, “finalizando”, “pronto para colar” e “recuperável” |
| P2 | Listeners legados anunciam “Pasted” sem realizar colagem | Remover fluxo legado após confirmar ausência de emissores; uma fonte de verdade no backend |
| P2 | `lib.rs` com 2.837 linhas e `App.tsx` com 1.164 | Extrair áudio, sessão, entrega, fila e componentes durante cada mudança funcional |
| P2 | Histórico inteiro carregado para cada atualização/último item | Paginação, índice compatível e consulta `LIMIT 1`; medir antes de otimizações maiores |
| P2 | Custo estimado apenas por caracteres/tempo e tabela fixa | Persistir usage real quando retornado; identificar estimativa e versão da tabela |
| P2 | `.env` em texto e logs de texto/clipboard no código | Planejar Windows Credential Manager e diagnóstico sem payload, com exportação explícita |
| P2 | Sem CI de regressão em push/PR | Build/teste de frontend e Rust, fixtures de protocolo e release separado |
| P2 | README ainda descreve MVP e funções já existentes como pendentes | Atualizar atalhos, arquitetura real, recuperação e identificação do build |
| P3 | CSP nula e permissões amplas entre janelas | Revisar capacidades por janela e CSP; validar os fluxos locais necessários |

As alterações locais anteriores acrescentam pausa de retry para erros permanentes. Devem ser integradas com testes próprios: `compact_queue_error` recorta bytes em 240 e pode causar panic em UTF-8. Reproduzido com `é` cruzando esse limite. Esse defeito pertence à fonte local não publicada; não explica o comportamento do instalado com schema 2.

`npm audit` informou 30 pacotes sinalizados, incluindo 5 critical e 15 high, no conjunto completo. `npm audit --omit=dev` informou zero. A exposição precisa ser triada por ferramenta e caminho, especialmente `to-ico`, `sharp`, Vite e suas dependências; isso não demonstra exploração nem vulnerabilidade equivalente no executável distribuído. Não executar `audit fix --force` nem migrar React/Tailwind inteiros sem necessidade.

## Resultado dos testes

Ver [test-plan.md](test-plan.md) e os arquivos de [evidência](evidence/README.md). O build do frontend passou. O harness corrigido tem um contrato aprovado e três falhas esperadas: falso sucesso, perda de clipboard não textual e panic UTF-8 da alteração local. O teste Rust integral terminou com zero testes. Clippy concluiu com 19 avisos, incluindo helpers de captura e pós-processamento sem uso. OpenSpec passou na validação estrita.

## Decisão recomendada

Começar pela confiabilidade da entrega e pela medição. Depois experimentar segmentos de áudio enviados durante a gravação, com modelo de arquivo atual e colagem única. Realtime moderno permanece alternativa medida, principalmente se vencer claramente na espera final. Atualizar o modelo de comandos é uma terceira decisão, independente das duas anteriores.

Não há evidência de que criar um backend remoto ou substituir o framework resolva os sintomas atuais.
