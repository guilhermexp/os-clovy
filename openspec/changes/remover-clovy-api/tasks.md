# Tasks

## 1. Pré-requisito

- [ ] 1.1 Concluir a fase 4 de `cli-substitui-agente-nativo`: tarefas 3.2, 4.1 a 4.5 e 4.7, com a 4.6 trocada por esta change. Verificar: tarefas marcadas lá, com a evidência que cada uma pede.

Implementação dos grupos2-6 liberada pela instrução do orquestrador de 2026-10-28 após o código da fase4 (commit `0693e0db`). Gates do código: Rust2141/0, clippy/fmt, frontend e regressões focadas; os dois erros Biome de configuração preexistentes permanecem fora de escopo. 1.1 continua aberta porque o bundle final, o smoke MCP e4.7 ainda não estão integralmente provados. O aceite do dono está registrado na tarefa3.3 após o primeiro smoke real; essa decisão não declara sucesso da ferramenta que falhou por socket longo.

## 2. Provedores diretos sem reserva

- [x] 2.1 Copiar `note_generate.md` e `note_transcript_cleanup.md` para `src-tauri/src/prompts/` e apontar os `include_str!`, inclusive nos testes. Verificar: `cargo test --manifest-path src-tauri/Cargo.toml --locked` compila sem ler `clovy-api/`.
- [x] 2.2 Transcrição de nota, prévia ao vivo e ditado: só Groq. Sem chave, devolver o código de chave ausente sem requisição, com a frase nova no HUD do ditado e o portão da prévia pela chave; apagar o pacer e a reserva do servidor em `processing.rs`. Verificar: testes com Groq falsa e servidor falso em `127.0.0.1:8080` com zero requisições; teste do HUD em en e pt-BR.
- [ ] 2.3 Geração e limpeza da nota e limpeza do ditado: só OpenRouter, CLI ou endpoint. Saem o ramo `/v1/notes/generate` com SSE, Auto, custo/qualidade e `cleanup_text` remoto; a migração de `ProviderRef::Clovy` para OpenRouter acontece uma vez. Verificar: testes de cada cenário de `llm-providers` e `dictation`, mais a fixture de `provider-settings.json`.
- [x] 2.4 Aquecimento da rota só na Groq, sem `/healthz`; rotinas só em CLI, com erro claro sem Claude nem Codex. Verificar: testes de `dictation.rs` e `routines.rs`.


Evidência do grupo 2 (2026-10-28): `.tmp/verify/remover-clovy-api/group2-rust-delivery.md`. RED: 6 transportes, depois 8 com o portão da prévia (`group2-red.log`, `group2-preview-red.log`); as duas migrações também falharam antes da implementação. GREEN: `cargo test --manifest-path src-tauri/Cargo.toml --locked`, 2144 passaram, 0 falhas, 22 ignorados (`group2-all-tests-complete.log`); clippy em todos os alvos e fmt saída 0. Os oito testes `direct_only_` observaram zero requisições no listener aposentado, em porta efêmera conforme autorização do orquestrador. Os testes de tempo/benchmark passaram a usar provedores falsos diretos, chaves em memória e origens injetáveis somente sob `cfg(test)`.

Frontend: 103 testes focados e 93 de HUD/i18n passaram; typecheck e Biome dos arquivos alterados passaram. A bateria completa teve 1834/1837 antes de atualizar duas guardas: o teste de identidade ainda exigia ID de operação/SSE removidos, e a guarda da identidade recusava os nomes de Keychain usados somente na aposentadoria dos segredos. Após os ajustes, os 8 testes de identidade e os 4 da guarda passaram (`group2-technical-identity-green.log`, `group2-retirement-guard-green.log`). A exceção da identidade é limitada ao módulo de aposentadoria; o módulo continua coberto pela varredura de domínios. `pnpm check`/`make verify` continuam com somente os dois erros preexistentes em `.mcp.json` e `opencode.json` (`group2-biome-all.log`, `group2-make-verify.log`).

2.3: geração/limpeza diretas e migração com gravação/recarga passaram. A tarefa permanece aberta até remover os campos Auto/custo/qualidade legados dos seletores/DTOs e do proxy que será apagado nos grupos seguintes. Nenhuma chamada real de provedor, turno Codex, prova nativa final ou prova de Keychain é declarada por estes testes.

`trail check` encontrou sete conflitos lexicais nos registros do smoke/ADRs, já presentes no corte anterior (`group2-trail-check.log`; limite de diff de 400 KB). Refutação: esses textos registram o turno Claude concluído e a falha MCP, sem declarar ferramentas provadas nem bloquear a implementação pelo smoke. O aceite do dono não equivale à aprovação do smoke incompleto. O commit registra essa distinção.

## 3. Catálogo e seletor de texto

- [x] 3.1 `list_provider_models(Generation)` vindo do OpenRouter `/models` (com mapeamento, cache e tempo limite); listas curadas para transcrição, imagem e vídeo; `set_provider_model` sem efeito no chat. Verificar: testes Rust de mapeamento, falha e chat preservado.
- [x] 3.2 Frontend:
  - sai a exigência de ferramentas no modo texto;
  - recarga após salvar ou remover a chave OpenRouter;
  - aviso de falha com nova tentativa;
  - descrição com o motor de Notas atual, em en e pt-BR;
  - saem Auto e a opção "Clovy" (que vira "OpenRouter") de `LlmProvidersSection`, `ChatEnginePicker`, `ModelPickerDialog` e `chat-engine.ts`.

  Verificar: testes de componente e `i18n-catalog.test.ts`.


