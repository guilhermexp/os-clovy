# Tasks

## 1. Pré-requisito

- [ ] 1.1 Concluir a fase 4 de `cli-substitui-agente-nativo`: tarefas 3.2, 4.1 a 4.5 e 4.7, com a 4.6 trocada por esta change. Verificar: tarefas marcadas lá, com a evidência que cada uma pede.

Implementação dos grupos2-6 liberada pela instrução do orquestrador de 2026-10-28 após o código da fase4 (commit `0693e0db`). Gates do código: Rust2141/0, clippy/fmt, frontend e regressões focadas; os dois erros Biome de configuração preexistentes permanecem fora de escopo. 1.1 continua aberta porque o bundle final, o smoke MCP e4.7 ainda não estão integralmente provados. O aceite do dono está registrado na tarefa3.3 após o primeiro smoke real; essa decisão não declara sucesso da ferramenta que falhou por socket longo.

## 2. Provedores diretos sem reserva

- [x] 2.1 Copiar `note_generate.md` e `note_transcript_cleanup.md` para `src-tauri/src/prompts/` e apontar os `include_str!`, inclusive nos testes. Verificar: `cargo test --manifest-path src-tauri/Cargo.toml --locked` compila sem ler `clovy-api/`.
- [x] 2.2 Transcrição de nota, prévia ao vivo e ditado: só Groq. Sem chave, devolver o código de chave ausente sem requisição, com a frase nova no HUD do ditado e o portão da prévia pela chave; apagar o pacer e a reserva do servidor em `processing.rs`. Verificar: testes com Groq falsa e servidor falso em `127.0.0.1:8080` com zero requisições; teste do HUD em en e pt-BR.
- [x] 2.3 Geração e limpeza da nota e limpeza do ditado: só OpenRouter, CLI ou endpoint. Saem o ramo `/v1/notes/generate` com SSE, Auto, custo/qualidade e `cleanup_text` remoto; a migração de `ProviderRef::Clovy` para OpenRouter acontece uma vez. Verificar: testes de cada cenário de `llm-providers` e `dictation`, mais a fixture de `provider-settings.json`.
- [x] 2.4 Aquecimento da rota só na Groq, sem `/healthz`; rotinas só em CLI, com erro claro sem Claude nem Codex. Verificar: testes de `dictation.rs` e `routines.rs`.


Evidência do grupo 2 (2026-10-28): `.tmp/verify/remover-clovy-api/group2-rust-delivery.md`. RED: 6 transportes, depois 8 com o portão da prévia (`group2-red.log`, `group2-preview-red.log`); as duas migrações também falharam antes da implementação. GREEN: `cargo test --manifest-path src-tauri/Cargo.toml --locked`, 2144 passaram, 0 falhas, 22 ignorados (`group2-all-tests-complete.log`); clippy em todos os alvos e fmt saída 0. Os oito testes `direct_only_` observaram zero requisições no listener aposentado, em porta efêmera conforme autorização do orquestrador. Os testes de tempo/benchmark passaram a usar provedores falsos diretos, chaves em memória e origens injetáveis somente sob `cfg(test)`.

Frontend: 103 testes focados e 93 de HUD/i18n passaram; typecheck e Biome dos arquivos alterados passaram. A bateria completa teve 1834/1837 antes de atualizar duas guardas: o teste de identidade ainda exigia ID de operação/SSE removidos, e a guarda da identidade recusava os nomes de Keychain usados somente na aposentadoria dos segredos. Após os ajustes, os 8 testes de identidade e os 4 da guarda passaram (`group2-technical-identity-green.log`, `group2-retirement-guard-green.log`). A exceção da identidade é limitada ao módulo de aposentadoria; o módulo continua coberto pela varredura de domínios. `pnpm check`/`make verify` continuam com somente os dois erros preexistentes em `.mcp.json` e `opencode.json` (`group2-biome-all.log`, `group2-make-verify.log`).

2.3: geração/limpeza diretas e migração com gravação/recarga passaram. Os campos Auto/custo/qualidade legados dos seletores/DTOs e o proxy foram removidos e verificados no grupo 5 abaixo; a tarefa está concluída. Nenhuma chamada real de provedor, turno Codex, prova nativa final ou prova de Keychain é declarada por estes testes.

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

