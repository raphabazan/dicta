## Why

O Dicta consultava novas versões apenas ao montar a janela e ocultava falhas. Uma instância na bandeja podia continuar desatualizada indefinidamente.

## What Changes

- Supervisão nativa ao iniciar, a cada 15 minutos e 10 segundos após o evento de retomada do Windows.
- Download com assinatura verificada em segundo plano. Instalação automática com exclusão mútua contra captura, transcrição, comandos, recuperação, fila e colagem.
- Falhas visíveis, registro diagnóstico e nova tentativa após um minuto.
- Estado da atualização sincronizado com a interface e perfis de desenvolvimento impedidos de instalar releases.

## Impact

O aplicativo pode reiniciar automaticamente quando suas tarefas terminarem. A instalação usa o mecanismo assinado existente do Tauri.
