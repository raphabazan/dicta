## Purpose

Manter sessão, fila e interface consistentes diante de falhas e retomadas, disponibilizando progresso real e diagnóstico útil sem expor conteúdo pessoal.

## ADDED Requirements

### Requirement: Estado observável da sessão
O sistema SHALL representar gravação, processamento, finalização, resultado disponível e falha recuperável de forma consistente entre backend e interface. A interface MUST NOT anunciar prontidão antes de concluir o trabalho necessário.

#### Scenario: Stop retorna antes da API
- **WHEN** o comando de stop retorna enquanto o ASR continua trabalhando
- **THEN** a interface continua mostrando processamento e não declara o texto pronto

#### Scenario: Interface reconectada
- **WHEN** a interface abre ou retoma após perder eventos
- **THEN** ela consulta o estado atual e recupera progresso sem iniciar uma segunda operação

### Requirement: Fila sem processamento duplicado
O sistema SHALL impedir que retry manual e automático processem o mesmo item simultaneamente e persistir resultado antes de remover os dados necessários à recuperação.

#### Scenario: Retry manual coincide com timer
- **WHEN** os dois mecanismos tentam obter o mesmo item
- **THEN** somente um processamento o assume e o resultado é persistido uma única vez

### Requirement: Recuperação não cola sozinha
O sistema SHALL disponibilizar resultados recuperados e exigir nova intenção de entrega quando o contexto original não for mais válido. A conclusão de um item antigo MUST NOT colar no aplicativo atual por si só.

#### Scenario: Rede retorna enquanto o usuário escreve outro documento
- **WHEN** um item antigo termina após restabelecimento da conexão
- **THEN** o Dicta avisa que o texto está disponível e não interfere no documento atual

### Requirement: Diagnóstico privado
O sistema SHALL registrar identificadores, etapas, tempos e erros com rotação limitada. Logs MUST NOT conter transcrição, áudio, conteúdo do clipboard, segredo ou título de janela.

#### Scenario: Falha de clipboard
- **WHEN** uma tentativa de entrega falha
- **THEN** é possível correlacionar sessão, etapa, contexto técnico e erro sem recuperar conteúdo privado dos logs

### Requirement: Migração e reinício recuperáveis
O sistema SHALL aplicar migrações transacionais e idempotentes e preservar trabalho pendente em reinício. Texto Unicode em erros MUST NOT encerrar o processamento.

#### Scenario: Interrupção durante migração
- **WHEN** uma migração é interrompida e o aplicativo inicia novamente
- **THEN** o banco mantém estado íntegro e a migração pode ser retomada sem alterações duplicadas

#### Scenario: Erro com acento no limite do resumo
- **WHEN** um erro possui caractere multibyte na fronteira de truncamento
- **THEN** o resumo é produzido sem panic e sem invalidar o estado da fila
