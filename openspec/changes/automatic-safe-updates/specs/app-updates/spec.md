## ADDED Requirements

### Requirement: Consultas automáticas
O aplicativo SHALL consultar atualizações no processo nativo ao iniciar, a cada 15 minutos e após a retomada do Windows, inclusive com a janela na bandeja.

#### Scenario: Retomada com rede indisponível
- **WHEN** o Windows retoma e a consulta falha
- **THEN** o aplicativo registra a falha, apresenta seu estado e tenta novamente após um minuto

### Requirement: Instalação sem interrupção de trabalho
O aplicativo MUST verificar a assinatura do download e aguardar o término de todas as tarefas em andamento antes de instalar e reiniciar automaticamente.

#### Scenario: Atualização durante um ditado
- **WHEN** uma atualização está pronta durante captura, processamento ou colagem
- **THEN** a instalação aguarda essas operações e bloqueia a entrada de novas tarefas durante a instalação

#### Scenario: Falha de download ou instalação
- **WHEN** a atualização falha
- **THEN** o aplicativo permite continuar trabalhando e tenta novamente após um minuto

### Requirement: Isolamento de desenvolvimento
Uma build de desenvolvimento MUST manter a instalação automática desativada.

#### Scenario: Testes locais
- **WHEN** o aplicativo é executado em modo debug
- **THEN** a supervisão não instala uma release de produção
