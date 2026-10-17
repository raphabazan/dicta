# Implementação local — 17/10/2026

Versão preparada: **0.5.0**. O instalado continua **0.4.3**. Este documento registra implementação e testes locais; não encerra o aceite do change.

Após esse registro, Rapha autorizou o push e a release 0.5.0 para teste manual próprio. O aceite operacional continua pendente e está identificado nas notas da versão em `.github/release-notes/v0.5.0.md`.

## Comportamento implementado

- Envio opcional durante a gravação. Alvos de 10/20/30 s, pausa local de 300/500/700 ms, teto de alvo + 10 s e sobreposição de 500 ms somente no corte forçado. O stop fecha a cauda imediatamente.
- Captura CPAL com confirmação de início, conversão explícita de f32/i16/u16 e canais para mono, taxa real até a conversão para o provedor e canal limitado. Perda de frames ou erro de dispositivo tornam a sessão incompleta.
- Spool PCM local e WAVs selados antes do upload, no máximo duas requisições simultâneas e montagem por índice. Respostas fora de ordem não reordenam o texto.
- Um comando opcional sobre o texto inteiro e uma única tentativa automática de entrega ao concluir. Recuperação e fila antiga não colam automaticamente.
- Sessão com identidade, geração e sequência; painel e widget consultam snapshot e rejeitam eventos antigos. Gravação por atalho e stop do widget chamam o backend sem depender de listeners React.
- Coordenador serial de entrega, checagem de janela/processo, espera por modificadores soltos, verificação de clipboard e SendInput. Texto final salvo antes da tentativa de colagem.
- Snapshot conservador de formatos HGLOBAL do clipboard. Imagem em DIB, texto, HTML/RTF e arquivos são restaurados quando suportados. Formatos que não podem ser preservados bloqueiam a substituição. Cópia nova do usuário não é sobrescrita por restauração antiga.
- Eventos nativos de bloqueio e energia invalidam o destino antigo e interrompem recursos transitórios. Retomar não cola nem registra todos os atalhos novamente. Erros de registro são visíveis na interface. Há recuperação no tray.
- SQLite schema 4 com migrações transacionais/idempotentes, claims/leases, chaves únicas de resultado e recuperação após reinício. Cancelamento mantém o material recuperável; descarte explícito remove o áudio.
- Adaptadores de arquivo para Whisper/GPT-Transcribe e de comandos para Responses com GPT-5.4/GPT-6.1 Sol/GPT-4o-mini. Uso correto de `languages[]`, texto canônico e múltiplos blocos de resposta. Resposta vazia ou incompleta é erro.
- Realtime legado retirado. Não foi implementada uma nova rota Realtime nesta etapa.
- Logs JSONL com rotação, sem payload privado; identificação do build; histórico paginado; CSP definida; dependências dispensáveis retiradas e CI de verificação separada do release.

## Verificação executada

| Verificação | Resultado | O que não comprova |
|---|---|---|
| Testes Rust no Windows | 34 passaram após a revisão pré-release | Não exercitam colagem entre aplicativos nem retomada física |
| Testes de reconciliação do frontend | 5 passaram | Não substituem QA visual ou IPC completo |
| Build TypeScript/Vite | Passou | Não confirma comportamento da janela nativa |
| Build Tauri debug com frontend incorporado, sem bundle | Passou | Não instala nem valida o comportamento entre aplicativos |
| `cargo fmt --check` | Passou | — |
| Clippy, todos os targets | Sem erros, com avisos de estilo | Não equivale a ausência de defeitos |
| OpenSpec strict | Passou | Valida artefatos, não aceite operacional |
| `git diff --check` | Passou | — |
| Audit de produção | Zero alertas | Não cobre crates Rust, SO ou WebView |
| Audit completo | 7 alertas: 5 high, 2 moderate; zero critical | Restantes no toolchain Tailwind 3; correção indicada envolve major |
| API real com áudio sintético dedicado | Whisper e GPT-Transcribe retornaram transcrição; GPT-5.4 e GPT-6.1 responderam ao comando controlado | Não valida qualidade, p95, imagem, busca ou custo consolidado |

Os testes Rust incluem preservação/concorrência do clipboard simulado, rollback de escrita, falha de injeção, rejeição de destino alterado, continuidade dos segmentos, silêncio digital, cauda, conversão 16/44,1/48 kHz, parsers, Unicode, exclusividade de claim, migrações 0/1/2/3, rollback de migração e sessão antiga. O cancelamento de jobs pendentes é testado para não aguardar o timeout de 90 s do provedor.