- [x] 5.1 Remover compartilhamento (`share_*`, `ShareDialog`, `share-crypto`, `share-payload`, chaves locais de share, pontos de entrada), "Verificar" em Sobre, títulos por modelo, classificador de imagem por modelo (fica a triagem por palavras), `forward_web_request`, mensagens de crédito, `provider_configured` e a constante `PROXY_PATH`. Verificar: `tauri-command-registry.test.ts` passa; os testes que fixavam esses caminhos foram apagados.
- [x] 5.2 Apagar `clovy_api_url`, `local_identity.rs` e o que sobrar de `clovy_api.rs` depois de mover o código direto para módulos de provedor; apagar `clovy-api/`, o workflow `clovy-api.yml`, as entradas em `coverage.yml` e `dependabot.yml`, a ação `setup-rust` que lê o toolchain dele, os alvos de `Makefile` e `package.json`, e os trechos de `scripts/tauri-dev.mjs`, `scripts/tauri-before-dev.mjs`, `src-tauri/build.rs`, `clippy.toml` e `.env.example`; ajustar `technical-identity.test.mjs`. Verificar: `make verify` passa.
- [x] 5.3 Varredura no `make verify` contra `CLOVY_API_URL`, `clovy_api_url` e as rotas `/v1/...` fora de `docs/adr/` e do arquivo de changes. Verificar: o script falha com uma ocorrência plantada e passa sem ela.

Evidência do grupo 5 (2026-10-28): `group5-rust-delivery.md`, `group5-build-tests-green.log` e `group5-final-frontend.log`, sob `.tmp/verify/remover-clovy-api/`. O código direto foi movido para `providers/{direct,groq,openrouter,notes}.rs`; os corpos das rotas de notas CLI/endpoint e a API `llm::generate` foram preservados. Saíram os módulos de servidor/token, compartilhamento/Verify, classificador/títulos por modelo e DTOs de crédito/custo/qualidade. A migração 53 apaga somente as chaves de compartilhamento e preserva notas/histórico e esquemas anteriores. Seu marcador distingue a adoção de bancos sem versão e é idempotente nas reparações.

TDD: quatro REDs para classificador, chaves antigas, erro 402 e DTO costQuality; RED adicional confirmou redirecionamento HTTP a outra origem (`group5-provider-redirect-red.log`). Agora notas, ditado e verificação de chave recusam redirecionamentos. O scanner teve RED de ocorrência plantada, sufixo e bearer, depois 10/10 GREEN; os testes dos alvos/launchers tiveram cinco falhas antes da remoção e 25/25 GREEN depois. A varredura real terminou sem ocorrência (`group5-scan-final.log`). Os testes HTTP conservam a origem aposentada injetável, e o teste de chat usa o mesmo nome histórico de ambiente construído apenas na fixture.

A revisão independente da mídia identificou retenção acima do limite em conclusões simultâneas (`group4-fal-readonly-review.md`). RED confirmado em `group5-fal-cache-red.log`; a conclusão agora contabiliza o cache sob o mesmo mutex, entrega o resultado ao primeiro chamador e guarda um marcador leve quando não pode reter o payload, evitando outro POST no replay. O teste do limite e a revisão posterior passaram.

GREEN integrado: Rust 2113 passados/0 falhas/21 ignorados (`group5-rust-all-verified.log`), incluindo catálogo, transporte direto, fila fal, MCP, migrações e persistência; frontend 196 arquivos/1834 testes (`group5-final-frontend.log`). Typecheck, Biome dos arquivos alterados e Impeccable passaram (`group5-final-{typecheck,biome,impeccable}.log`). Clippy em todos os alvos e fmt registrados em `group5-clippy-final.log` e `group5-fmt.log`. `make verify` executou as duas guardas sem ocorrências e parou somente nos dois erros Biome preexistentes de `.mcp.json` e `opencode.json`, preservados conforme decisão do orquestrador (`group5-make-verify.log`); não há falha nova atribuída a este grupo.

Foram removidos os 115 arquivos versionados de `clovy-api/`, seu workflow/ação/coverage/dependabot e os alvos/startup de servidor. Os dois SQL do backend foram arquivados byte a byte em `docs/history/clovy-api-migrations/`; os 39 arquivos históricos de migração desktop também foram comparados e permanecem iguais (`group5-migration-archive.log`). Arquivos locais ignorados, caches e configurações do usuário não foram apagados. O alvo de instalação do Makefile passou pelo wrapper `sfw` já exigido na regra do repositório. Nenhuma chamada paga ou prova nativa final é declarada por estes gates.

`trail check` do grupo 5 apontou quatro conflitos lexicais nos docs ainda pendentes do grupo 6, limitado aos primeiros 400 KB (`group5-trail-check.log`). Refutação: a restrição MCP da análise de vídeo não se aplica ao chat; prova nativa continua exigida sem bloquear o código autorizado. Esses docs não entram no commit do grupo 5.

