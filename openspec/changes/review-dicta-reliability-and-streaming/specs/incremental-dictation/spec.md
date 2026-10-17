## Purpose

Processar áudio durante a gravação e apresentar um único texto final ordenado, preservando o final da fala e a recuperação de trechos ainda pendentes.

## ADDED Requirements

### Requirement: Envio durante gravação
O sistema SHALL iniciar processamento de unidades limitadas de áudio antes do stop em gravações longas. A janela escolhida SHALL ser configurável e justificada por benchmark de qualidade, latência e custo.

#### Scenario: Ditado de três minutos
- **WHEN** o usuário continua falando além da janela configurada
- **THEN** o sistema envia áudio progressivamente e mostra progresso enquanto a captura continua

### Requirement: Corte preferencial em pausa de fala
O sistema SHALL tratar o intervalo como alvo, buscar uma pausa de fala para fechar cada trecho e respeitar um limite máximo de espera. A segmentação MUST preservar a continuidade do áudio e da captura, sem presumir que toda queda de energia seja uma fronteira de palavra.

#### Scenario: Pausa depois do alvo
- **WHEN** o trecho atinge o alvo de 20 s e ocorre uma pausa elegível antes do teto de 30 s na configuração inicial
- **THEN** o sistema fecha o trecho nessa pausa e o envia enquanto continua gravando o seguinte

#### Scenario: Fala contínua até o teto
- **WHEN** o usuário fala sem pausa elegível até o limite máximo
- **THEN** o sistema fecha um trecho dentro do limite com proteção de borda e preserva o áudio necessário ao trecho seguinte

#### Scenario: Silêncio prolongado
- **WHEN** não existe fala nova depois do fechamento de um trecho
- **THEN** o sistema não envia sucessivos trechos contendo apenas silêncio

### Requirement: Metadados de áudio corretos
O sistema SHALL preservar taxa de amostragem, canais e formato reais e converter explicitamente para o contrato do provedor sem alterar duração ou velocidade da fala.

#### Scenario: Microfone a 44,1 kHz
- **WHEN** um dispositivo captura áudio a 44,1 kHz
- **THEN** o áudio enviado e o áudio de recuperação preservam a duração e não são rotulados como 48 kHz sem conversão

### Requirement: Montagem íntegra e ordenada
O sistema SHALL montar o texto por sessão e ordem do áudio, reconciliando revisões e sobreposição conhecida. Entregas repetidas ou fora de ordem MUST NOT duplicar, reordenar ou remover falas intencionalmente repetidas.

#### Scenario: Segundo segmento termina primeiro
- **WHEN** respostas chegam em ordem diferente da captura
- **THEN** o texto final respeita a ordem da fala

#### Scenario: Repetição intencional na borda
- **WHEN** o usuário repete uma frase próxima da fronteira entre segmentos
- **THEN** a montagem remove apenas a sobreposição identificada e preserva a repetição falada

### Requirement: Finalização completa
O sistema SHALL enviar a cauda no stop e somente declarar o texto completo quando todas as unidades necessárias estiverem confirmadas ou uma falha recuperável estiver explícita.

#### Scenario: Stop no meio da fala
- **WHEN** o usuário para com áudio ainda não enviado
- **THEN** o sistema processa a cauda e inclui as últimas palavras antes de disponibilizar o final

### Requirement: Uma entrega por sessão
O sistema SHALL acumular as transcrições e solicitar no máximo uma entrega automática do texto final por sessão. Preview e resultados parciais MUST NOT ser colados automaticamente.

#### Scenario: Dez resultados parciais
- **WHEN** dez unidades de transcrição completam durante o ditado
- **THEN** nenhuma delas é colada individualmente e o stop produz um único texto montado

#### Scenario: Modo de comandos
- **WHEN** a gravação inteira representa um comando para assistência
- **THEN** o comando textual é executado uma vez sobre o ditado completo

### Requirement: Consumo limitado e recuperação
O sistema SHALL limitar memória e operações simultâneas e preservar unidades não confirmadas em falha de rede, cancelamento ou reinício, sem reutilizar respostas de outra sessão.

#### Scenario: Rede cai na metade
- **WHEN** a rede falha após alguns segmentos confirmados
- **THEN** o sistema preserva o progresso e retoma somente o trabalho necessário sem duplicar o texto confirmado

#### Scenario: Nova gravação antes da resposta antiga
- **WHEN** uma resposta antiga chega após iniciar outra gravação
- **THEN** ela não modifica o preview, o texto final nem a entrega da nova sessão
