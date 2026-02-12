# Dicta - Smart Voice-to-Text Dictation

**Status:** Phase 0 - Local MVP (Personal Use Only)

## O que é?

Dicta transforma sua voz em texto polido e formatado usando IA. Diferente de ferramentas básicas de transcrição, Dicta usa um pipeline de dois estágios:

1. **ASR (OpenAI Realtime API)** - Transcrição em tempo real enquanto você fala
2. **LLM (GPT-4o-mini)** - Pós-processamento que limpa, formata e adapta o texto ao seu estilo

## Configuração Inicial

### Pré-requisitos
- ✅ Node.js (já instalado)
- ✅ Rust (já instalado)
- ✅ Chave OpenAI (já configurada em `.env`)

### Instalar Dependências

Já feito! As dependências estão instaladas.

## Como Usar

### Iniciar o App em Modo Desenvolvimento

```bash
cd dicta-app
npm run tauri dev
```

### Atalhos (quando implementados)

- **Ctrl+Space** - Iniciar/Parar gravação
- **Alt+Shift+Z** - Copiar última transcrição

## Estrutura do Projeto

```
dicta-app/
├── src/                    # Frontend React
│   ├── App.tsx            # Componente principal
│   ├── main.tsx           # Entry point
│   └── index.css          # Estilos globais
├── src-tauri/             # Backend Rust
│   ├── src/
│   │   ├── main.rs        # Entry point Rust
│   │   └── lib.rs         # Lógica principal
│   ├── Cargo.toml         # Dependências Rust
│   └── tauri.conf.json    # Configuração Tauri
├── .env                   # Chave OpenAI (NÃO COMITAR!)
└── package.json           # Dependências Node
```

## Roadmap - Phase 0

- [x] Setup inicial do projeto
- [x] Configuração básica Tauri + React
- [x] Arquivo .env com API key
- [ ] Implementar hotkey global (Ctrl+Space)
- [ ] Captura de áudio do microfone
- [ ] Integração OpenAI Realtime API
- [ ] Recording widget (waveform, timer, stop)
- [ ] LLM post-processing (GPT-4o-mini)
- [ ] Auto-paste no cursor
- [ ] System tray icon

## Segurança

⚠️ **IMPORTANTE**: Sua chave da OpenAI está armazenada em `.env` e **NÃO deve ser commitada** no Git. O arquivo `.gitignore` já está configurado para ignorar este arquivo.

## Próximos Passos

Depois da Phase 0 funcionar, podemos adicionar:
- SQLite local para histórico de transcrições
- Dicionário customizável
- Settings (alterar hotkey, microfone, etc.)
- Interface para visualizar histórico

---

**Versão:** 0.1.0 (Local MVP)
**Autor:** Raphael