## 6. Docs e regras

- [x] 6.1 ADR nova superando a ADR-0021 e as partes de servidor das ADRs 0008, 0013, 0015, 0062, 0065 e 0070; atualizar `AGENTS.md` e `CLAUDE.md` (regras de fronteira), `CONTEXT.md`, `README.md`, `PRIVACY.md`, `SECURITY.md`, `CONTRIBUTING.md` e `docs/{configuration,llm-providers,dictation,audio-pipeline,development,index,private-sharing-design,qa/agent-driven-integration}.md`. Verificar: busca por "Clovy API" em docs vigentes só encontra a menção histórica da ADR nova.
- [x] 6.2 Em `cli-substitui-agente-nativo`, confirmar que o requisito "Contrato do clovy-api preservado" saiu do delta e que a tarefa 4.6 aponta para esta change. Verificar: `openspec validate cli-substitui-agente-nativo --strict`.

Evidência do grupo 6 (2026-10-28): `group6-doc-delivery.md`, manifesto `group6-owned-docs.txt`, links/história em `group6-doc-static-checks.log` e `group5-migration-archive.log`. ADR-0073 após conferir main (0070) e reservar 0071 para a outra branch; ADR-0072 é do corte anterior nesta branch. As sete ADRs existentes receberam somente adendos; seus bytes anteriores foram preservados. Docs vigentes descrevem provedores diretos, chat CLI e catálogo independente; planos/medições antigos estão identificados como históricos. A busca literal restante encontra somente história/ADRs e linhas retiradas do tracker. Os dois SQL arquivados são registros históricos, sem execução pelo app.

OpenSpec estrito passou (`group6-openspec-final.log`); tracker QA 4/4 passou (`group6-qa-tracker-retirement.log`), links relativos resolvem e `CLAUDE.md` permanece symlink. O bloco Trail alterado pelo usuário foi preservado byte a byte na árvore e não entra no commit: somente as regras/documentação desta change são staged. `trail check` mantém a mesma refutação do grupo 5: a regra de MCP para análise de vídeo não proíbe MCP no chat e a prova nativa pendente não bloqueia implementação autorizada. Nenhuma prova de provider pago ou aceite nativo final é declarada nestes docs.

## 7. Verificação integrada

- [x] 7.1 Gates: `pnpm check`, `pnpm typecheck`, `NODE_OPTIONS=--no-experimental-webstorage pnpm test`, `cargo fmt --manifest-path src-tauri/Cargo.toml --all --check`, `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings`, `pnpm test:rust` e `make verify`. Verificar: saída sem falhas novas.
  Verificado em 2026-10-28: `make -k verify` executou todos os alvos, com frontend 196 arquivos/1834 testes, Rust desktop+companion 2146 passados/0 falhas/21 ignorados, typecheck, fmt, clippy e ambas as guardas aprovados. Exit 2 somente por `pnpm check` nos dois erros de formatação preexistentes `.mcp.json` e `opencode.json`, explicitamente fora do escopo pelo orquestrador. `make verify` sem `-k` confirma a mesma parada em `check`. Logs: `.tmp/verify/remover-clovy-api/final-make-verify-all.log`, `final-gates-summary.txt`, `group5-make-verify.log`. Não foi contornada a configuração dos checks, e esses arquivos não foram alterados.

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

### Prova nativa parcial e revisão, 2026-10-28

Prova pós-corte em `.tmp/verify/remover-clovy-api/run-20261028-final/report.md`, app debug com identidade compilada `dev.g.clovy.apiproof1028` e perfil sintético novo. `lsof` confirmou somente bancos/socket do perfil próprio. Catálogo público, modelos sem tools, troca entre dois modelos Moondream preservando chat Claude e descrição do motor Notas foram observados. Ditado sem Groq mostrou a frase exigida. Claude/MCP completou clarificação e escrita aprovada; escrita negada não aparece no banco; cancelamento persistiu `cancelled`; `generate_image` sem fal retornou `fal_api_key_required`, sem asset/retry. Nenhum turno Codex ou chamada paga foi feito.

O primeiro listener registrou dez GETs não atribuídos a `/props` e `/models` às 10:54; o log foi preservado e não representa zero global. Segundo listener em 8080, com atribuição de conexões habilitada, registrou **zero** durante repetição do ditado sem chave, catálogo/seleção, negação/cancelamento e erro fal. Ambos foram encerrados pelo run; app próprio também encerrado, perfil preservado e autostart do dono intacto. Áudio com chave/permissões, gravação e inferência OpenRouter/fal permanecem não provados. 7.2 continua aberta; prova pós-corte não reclassifica o MCP pré-corte que falhou em 3.2.

