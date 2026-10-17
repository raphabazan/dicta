## Purpose

Permitir evolução independente dos provedores de transcrição e comandos, com contratos compatíveis e decisões medidas antes de alterar o modelo padrão.

## ADDED Requirements

### Requirement: Modelos separados por função
O sistema SHALL selecionar independentemente o modelo de arquivo, o modelo de transcrição ao vivo e o modelo de comandos. Um modelo sem suporte a áudio MUST NOT ser usado como substituição direta de ASR.

#### Scenario: Atualização do modelo de comandos
- **WHEN** GPT-6.1 Sol é habilitado para comandos
- **THEN** a transcrição continua usando o modelo de áudio selecionado e seu próprio contrato

### Requirement: Contratos por capacidade
O sistema SHALL enviar somente parâmetros suportados pelo modelo e interpretar o formato de resposta correspondente. Ausência de timestamps ou confiança MUST NOT apagar texto válido nem produzir confiança inventada.

#### Scenario: Resposta textual sem words
- **WHEN** o provedor retorna texto válido sem uma lista de palavras
- **THEN** o texto é preservado e a interface não afirma ter confiança ou timestamps inexistentes

#### Scenario: Idiomas múltiplos
- **WHEN** o modelo suporta hints por lista de idiomas
- **THEN** o sistema utiliza seu campo documentado e preserva code-switching no corpus validado

### Requirement: Migração comprovada
O sistema SHALL exigir smoke de inferência e benchmark antes de promover um modelo padrão. Consulta de metadados bem-sucedida MUST NOT ser tratada como validação de billing, inferência, precisão ou latência.

#### Scenario: Metadados disponíveis e inferência falha
- **WHEN** GET do modelo funciona mas uma chamada real falha por quota ou incompatibilidade
- **THEN** o novo modelo não é promovido e o usuário recebe erro recuperável

### Requirement: Falhas e fallback explícitos
O sistema SHALL distinguir modelo retirado, autenticação, quota, rate limit, falha transitória e resposta incompleta. Fallback MUST ser compatível com a função e não poderá repetir silenciosamente uma entrega.

#### Scenario: Modelo Realtime indisponível
- **WHEN** a API informa modelo inexistente
- **THEN** o sistema não entra em retry infinito e oferece recuperação por uma rota de áudio válida

### Requirement: Comandos completos e mensuráveis
O sistema SHALL preservar o contrato de comandos, extrair todo o texto final válido e registrar modelo e uso disponíveis. Custo estimado SHALL ser identificado como estimativa.

#### Scenario: Resposta com vários blocos
- **WHEN** uma resposta válida contém múltiplos blocos textuais
- **THEN** o usuário recebe o conteúdo completo na ordem correta, sem incluir blocos de ferramentas ou raciocínio

#### Scenario: Redação sem busca
- **WHEN** o modo de redação não exige consulta externa
- **THEN** o sistema pode executar o comando sem busca e mantém a decisão de ferramenta registrada sem payload privado
