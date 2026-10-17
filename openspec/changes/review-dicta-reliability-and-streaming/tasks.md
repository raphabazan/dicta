## 0. Investigação e planejamento desta entrega

- [x] 0.1 Identificar checkout, alterações prévias, processo instalado e configuração em uso.
- [x] 0.2 Ler SQLite em modo read-only e coletar somente configurações permitidas/metadados.
- [x] 0.3 Consultar documentação oficial e disponibilidade de metadados dos modelos com a chave atual.
- [x] 0.4 Executar build do frontend e sondas Windows não invasivas.
- [x] 0.5 Reproduzir os defeitos da função de colagem em harness isolado e identificar o panic UTF-8 local.
- [x] 0.6 Consolidar resultado do teste Rust integral, corrigir o harness conforme o cleanup da dependência e validar o OpenSpec em modo estrito.

## 1. Baseline operacional antes da correção

- [x] 1.1 Vincular SHA do build ao executável testado e documentar diferenças do instalado. Identidade, hashes e escopo de teste em `evidence/implementation-checks.json`; executável compilado, sem aceite nativo de colagem.
- [x] 1.2 Criar fixtures de clipboard/teclado e testes de contrato no projeto, preservando os casos vermelhos encontrados.
- [ ] 1.3 Preparar destino de teste isolado e matriz Windows do test-plan; não testar colagem em documentos reais.
- [x] 1.4 Adicionar captura redigida de eventos de atalho, foco, modificadores, entrega e retomada em build diagnóstico.
- [ ] 1.5 Registrar reprodução natural da falha ou executar sessão manual controlada de retomada, com coordenação do host e proteção do Docker.
- [ ] 1.6 Medir espera por etapa em gravações de 10 s, 60 s, 180 s e 360 s; não confundir duração do áudio com latência.

## 2. Entrega confiável e retomada — P1

- [x] 2.1 Centralizar gravação, comandos e Alt+Shift+Z em um coordenador serial de entrega.
- [x] 2.2 Implementar garantia de liberação de teclas sintéticas, espera por modificadores e diagnóstico do resultado nativo.
- [x] 2.3 Revalidar contexto de destino e sessão interativa; manter resultado recuperável em mudança de foco/privilégio/bloqueio.
- [x] 2.4 Fechar política de clipboard por spike e preservar formatos; não restaurar vazio nem sobrescrever clipboard alterado pelo usuário. Implementado snapshot HGLOBAL conservador e bloqueio de formatos não restauráveis; validação nativa na tarefa 2.8.
- [x] 2.5 Separar “envio realizado”, “confirmado quando possível” e “falha recuperável”; impedir retry automático ambíguo.
- [x] 2.6 Tratar energia/desbloqueio com cleanup idempotente e revalidação; não registrar atalhos indiscriminadamente.
- [x] 2.7 Expor erros na interface e ação de recuperação pelo tray; usar `LIMIT 1` para último item.
- [ ] 2.8 Passar testes automatizados de entrega e matriz manual antes de alterar o transporte do áudio.

## 3. Captura e benchmark dos provedores — P1

- [x] 3.1 Propagar taxa/canais/formato reais da captura até ASR/fila; testar 16/44,1/48 kHz e i16/u16/f32. Conversão coberta em teste; matriz de dispositivos físicos pendente.
- [x] 3.2 Tratar início de captura com confirmação real, erros do dispositivo e encerramento por geração de sessão.
- [ ] 3.3 Criar adaptadores por capacidade para arquivo/Realtime/comandos e fixtures de contrato, inclusive resposta vazia/incompleta.
- [ ] 3.4 Fazer smoke real com áudio de teste consentido, sem usar gravações privadas do histórico; validar billing e formatos.
- [ ] 3.5 Comparar arquivo inteiro, segmentos 10/20/30 s e Realtime atual no corpus do test-plan.
- [ ] 3.6 Avaliar GPT-6.1 Sol para comandos separadamente de ASR, com GPT-5.4 como baseline.
- [ ] 3.7 Registrar mediana/p95, precisão de campos críticos, custos e escolha do padrão. Só promover após satisfazer os gates.

## 4. Ditado incremental e recuperação — P1

