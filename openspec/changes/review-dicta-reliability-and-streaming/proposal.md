## Why

O Dicta apresenta falhas intermitentes de colagem e envia a gravação inteira somente depois de parar, aumentando a espera em áudios longos. A revisão de 17/10/2026 encontrou problemas reproduzíveis na rotina de colagem e um modelo Realtime retirado da API.

## What Changes

- Primeiro estabelecer testes, métricas e diagnóstico da falha, inclusive após retomada do Windows.
- Tornar colagem automática e Alt+Shift+Z uma operação única, serializada, com recuperação e mensagens honestas sobre o resultado.
- Transcrever durante a gravação, acumular o texto e colar uma única vez ao terminar.
- Comparar segmentos enviados ao modelo de arquivo com uma sessão de transcrição Realtime atual antes de escolher o padrão.
- Separar o modelo de transcrição do modelo de comandos; avaliar GPT-6.1 Sol somente para comandos e assistência.
- Preservar áudio e progresso em falhas, impedir processamento duplicado e impedir colagem tardia pela fila.
- Mostrar etapas reais na interface, atualizar dependências por risco e criar verificação contínua.

Esta entrega contém investigação e planejamento. A aplicação das mudanças permanece pendente. Não há alteração do executável instalado, das configurações pessoais ou do banco operacional.

## Capabilities

### New Capabilities

- `reliable-text-delivery`: entrega de texto recuperável, com controle de foco, teclado, concorrência e retomada.
- `incremental-dictation`: transcrição incremental com montagem ordenada e uma única entrega final.
- `model-routing`: seleção independente e verificável dos modelos de áudio e texto.
- `recoverable-session-state`: sessões duráveis, fila segura, progresso e diagnóstico sem conteúdo privado.

### Modified Capabilities

Nenhuma especificação anterior existe neste repositório. As capacidades acima documentam comportamentos existentes e os contratos propostos.

## Impact

Backend: captura de áudio, cliente OpenAI, Realtime, SQLite, fila, atalhos e integração Windows. Frontend: tela principal, widget de gravação e janela de comandos. Infraestrutura: dependências, CI e identificação de releases. Não é necessário criar um servidor remoto para esta fase.

Ordem proposta: confiabilidade e diagnóstico → benchmark de áudio/modelos → transcrição incremental → interface e manutenção. Ver [investigation.md](investigation.md), [design.md](design.md), [test-plan.md](test-plan.md) e [tasks.md](tasks.md).