Na revisão pré-release, uma sonda Windows reproduziu `PermissionDenied` ao chamar `sync_all` em um arquivo reaberto somente para leitura. O spool passou a reabrir o WAV com acesso de escrita antes da sincronização. Um teste adicional grava, sincroniza, renomeia e relê arquivos reais em 16/44,1/48 kHz, verificando duração e amostras. Esse caso não era coberto pelos 33 testes iniciais.

O smoke de API usou uma única fixture sintética de 14,073 s. Tempos observados: Whisper 2.253 ms, GPT-Transcribe 1.745 ms; comando GPT-5.4 2.272 ms e GPT-6.1 Sol 2.234 ms. São quatro observações exploratórias, não uma comparação estatística. A qualidade da voz sintética não foi suficiente para concluir sobre precisão dos ASRs. Nenhuma gravação privada foi enviada.

## Decisões e limites ainda abertos

1. **Padrões preservados.** Segmentação começa desligada, Whisper permanece o ASR padrão e GPT-5.4 continua a referência de comandos. Promover exige o corpus e os gates do test-plan.
2. **Colagem nativa pendente.** Computer Use foi interrompido na preparação de um editor de teste. A tentativa seguinte, autorizada pelo Rapha, voltou a retornar interrupção por Escape. Não houve validação do texto recebido por um aplicativo. A rotina usa uma janela de 750 ms antes de restaurar o clipboard; não existe ACK universal do consumidor.
3. **Episódio intermitente sem causa única comprovada.** Foram corrigidos problemas reproduzidos da rotina anterior, acrescentados diagnóstico e tratamento de retomada. Isso não prova que a falha observada após hibernação tinha uma única causa. Ainda faltam os ciclos manuais e uso prolongado.
4. **Bordas exigem fala real.** A detecção usa energia local, não um modelo semântico. O corte forçado ocorre no frame do teto e carrega sobreposição; a busca por um mínimo de atividade próximo do teto ainda não foi implementada. Reconciliação textual examina até seis palavras apenas onde há sobreposição conhecida. Repetições, nomes, negações e últimas palavras nas bordas continuam como gate aberto. Não há hash persistido de cada segmento nesta etapa.
5. **Identidade do destino limitada.** A validação usa HWND/processo e época da sessão interativa. Não inspeciona campo, cursor ou aba dentro da mesma janela.
6. **Custos e credenciais.** A interface continua rotulando custos estimados. Tokens de Responses entram no diagnóstico, mas ainda não há contabilidade completa por etapa. A chave permanece em ambiente/.env; migração para Credential Manager está pendente.
7. **Matriz de falhas incompleta.** Contratos automatizados passaram; perda real de rede, disco cheio, dispositivos físicos, privilégios elevados, formatos especiais de clipboard, imagem/busca e atualização/rollback de instalador ainda exigem testes próprios.

## Teste isolado e rollback

O README descreve o build debug com frontend incorporado e `scripts/Start-IsolatedTest.ps1`. O perfil de teste é separado e registra apenas Ctrl+Alt+Shift+Espaço. O instalado não foi substituído, seu banco não foi migrado e não houve commit/push/publicação.

Antes de qualquer teste de hibernação iniciado por tarefa, aplicar o procedimento global do host para parar o Docker com segurança e confirmar o backend encerrado. Nenhuma suspensão ou hibernação foi executada nesta entrega.

Para uma futura atualização do instalado: encerrar o Dicta, copiar o perfil completo e guardar o executável anterior antes de migrar. Para rollback, preservar o perfil 0.5 e restaurar a cópia 0.4.3. Trocar apenas o modelo/modo não exige downgrade de schema.

## Referências oficiais consultadas

- [OpenAI: Speech to text](https://developers.openai.com/api/docs/guides/speech-to-text), [Realtime transcription](https://developers.openai.com/api/docs/guides/realtime-transcription) e [VAD](https://developers.openai.com/api/docs/guides/realtime-vad).
- [GPT-6.1 Sol](https://developers.openai.com/api/docs/models/gpt-6.1-sol), [GPT-Transcribe](https://developers.openai.com/api/docs/models/gpt-transcribe) e [deprecações](https://developers.openai.com/api/docs/deprecations).
- [Microsoft: SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput): retorno informa eventos inseridos; UIPI e teclas já pressionadas afetam a injeção.
- [Microsoft: EnumClipboardFormats](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enumclipboardformats): fim de enumeração e falha retornam zero e precisam ser distinguidos com GetLastError. Essa distinção foi implementada no snapshot.
- [Microsoft: FlushFileBuffers](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers): a sincronização exige um handle aberto com acesso de escrita. O contrato do spool foi corrigido e recebeu teste real de filesystem antes da release.
