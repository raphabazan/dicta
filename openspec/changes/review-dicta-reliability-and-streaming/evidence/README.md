# Evidência da baseline

Coleta local de 17/10/2026. Arquivos JSON contêm metadados e configurações selecionadas, sem conteúdo das transcrições ou valores de credenciais.

Os snapshots `baseline-local.json`, `baseline-windows.json` e `implementation-checks.json` ficam somente no checkout local e são ignorados pelo Git. A documentação pública conserva os resultados agregados, sem publicar metadados do perfil/computador.

- `baseline-local.json`: hashes iniciais das três fontes previamente modificadas, configurações permitidas, schema, contagens e hashes dos executáveis.
- `baseline-windows.json`: processo, teclas modificadoras e clipboard no instante saudável da coleta; eventos de energia limitados.
- `baseline-models.json`: consultas GET de metadados com status e tempo HTTP; estes tempos não medem inferência.
- `baseline-npm-production.json`: audit omitindo dependências de desenvolvimento.
- `baseline-regressions.txt`: saída do harness corrigido; um passou e três falhas esperadas na baseline.
- `baseline-checks.json`: resultados de build, teste Rust e audit.
- `baseline-clippy.txt`: avisos do lint Rust; nenhuma correção aplicada.
- A validação `openspec validate review-dicta-reliability-and-streaming --strict` passou. O status indica os quatro artefatos de planejamento prontos; isso não significa implementação concluída.
- `reproduce.cjs`: harness histórico da fonte anterior à implementação. Resolve o repositório a partir deste arquivo; as funções antigas foram substituídas e ele não representa o código atual.

Para verificar o código atual, executar `rtk cargo test --manifest-path src-tauri/Cargo.toml --locked` e `rtk npm test`. Os casos de regressão foram portados para adaptadores do projeto. O harness histórico exige a fonte anterior para reproduzir a baseline; não o usar como gate do código novo.

Não há dump do banco, gravação de áudio, `.env` ou segredo nesta pasta. A coleta de baseline não forçou suspensão ou hibernação. A instalação em uso não foi substituída durante a implementação local.

## Evidência da implementação

- `implementation-audit.json`: audit atualizado, com zero alertas de produção e sete no toolchain de desenvolvimento. As correções restantes apontam para uma migração major de Tailwind.
- `implementation-checks.json`: identificação e hash do executável de teste 0.5.0, comparação com o instalado, checks locais e metadados do smoke sem conteúdo transcrito.
- `../implementation.md`: escopo implementado, testes, limites e gates manuais restantes. A baseline acima permanece histórica.
