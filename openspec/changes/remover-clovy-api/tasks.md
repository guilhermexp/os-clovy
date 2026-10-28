# Tasks

## 1. Pré-requisito

- [ ] 1.1 Concluir a fase 4 de `cli-substitui-agente-nativo`: tarefas 3.2, 4.1 a 4.5 e 4.7, com a 4.6 trocada por esta change. Verificar: tarefas marcadas lá, com a evidência que cada uma pede.

## 2. Provedores diretos sem reserva

- [ ] 2.1 Copiar `note_generate.md` e `note_transcript_cleanup.md` para `src-tauri/src/prompts/` e apontar os `include_str!`, inclusive nos testes. Verificar: `cargo test --manifest-path src-tauri/Cargo.toml --locked` compila sem ler `clovy-api/`.
- [ ] 2.2 Transcrição de nota, prévia ao vivo e ditado: só Groq. Sem chave, devolver o código de chave ausente sem requisição, com a frase nova no HUD do ditado e o portão da prévia pela chave; apagar o pacer e a reserva do servidor em `processing.rs`. Verificar: testes com Groq falsa e servidor falso em `127.0.0.1:8080` com zero requisições; teste do HUD em en e pt-BR.
- [ ] 2.3 Geração e limpeza da nota e limpeza do ditado: só OpenRouter, CLI ou endpoint. Saem o ramo `/v1/notes/generate` com SSE, Auto, custo/qualidade e `cleanup_text` remoto; a migração de `ProviderRef::Clovy` para OpenRouter acontece uma vez. Verificar: testes de cada cenário de `llm-providers` e `dictation`, mais a fixture de `provider-settings.json`.
- [ ] 2.4 Aquecimento da rota só na Groq, sem `/healthz`; rotinas só em CLI, com erro claro sem Claude nem Codex. Verificar: testes de `dictation.rs` e `routines.rs`.

## 3. Catálogo e seletor de texto

- [ ] 3.1 `list_provider_models(Generation)` vindo do OpenRouter `/models` (com mapeamento, cache e tempo limite); listas curadas para transcrição, imagem e vídeo; `set_provider_model` sem efeito no chat. Verificar: testes Rust de mapeamento, falha e chat preservado.
- [ ] 3.2 Frontend:
  - sai a exigência de ferramentas no modo texto;
  - recarga após salvar ou remover a chave OpenRouter;
  - aviso de falha com nova tentativa;
  - descrição com o motor de Notas atual, em en e pt-BR;
  - saem Auto e a opção "Clovy" (que vira "OpenRouter") de `LlmProvidersSection`, `ChatEnginePicker`, `ModelPickerDialog` e `chat-engine.ts`.

  Verificar: testes de componente e `i18n-catalog.test.ts`.

## 4. Mídia direto na fal

- [ ] 4.1 Cliente da fila da fal (envio, estado, resultado, download, `requestId`, tempo limite) com regras portadas de `clovy-api/`; repontar `generate_image`, `edit_image` e `generate_video` (`mcp_server/tools/media.rs`, `providers/mod.rs`); erro de chave fal ausente. Verificar: testes com fal falsa para imagem, edição, vídeo demorado e sem chave.

## 5. Remoção do que só o servidor fazia

- [ ] 5.1 Remover compartilhamento (`share_*`, `ShareDialog`, `share-crypto`, `share-payload`, chaves locais de share, pontos de entrada), "Verificar" em Sobre, títulos por modelo, classificador de imagem por modelo (fica a triagem por palavras), `forward_web_request`, mensagens de crédito, `provider_configured` e a constante `PROXY_PATH`. Verificar: `tauri-command-registry.test.ts` passa; os testes que fixavam esses caminhos foram apagados.
- [ ] 5.2 Apagar `clovy_api_url`, `local_identity.rs` e o que sobrar de `clovy_api.rs` depois de mover o código direto para módulos de provedor; apagar `clovy-api/`, o workflow `clovy-api.yml`, as entradas em `coverage.yml` e `dependabot.yml`, a ação `setup-rust` que lê o toolchain dele, os alvos de `Makefile` e `package.json`, e os trechos de `scripts/tauri-dev.mjs`, `scripts/tauri-before-dev.mjs`, `src-tauri/build.rs`, `clippy.toml` e `.env.example`; ajustar `technical-identity.test.mjs`. Verificar: `make verify` passa.
- [ ] 5.3 Varredura no `make verify` contra `CLOVY_API_URL`, `clovy_api_url` e as rotas `/v1/...` fora de `docs/adr/` e do arquivo de changes. Verificar: o script falha com uma ocorrência plantada e passa sem ela.

## 6. Docs e regras

- [ ] 6.1 ADR nova superando a ADR-0021 e as partes de servidor das ADRs 0008, 0013, 0015, 0062, 0065 e 0070; atualizar `AGENTS.md` e `CLAUDE.md` (regras de fronteira), `CONTEXT.md`, `README.md`, `PRIVACY.md`, `SECURITY.md`, `CONTRIBUTING.md` e `docs/{configuration,llm-providers,dictation,audio-pipeline,development,index,private-sharing-design,qa/agent-driven-integration}.md`. Verificar: busca por "Clovy API" em docs vigentes só encontra a menção histórica da ADR nova.
- [ ] 6.2 Em `cli-substitui-agente-nativo`, confirmar que o requisito "Contrato do clovy-api preservado" saiu do delta e que a tarefa 4.6 aponta para esta change. Verificar: `openspec validate cli-substitui-agente-nativo --strict`.

## 7. Verificação integrada

- [ ] 7.1 Gates: `pnpm check`, `pnpm typecheck`, `NODE_OPTIONS=--no-experimental-webstorage pnpm test`, `cargo fmt --manifest-path src-tauri/Cargo.toml --all --check`, `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings`, `pnpm test:rust` e `make verify`. Verificar: saída sem falhas novas.
- [ ] 7.2 Prova no app de dev (identificador próprio), com um servidor falso escutando `127.0.0.1:8080` e registrando requisições. Roteiro:
  - ditado com e sem chave Groq;
  - gravação de nota com motor Claude Code e com motor OpenRouter;
  - seletor "Modelo de texto" com o catálogo do OpenRouter;
  - troca de modelo mantendo o chat no Claude Code;
  - `generate_image` num turno do Claude Code com chave fal de teste (só com autorização do dono para gasto).

  O servidor falso precisa registrar zero requisições. Evidência em `.tmp/verify/remover-clovy-api/`.
- [ ] 7.3 Rodar `/Users/guilhermevarela/orchestrator.v5/scripts/review-pair.sh --gate` antes do commit final e tratar os achados. Verificar: exit 0, ou achados refutados registrados aqui com o caminho do `summary.md`.

## Workflow follow-up

- Arquivar esta change e `cli-substitui-agente-nativo` depois da revisão e atualizar o `AGENTS.md` do projeto.
- O dono decide o destino de `replace-venice-with-groq-openrouter-fal-tavily`.
