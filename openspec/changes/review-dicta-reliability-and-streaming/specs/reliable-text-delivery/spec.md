## Purpose

Garantir que o texto produzido pelo Dicta possa ser entregue ou recuperado sem perda, concorrência entre operações ou confirmação enganosa de colagem.

## ADDED Requirements

### Requirement: Entrega serializada
O sistema SHALL executar no máximo uma tentativa de entrega por vez, compartilhando a mesma coordenação entre ditado, comandos e recuperação por atalho.

#### Scenario: Resultado chega durante recuperação manual
- **WHEN** uma transcrição termina enquanto Alt+Shift+Z está entregando outro texto
- **THEN** o sistema serializa as tentativas sem misturar textos nem restaurar o clipboard de outra tentativa

### Requirement: Resultado honesto e recuperável
O sistema SHALL distinguir envio de teclado, confirmação quando suportada e falha recuperável. Igualdade do clipboard MUST NOT ser usada como comprovação de que o destino recebeu o texto.

#### Scenario: Destino não recebe o atalho
- **WHEN** a injeção de teclado retorna sucesso mas não há confirmação do destino
- **THEN** a interface não afirma entrega confirmada e mantém o texto disponível para recuperação

#### Scenario: Falha ambígua
- **WHEN** não é possível determinar se o destino consumiu a tentativa
- **THEN** o sistema não repete automaticamente a colagem e permite decisão manual com o texto preservado

### Requirement: Limpeza do teclado
O sistema SHALL esperar a soltura dos modificadores dentro de um prazo limitado e liberar as teclas sintéticas que ele próprio pressionou em qualquer saída, sem liberar indiscriminadamente teclas físicas do usuário.

#### Scenario: Falha depois de pressionar Ctrl
- **WHEN** a operação de inserir V falha depois do Ctrl sintético
- **THEN** o Ctrl sintético é liberado e o resultado fica recuperável

#### Scenario: Usuário mantém Alt pressionado
- **WHEN** Alt continua fisicamente pressionado até expirar o prazo
- **THEN** a tentativa é interrompida sem injetar uma combinação conflitante

### Requirement: Preservação do clipboard
O sistema SHALL preservar o clipboard anterior quando possuir snapshot restaurável e só restaurá-lo enquanto ainda detiver a tentativa atual. Conteúdo não textual MUST NOT ser substituído por uma string vazia como fallback de restauração.

#### Scenario: Imagem no clipboard
- **WHEN** existe uma imagem que não pode ser preservada pelo mecanismo disponível
- **THEN** o sistema mantém o texto recuperável e informa a limitação sem sobrescrever silenciosamente a imagem

#### Scenario: Usuário copia outro conteúdo
- **WHEN** o usuário altera o clipboard durante a tentativa
- **THEN** a restauração antiga não sobrescreve o conteúdo mais recente

### Requirement: Destino e sessão válidos
O sistema SHALL revalidar o contexto de destino e a disponibilidade da sessão interativa antes de injetar texto. Mudança de aplicativo, desktop bloqueado ou restrição de privilégio SHALL resultar em recuperação explícita, sem roubar foco silenciosamente.

#### Scenario: Usuário muda de aplicativo durante o ASR
- **WHEN** o texto termina depois de o usuário mudar o aplicativo de destino
- **THEN** o resultado fica pronto para ação manual sem colagem no novo aplicativo

### Requirement: Recuperação após retomada
O sistema SHALL reconciliar estado transitório e recursos após retomada/desbloqueio, preservando o texto. Retomada sozinha MUST NOT causar colagem ou reprocessamento duplicado.

#### Scenario: Retomada de gravação interrompida
- **WHEN** a sessão do Windows volta após suspensão durante uma operação
- **THEN** o Dicta apresenta estado real e recuperação disponível, sem permanecer preso em gravação ou entrega

#### Scenario: Atalho indisponível
- **WHEN** o registro de um atalho próprio falha ou sua revalidação identifica indisponibilidade
- **THEN** o erro fica visível e a recuperação continua acessível pelo tray