Rodada1 de review-pair: `.tmp/verify/remover-clovy-api/review-pair/1/summary.md`, **exit 10**, sem permissão para commit final. OpenQodex incompleto (limite de tempo) e cargo-deny sem crate de outra plataforma no cache; CodeRabbit não recebeu o snapshot devido ao gate. O suposto segredo é uma literal sintética no teste de sanitização já existente na base e apagado nesta branch; SHA-256 e uso local foram conferidos sem expor o valor, e a allowlist exata suportada pelo reviewer está em `review-secret-allowlist.json`. Não há exceção ampla nem desligamento do scanner.

Tratamento dos quatro achados: dois majors duplicam a dependência real do sidecar em `scripts/build-macos-release.sh`, sucessor do arquivo nomeado na tarefa4.1 (rename `95a60d3c`); removidos build SEA, packaging e smokes obsoletos, regressão RED/GREEN 11/11 e sintaxe bash válidas. Os minors em `.cursor/hooks.json:7` e `.trail/hook.cjs:143` pertencem às alterações do usuário de Graft para Trail, anteriores/separadas desta tarefa e fora do escopo autorizado; preservados e encaminhados ao dono, sem alegar que foram corrigidos. A segunda rodada deve usar saída nova e prazo maior; exit4/10 continua bloqueando o commit final.

Rodada2: CodeRabbit recusou o diff completo com `too_many_files` (433 arquivos, limite150), sem achados e sem revisão concluída; mensagem em `.tmp/verify/remover-clovy-api/review-pair/2/coderabbit/stdout.ndjson`. Não foram usados créditos, plano pago nem escopo reduzido. O orquestrador precisa fornecer capacidade ou revisão em lotes com cobertura integral antes de um gate válido. OpenQodex continua nesta rodada; não há autorização para commit final com exit4/10.

Rodada2 encerrada pelo run após confirmar a recusa não recuperável de tamanho do CodeRabbit: `.tmp/verify/remover-clovy-api/review-pair/2/summary.md`, **exit130**, OpenQodex interrompido e CodeRabbit `skip:too_many_files`. Gate de segredos limpo com a exceção exata verificada. Nenhum resultado dessa rodada é tratado como revisão concluída; não foi feito commit final. Opções concretas de desbloqueio/contrato de cobertura em `review-unblock.md`. Será necessária nova rodada em diretório vazio após decisão do orquestrador.

### Decisão de fechamento do orquestrador, 2026-10-28

"per-commit review because the full diff exceeds CodeRabbit's 150-file limit (2026-10-28)". O histórico não publicado será reescrito em commits de até140 arquivos alterados, com exclusões puras das árvores agent-runtime e clovy-api separadas, preservando a árvore final. Cada commit será revisado em worktree temporária detached fora deste checkout, com base no próprio pai, mesma allowlist exata e saída `review-pair/c-<shortsha>-<n>/`; a worktree será removida depois. Não se altera o harness nem se usam créditos. Exit4/10 bloqueia; achados de exit6 serão corrigidos em commit adicional revisado ou refutados com evidência aqui.

O smoke3.2 foi aceito explicitamente com a prova Claude pós-corte e o relatório pré-corte existente; o retry foi encerrado sem lançamento e seus outputs próprios removidos. Fluxos positivos Groq/OpenRouter/fal permanecem declarados não provados, para o dono tratar separadamente, conforme decisão do orquestrador. O segundo listener de8080 com zero requisições permanece a prova aceita. A atribuição barata dos GETs iniciais está em `localhost-probe-binary-scan.json`; sem PID histórico não se afirma autoria.

Atribuição barata concluída: no executável pós-corte a única ocorrência de `/props` é o nome de fonte `icu_properties-2.2.0/src/props.rs`, não uma rota; relay/shim/helpers não contêm a rota. `/models` tem usos legítimos de catálogo direto. Outra aplicação local é plausível, porém autoria não é provada sem o PID histórico. A janela zero do segundo listener é aceita pelo orquestrador. Scan/contexto nos arquivos de evidência acima.

### Cobertura por commit e tratamento dos achados