- [x] 4.1 Introduzir identidade/geração de sessão, spool e migração SQLite transacional testada em cópia. Testes usam bancos isolados dos schemas legados; banco instalado não foi migrado.
- [x] 4.2 Implementar alvo de duração com espera por pausa detectada localmente, teto de espera, proteção das bordas e supressão de uploads só de silêncio; manter fila limitada e no máximo duas requisições no experimento inicial.
- [ ] 4.3 Montar texto em ordem; resolver somente sobreposição conhecida e preservar repetições intencionais.
- [x] 4.4 Fazer stop enviar cauda e aguardar todos os segmentos; executar comando textual uma vez sobre o texto completo.
- [x] 4.5 Se Realtime for escolhido, substituir endpoint/protocolo legado e reconciliar finais por item com ACKs. Não escolhido nesta implementação; rota legada retirada e nova rota Realtime adiada.
- [x] 4.6 Implementar claim/lease/idempotência de fila, classificação de erros, backoff, cancelamento e deadlines por etapa.
- [x] 4.7 Preservar resultado e áudio pendentes no reinício; impedir colagem automática de recuperação antiga.
- [x] 4.8 Integrar as alterações locais anteriores da fila, corrigindo o panic UTF-8 e testando migrações 0/1/2/3 e retomada interrompida.
- [ ] 4.9 Passar perda de rede, duplicação de eventos, falhas de disco e sessões concorrentes sem perda ou duplicação de texto.

## 5. Interface, qualidade e release — P2

- [x] 5.1 Exibir fase real, segmentos pendentes, preview separado do final e texto completo pronto para copiar/colar.
- [x] 5.2 Remover listeners de colagem legados e sincronizar frontend por snapshot/eventos versionados do backend. Atalhos de gravação e stop do widget chamam o backend diretamente.
- [x] 5.3 Separar componentes e módulos tocados, sem uma refatoração global paralela ao ajuste Windows.
- [x] 5.4 Paginar histórico e registrar uso/custo real quando disponível, rotulando estimativas.
- [x] 5.5 Revisar dependências e remover ferramentas de ícones dispensáveis; atualizar em grupos com build e smoke. Vulnerabilidades restantes do toolchain Tailwind 3 documentadas; sem migração forçada de major.
- [ ] 5.6 Revisar Credential Manager, CSP/capacidades e logs privados conforme os fluxos efetivamente necessários.
- [x] 5.7 Criar CI em push/PR com build, testes de contrato e lint proporcional; manter publicação separada.
- [x] 5.8 Atualizar README, atalhos, troubleshooting e identificação do build.
- [ ] 5.9 Executar aceite do test-plan, documentar rollback e solicitar publicação somente com resultado concreto revisável.

## Progresso parcial e decisões de 17/10/2026

Checkboxes de implementação não equivalem a aceite operacional. O modo segmentado está experimental e desligado por padrão. A matriz manual de entrega (2.8), o corpus e os gates de promoção (3.5–3.7) continuam abertos. O acesso de Computer Use foi interrompido durante a preparação do destino de teste e voltou a retornar interrupção por Escape na tentativa autorizada seguinte. Nenhum teste de colagem nativa foi declarado aprovado.

- 3.3: adaptadores de arquivo e Responses implementados; Realtime atual não foi implementado.
- 3.4/3.6: smoke real de dois ASRs e dois modelos de comando passou com fixture sintética. Isso não valida qualidade de fala, busca, imagem ou cobrança consolidada.
- 4.3: montagem por índice e reconciliação de até seis palavras apenas nas bordas com sobreposição implementadas. Repetições na borda forçada ainda exigem corpus real.
- 5.4: paginação implementada, estimativas rotuladas e tokens de Responses no diagnóstico. Contabilidade de custo real por etapa ainda pendente.
- 5.6: CSP definida e logs sem payload; credenciais continuam no mecanismo existente de ambiente/.env. Credential Manager pendente.
- No fechamento da implementação local ainda não havia instalação, commit, push ou publicação. O perfil de produção permanece separado do debug.
- Posteriormente, Rapha autorizou explicitamente o push e a release 0.5.0 para realizar os testes operacionais por conta própria. Essa autorização permite publicar antes da matriz manual; as tarefas de aceite operacional permanecem abertas.

## Interface e contabilidade — 0.5.1

- [x] Prévia no topo, shell limitado ao viewport e rolagem dentro do conteúdo.
- [x] Histórico e fila compactos, com expansão de textos longos e paginação do histórico.
- [x] Preferências e ajuda em Ajustes; revisão de português e redução do contador técnico do widget.
- [x] GPT-6.1 Sol e GPT-6 Luna como seleções ativas, com normalização dos modelos anteriores.
- [x] Padrões de trechos, busca e modo de jogo ativados quando não há escolha anterior salva.
- [x] Ledger por tentativa, sem payload, schema 5 e estimativas a partir do uso retornado pela API; custos desconhecidos e históricos antigos separados.
- [x] Smoke real dos dois modelos de comando com validação de uso e custo; sem corpus de qualidade ou verificação da fatura.
- [x] QA das cinco telas em 800×600, 360×640 e 1280×800, expansão/cópia de texto longo, paginação e preferências com fixtures de desenvolvimento.

Rapha confirmou que o envio nas pausas funcionou bem após ativar a opção. A lógica dos cortes foi preservada nesta entrega; mudou apenas o padrão para configurações ausentes. As observações de 0.5.0 acima são históricas. O aceite nativo de colagem/hibernação e o corpus de qualidade continuam abertos.
