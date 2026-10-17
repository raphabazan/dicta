# Dicta

Aplicativo Windows de ditado com Tauri 2, Rust e React. O backend local captura áudio, persiste sessões em SQLite e chama a API OpenAI diretamente. Não há servidor próprio intermediário.

Versão em desenvolvimento: **0.5.0**. A implementação e os testes automatizados estão locais. O aceite de colagem entre aplicativos, hibernação e qualidade de fala real continua pendente; consulte [o change OpenSpec](openspec/changes/review-dicta-reliability-and-streaming/tasks.md).

## Ditado

Em Configurações, escolha o modelo de transcrição: Whisper ou GPT-Transcribe. O padrão permanece Whisper até a avaliação com fala real.

A opção **Enviar trechos nas pausas enquanto gravo** é experimental e começa desligada. Quando ativada:

1. Após o alvo de 10, 20 ou 30 segundos, o Dicta procura uma pausa acústica de 300, 500 ou 700 ms.
2. O trecho é salvo localmente e enviado, com até duas requisições simultâneas. A captura continua.
3. Sem pausa, o limite é o alvo mais 10 segundos. Esse corte usa 500 ms de sobreposição para proteger a borda.
4. Ao parar, a cauda é enviada imediatamente. O backend reúne os resultados na ordem, processa o comando opcional e tenta colar o texto inteiro uma vez.

Pausa acústica não garante fim de frase ou palavra. A reconciliação das bordas ainda exige avaliação com nomes, números e repetições reais. Com a opção desligada, a gravação inteira é enviada depois de parar. O limite de gravação é de seis minutos.

Para comandos, estão disponíveis GPT-5.4, GPT-6.1 Sol e GPT-4o-mini. GPT-6.1 é um modelo de texto, separado do ASR. Busca na web é opcional e começa desligada. A rota Realtime antiga foi retirada; esta versão usa arquivos inteiros ou segmentos.

## Atalhos

| Atalho | Ação |
|---|---|
| Ctrl+Espaço | Iniciar/parar ditado |
| Ctrl+Shift+Espaço | Iniciar/parar com o modelo de comando salvo |
| Ctrl+Alt+Espaço | Iniciar/parar com GPT-5.4 |
| Alt+Shift+Z | Tentar colar o último resultado salvo |
| Ctrl+B | Abrir comando por texto/imagem |
| Ctrl+Alt+S | Ativar/desativar leitura por voz |
| Alt+Shift+S | Interromper leitura por voz |

O modo de jogo pode bloquear esses atalhos. Conflitos de registro aparecem na janela principal e no log. Fechar a janela minimiza para o tray; **Sair** encerra o processo.

## Colagem e recuperação

Gravações, comandos e Alt+Shift+Z usam a mesma rotina serial de entrega. Ela aguarda a liberação dos modificadores, verifica o destino e preserva formatos restauráveis do clipboard. Se o foco mudou, a sessão foi bloqueada ou o formato original não pode ser preservado, o texto permanece no histórico para recuperação explícita.

“Atalho enviado” significa que o Windows recebeu a sequência de entrada. O Dicta não consegue confirmar universalmente que outro aplicativo consumiu a colagem. Não há repetição automática de uma tentativa ambígua.

Pendências aparecem na fila. **Recuperar** conclui o processamento e salva o texto, sem colá-lo automaticamente. Cancelar interrompe o processamento e preserva a sessão na fila; **Descartar** remove seu áudio pendente. Captura com perda de áudio é marcada como incompleta e exige revisão. A ação **Recuperar sessão e colagem** no tray interrompe recursos transitórios e invalida o destino antigo; não cola texto sozinha.

## Desenvolvimento e testes

Requer Windows, WebView2, Node.js 22+, Rust estável e as ferramentas de compilação C++ para Tauri.

```powershell
rtk npm ci
rtk npm test
rtk npm run build
rtk cargo test --manifest-path src-tauri/Cargo.toml --locked
rtk cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
rtk cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked
```

Para desenvolvimento com Vite: `rtk npm run tauri dev`. O perfil de debug padrão é `%LOCALAPPDATA%\com.dicta.app-dev`, separado do perfil instalado. Para testar com a versão instalada aberta, use os atalhos isolados abaixo ou `DICTA_DISABLE_HOTKEYS=1`, evitando conflitos.

Para produzir um executável de teste com o frontend incorporado, sem instalar ou publicar:

```powershell
rtk npm run tauri build -- --debug --no-bundle
```

O resultado é `src-tauri\target\debug\dicta.exe`. Para um teste com áudio sintético e perfil dedicado:

```powershell
rtk proxy powershell.exe -NoProfile -File scripts/Generate-TestAudio.ps1 -OutputPath C:\Temp\dicta-test.wav
rtk proxy powershell.exe -NoProfile -File scripts/Start-IsolatedTest.ps1 -ProfilePath C:\Temp\dicta-test-profile -AudioPath C:\Temp\dicta-test.wav
```

Esse launcher abre a janela de teste e registra apenas **Ctrl+Alt+Shift+Espaço**. A captura usa o WAV em tempo real, sem microfone. As chamadas de transcrição continuam reais e podem gerar cobrança. Áudio sintético testa o contrato; não determina qualidade de transcrição. Essas opções de fixture existem somente em builds de debug.

## Dados, chave e diagnóstico

O perfil de produção é `%LOCALAPPDATA%\com.dicta.app`. Contém `dicta.db`, sessões pendentes e logs. `DICTA_DATA_DIR` permite selecionar outro perfil. Não aponte um build experimental para o perfil instalado.

Defina `OPENAI_API_KEY` no ambiente ou em `.env` no perfil. Debug pode consultar o `.env` do perfil instalado e, depois, o do checkout. Nunca comite a chave. Esta entrega mantém o mecanismo de credenciais existente; a migração para Windows Credential Manager continua pendente.

O SQLite contém o histórico e o conteúdo necessário para recuperar sessões; o spool guarda áudio pendente. Não são criptografados pelo aplicativo. Áudio de sessões concluídas ou explicitamente descartadas é removido. Não envie esses arquivos como diagnóstico sem revisar seu conteúdo.

`runtime.jsonl` tem rotação e registra identificação do build, estados, índices dos trechos, tempos, status da API e entrega. Não registra texto ditado, áudio, conteúdo do clipboard, chave ou título de janela. O build também aparece na interface.

## Atualização e rollback

A CI de push/PR compila o frontend, executa testes e verifica Rust. Publicação continua separada. Antes de atualizar o instalado, concluir o [test-plan](openspec/changes/review-dicta-reliability-and-streaming/test-plan.md), encerrar o aplicativo e fazer backup do perfil completo. O schema novo é 4. Para voltar a 0.4.3, preservar o perfil 0.5 e restaurar a cópia anterior; não forçar downgrade do banco nem apagar pendências.

Testes que suspendam ou hibernem o computador são manuais e coordenados. Devem respeitar as instruções do host para encerrar o Docker com segurança antes da mudança de energia.
