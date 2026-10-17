# Tasks

Gates de `AGENTS.md` ("Verification") em cada grupo para o que ele tocou. Prova nativa só no grupo 4, com identificador próprio conforme "Functional proof".

## 1. Início no motor CLI

- [ ] 1.1 `ensureHomeSession` cria a sessão do Início com o motor padrão (`chatDefaultEngine().modelId`) e resolve o caso da sessão antiga `open-software/auto` (design D2: confirmar se `start_agent_run` usa o modelo do pedido ou da sessão e atualizar a sessão se preciso). Verificar com teste de componente que falha antes: sessão antiga + envio → `startRun` com `__clovy_cli_engine__:claude`.
- [ ] 1.2 Envio no Início chama `startRun` na sessão do Início (`safetyMode: "sandboxed"`, anexos) e renderiza a conversa a partir dos itens da sessão com a projeção das sessões CLI (streaming, aprovação, erro). Verificar: testes de componente para resposta em streaming, cartão de aprovação no Início, nenhuma sessão nova criada, e sessão do Início fora da barra lateral.
- [ ] 1.3 Erros do turno no Início usam o mapeamento das sessões CLI e preservam a mensagem no compositor (nenhum CLI instalado, turno falho). Verificar com testes de componente.
- [ ] 1.4 Teste de integração Rust: dois turnos na sessão do Início com CLI falso; o segundo recebe `--resume` ou o histórico prefixado; servidor falso da Clovy API recebe zero chamadas. Verificar com `cargo test` do teste novo.

## 2. Histórico antigo

- [ ] 2.1 Comando Rust de importação única dos turnos de `clovy:home:direct-turns:v1` como itens da sessão do Início, em ordem e com hora original, idempotente por marcador no banco, ignorando entradas ilegíveis (design D3). Verificar com teste de integração: importa, reimporta sem duplicar, entrada ilegível não falha.
- [ ] 2.2 Frontend chama a importação na abertura do Início e continua renderizando os cartões de `clovy:home:task-handoffs:v1` intercalados por hora, abrindo a sessão de destino; `localStorage` não é apagado. Verificar com teste de componente usando fixture de `localStorage`.

## 3. Remoção e documentação

- [ ] 3.1 Remover `clovy_home_chat`, `june_home_chat` e todo o código só da triagem em `clovy_api.rs` e `lib.rs` (design D5), mantendo `proxy_agent_chat_completions`; registrar se `persona.rs` ficou sem usuário. Verificar: `cargo clippy --all-targets -D warnings` e `pnpm test:rust` sem falhas; busca sem referências.
- [ ] 3.2 Remover do frontend `clovyHomeChat`, `startHomeTask`, `buildClovyHomeConversationContext` e o que só a triagem usava, códigos `home_chat_*` e mensagens i18n sem uso; apagar os testes que fixavam a triagem. Verificar: `src/test/tauri-command-registry.test.ts` verde, `pnpm check`, `pnpm typecheck` e vitest completo sem falhas novas.
- [ ] 3.3 ADR-0064 "Home runs on the default CLI engine", nota `superseded in part by ADR-0064` na ADR-0062, `docs/llm-providers.md` e `CONTEXT.md` sem triagem do Início. Verificar: busca por `home_chat`, `june_home_chat` e "triage" em `docs/`, `CONTEXT.md` e `AGENTS.md` só em ADRs históricas.

## 4. Prova

- [ ] 4.1 `make verify` sem falhas novas.
- [ ] 4.2 Prova nativa com identificador próprio e Clovy API local sem chave da OpenRouter: enviar "eae" e uma segunda mensagem que depende da primeira no Início, ver as respostas do Claude Code, reiniciar o app e ver a conversa; conferir no banco da cópia de perfil os itens da sessão do Início e no log da Clovy API nenhuma chamada a `/v1/chat/completions`. Evidência em `.tmp/verify/inicio-no-claude-code/run-<id>/report.md`.

## Workflow follow-up

- Revisão por harness diferente do autor, aceite do dono, `/openspec-archive-change`, `/sync` e `/cleanup`.
- A linha do Início em `openspec/changes/cli-substitui-agente-nativo/design.md` ("`POST /v1/chat/completions` continua (Início, ...)") fica desatualizada; ajustar por `/openspec-update-change` naquela change.
