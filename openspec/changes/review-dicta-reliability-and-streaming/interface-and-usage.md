# Interface e uso — continuação de 17/10/2026

Rapha confirmou que o envio por trechos funciona após ativar a configuração. Não alterar o segmentador. O novo escopo é organizar início/histórico/fila, corrigir textos, revisar padrões e trocar os modelos textuais antigos, mantendo ASR disponível.

## Arquiteturas avaliadas com frontend-design

1. Mínima: apenas prévia no início e tudo em Configurações. Menor distração, pouco contexto de pendências.
2. Equilibrada — escolhida: prévia domina o início, estado e ação em uma faixa curta; navegação persistente; histórico e fila em linhas compactas expansíveis; preferências e ajuda em Configurações.
3. Dados primeiro: dashboard de custo/contagens no início. Útil para operação em escala, mas atrapalha o ditado pessoal.

Alvo principal: janela Windows de 800×600, com validação também em 360×640 e 1280×800. Um shell ocupa a janela; somente a área de conteúdo necessária rola. Texto longo quebra linha e não desloca controles. Sistema tipográfico Segoe UI/system-ui, cinzas neutros, ação em azul #0f83fd, animações reduzidas conforme preferência do sistema.

## Uso e custos

O cálculo antigo estimava tokens pela extensão do texto final e não incluía toda a entrada nem as etapas ASR/TTS/busca. Substituir o total por um ledger por tentativa HTTP. Registrar apenas metadados: sessão, request ID, modelo, etapa, tokens de entrada/cache/saída, duração/quantidade de caracteres, chamadas de busca, status e custo em nanodólares USD.

Responses: usar `usage`, incluindo contexto/imagem já contabilizados pelo provedor, separar cache do restante da entrada e não somar raciocínio novamente à saída. Contabilizar chamadas de busca e faixas de contexto. ASR: duração efetivamente enviada por segmento/tentativa, incluindo overlap. TTS: caracteres Unicode efetivamente enviados. Falhas ambíguas ficam sem custo conhecido. Não reconstruir uso histórico a partir de texto nem somar estimativa antiga ao ledger novo. O valor continua estimado a partir da tabela pública; não equivale à fatura.

Referências: [preços](https://developers.openai.com/api/docs/pricing), [GPT-6.1 Sol](https://developers.openai.com/api/docs/models/gpt-6.1-sol), [GPT-6 Luna](https://developers.openai.com/api/docs/models/gpt-6-luna), [GPT-Transcribe](https://developers.openai.com/api/docs/models/gpt-transcribe).

## Modelos e padrões

- GPT-5.4 → GPT-6.1 Sol. GPT-4o-mini → GPT-6 Luna (esse é o identificador oficial; não há GPT-6.1 Luna no catálogo consultado).
- Preferências novas/ausentes: envio por trechos, busca na web e modo de jogo ligados. Preservar escolhas explícitas já salvas.
- Esconder o contador técnico `7/7` do widget; manter feedback simples de gravação/processamento.
- Manter o histórico e a migração de dados; somente os novos requests recebem uso detalhado.

## Verificação

- Testes de cache, entrada/saída, raciocínio, busca, duração, TTS Unicode, precisão e idempotência do ledger.
- Migração de schema 4 para 5 em banco isolado e preservação de dados anteriores.
- Build/testes/lint, smoke dos modelos e revisão visual/interação nas três dimensões, com fixtures sintéticas e sem documentos pessoais.