A reescrita gerou dez commits com no máximo 140 arquivos alterados (contagem conservadora sem renames), mais o grupo final `a3b5d82e` com quatro arquivos. `git diff 6a1ac530 e0a99fb8 --exit-code` retornou 0, vazio; cada árvore intermediária equivalente também foi conferida. Evidência: `.tmp/verify/remover-clovy-api/rewritten-history.json` e `rewrite-tree-diff.txt`. Os dois commits de exclusão são deleções puras; os SQLs do backend foram arquivados antes da exclusão.

| Commit | Rodada concluída | Resultado e tratamento |
| --- | --- | --- |
| `4fb10a51` | `.tmp/verify/remover-clovy-api/review-pair/c-4fb10a51-3/summary.md` | Exit 6, ambos concluídos; cinco achados refutados como defeitos da árvore final, conforme abaixo. |

Refutações de `4fb10a51`, rodada 3: `agent-runtime/package.json:1` (Deleted runtime build target), `src/main.ts:1` (Missing development runtime entry point), `src/sdk-engine.ts:1` (Retained identity test reads deleted engine e Native turns retain deleted runtime dependency), `src/service.ts:1` (Required agent runtime removal), todos sob `agent-runtime/`, descrevem corretamente o commit intermediário de deleção pura explicitamente exigido pelo orquestrador. O commit seguinte `db0bab17` remove scripts/hooks/empacotamento, host nativo e a leitura do arquivo apagado no teste de identidade. `a3b5d82e` remove também os blocos obsoletos do script macOS renomeado. Não são dependências restantes na árvore final: build release exit 0, bundle sem sidecar, frontend 1834/1834 e prova Claude pós-corte. Evidência específica: `.tmp/verify/remover-clovy-api/review-split-followup-evidence.diff`, `release-bundle-build.log`, `release-bundle-files.txt`, `final-make-verify-all.log` e `run-20261028-final/report.md`. A divisão para limite de revisão não declara os commits intermediários de deleção compiláveis isoladamente.

Rodadas anteriores permanecem no histórico: `c-4fb10a51-1` exit 4 (OpenQodex com HTTP 429 e CodeRabbit com cota); `c-4fb10a51-2` exit 4 (OpenQodex completo, CodeRabbit com cota); `c-1714bb2c-1` exit 4 (CodeRabbit com cota). `c-db0bab17-1` foi interrompida ao serializar a fila e não produziu resumo do par; seus artefatos parciais foram preservados e uma rodada nova é obrigatória. Nenhuma dessas rodadas é aceita como gate. 7.3 permanece aberta até cobrir todos os commits, inclusive correções e fechamento.

Correções derivadas da revisão parcial de `db0bab17`, verificadas em 2026-10-28: histórico CLI inclui manifests de anexos disponíveis, inclusive mensagens só com anexos; retry começa conversa nova e exclui tentativas substituídas, com ancestralidade persistida, limite por sessão e proteção contra ciclos, inclusive após falha antes do conversationId ou troca de motor. `get_dictation` de rotina exige contexto habilitado no catálogo e revalida a permissão na chamada, inclusive após revogação. Motor enviado por sessões antigas coincide com o exibido, hidratação tardia respeita escolhas recentes e mudança de nota, e renomear foca/seleciona o título.

Evidência RED/GREEN dessas correções: `.tmp/verify/remover-clovy-api/review-fix-cli-red-{0,1,2}.log`, `review-fix-cli-red-persisted-2.log`, `review-fix-routine-red.log`, `review-frontend-red.log`; regressões frontend 96/96 e biblioteca Rust 1959/1959, com 14 ignorados, em `review-frontend-green.log` e `review-fix-lib-final.log`. Uma primeira compilação do teste adicional falhou por nome de fixture e uma primeira execução ampla encontrou uma expectativa antiga de resume no CLI falso; ambos foram corrigidos antes do GREEN, sem confundir essas falhas com sucesso. Os CLIs dos testes são falsos, sem turnos pagos.

Revalidação integrada após as correções: `make -k verify` exit 2 somente por `.mcp.json` e `opencode.json`, os mesmos erros preexistentes de formatação; frontend 196 arquivos/1843 testes e Rust desktop+companion 2151 passados/0 falhas/21 ignorados. Typecheck, fmt, clippy all-targets e guardas passaram. Logs: `.tmp/verify/remover-clovy-api/review-fix-make-verify.log` e `review-fix-gates-summary.json`. OpenSpec estrito de ambas as changes passou (`review-fix-openspec-{cli,remover}.log`). Impeccable não encontrou problemas nos três componentes alterados. A prova nativa aceita permanece a de `run-20261028-final`; estas correções adicionais foram provadas por testes, não por novos turnos nativos.