Evidência do grupo 3 (2026-10-28): catálogo direto em `providers/catalog.rs`, com saída textual sem exigência de ferramentas, preço USD por milhão de tokens, cache de 10 minutos e prazo total de 10 segundos. A chave só segue para a OpenRouter, redirecionamentos ficam desligados e o corpo é limitado a 8 MiB. Salvar/remover a chave invalida o cache; o modelo escolhido não altera o motor de chat nem de Notas. Transcrição, imagem e vídeo usam listas locais curadas, visíveis também sem chave.

TDD: cinco testes de catálogo falharam antes da implementação (`group3-catalog-red.log`); a seleção independente também falhou em `group3-selection-group4-fal-red.log`. Revisão por outro agente encontrou espera no mutex fora do prazo: `group3-deadline-red.log` reproduziu uma falha; o prazo agora inclui essa espera. GREEN final: 71 testes de provedores (`group3-catalog-green-final.log`). A suíte Rust completa anterior à correção estreita do prazo passou com 2149 testes/0 falhas/22 ignorados (`group3-rust-all.log`); os alvos afetados foram repetidos depois da correção. Clippy em todos os alvos e fmt registrados em `group3-clippy-final.log` e `group3-fmt.log`.

Frontend: RED do catálogo/preço e recarga, GREEN de 77 testes/12 arquivos (`group3-frontend-green-final-3.log`) e suíte completa de 1849/1849 (`group3-frontend-all.log`). Na integração, as regras antigas escondiam mídia sem disponibilidade do servidor: seis RED e 43 GREEN confirmaram listas visíveis, falha de atualização e seleção persistida (`group3-media-red.log`, `group3-media-green.log`). Typecheck/Biome dos arquivos alterados passaram, Impeccable `[]`. `i18n-catalog.test.ts` foi criado e passou. `make verify` continua limitado pelos mesmos dois erros de configuração preexistentes (`group3-make-verify.log`).

O catálogo público foi consultado anonimamente, sem inferência: 464 modelos com saída textual, incluindo 67 sem ferramentas (`openrouter-public-models.json`). Isso não é a prova nativa do seletor, que segue em 7.2. O `trail check` repetiu os mesmos sete conflitos lexicais já refutados acima, com o mesmo limite de 400 KB (`group3-trail-check.log`).

## 4. Mídia direto na fal

- [x] 4.1 Cliente da fila da fal (envio, estado, resultado, download, `requestId`, tempo limite) com regras portadas de `clovy-api/`; repontar `generate_image`, `edit_image` e `generate_video` (`mcp_server/tools/media.rs`, `providers/mod.rs`); erro de chave fal ausente. Verificar: testes com fal falsa para imagem, edição, vídeo demorado e sem chave.

Evidência do grupo 4 (2026-10-28): cliente direto em `providers/fal/`, ferramentas MCP repontadas e geometria de edição portada (orientação EXIF, limites, quadro inteiro, padding/crop e grade de 32 pixels). A fila usa somente a chave fal; downloads sem Authorization, sem redirecionamentos e com validação DNS/endereço, limites e prazo. Repetição ou recuperação reutiliza o mesmo job; a memória de jobs é limitada e não promete sobreviver ao reinício do app.

TDD: 17 falhas iniciais (`group4-fal-red-summary.log`), RED real do caminho MCP (`group4-mcp-direct-red-executed.log`) e RED específico do prazo de vídeo (`group4-video-deadline-red.log`). GREEN: 31 testes fal (`group4-fal-green-final.log`) e 10 MCP (`group4-mcp-green-final.log`). A suíte completa posterior também passou nos testes fal (incluindo o prazo) e nos testes MCP; terminou com 2022 passados/1 falha/15 ignorados porque uma fixture de processamento ainda esperava a frase de cobrança removida pelo trabalho em andamento do grupo 5 (`group4-rust-final.log`). Essa fixture fica no grupo 5, fora deste commit. O teste completo anterior tinha identificado o mesmo tipo de acoplamento ao catálogo frontend removido. Clippy final em todos os alvos e fmt passaram (`group4-clippy-final.log`); duas APIs portadas foram ajustadas para o MSRV 1.80 sem mudar a geometria.

`make verify` encontrou somente os dois erros Biome de configuração já conhecidos (`group4-make-verify.log`). `trail check` inicial repetiu sete conflitos lexicais; a rodada final encontrou 14, ainda com limite de diff de 400 KB (`group4-trail-check-final.log`). Refutação: a nova nota de outra branch sobre análise de vídeo não proíbe ferramentas MCP no chat; essa restrição pertence somente à rota de análise de vídeo. Os docs não declaram sucesso do MCP pré-corte e não bloqueiam implementação pelo smoke. As mudanças de docs do grupo 6 ainda não entram neste commit. Fal e MCP falsos comprovaram protocolo/artefatos, chave ausente sem envio e retomada sem novo POST. Chamadas fal pagas, assets reais e prova nativa não foram executados. O helper Computer Use foi recompilado pelo comando normal após a alteração do Cargo.lock, sem lançar o app (`group4-helper-prepare.log`).

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
