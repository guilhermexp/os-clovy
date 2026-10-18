# Tasks

Gates de `AGENTS.md` ("Verification") em cada grupo para o que ele tocou. Frontend segue `~/.agents/skills/impeccable/SKILL.md`.

## 1. Interface do chat

- [x] 1.1 Remover o rodapé de privacidade do início e o aviso de motor do composer e do seletor, com chaves i18n e testes que só fixavam o texto (design D1). Verificar: testes de componente sem `.agent-hero-footnote` e sem `.agent-composer-notice` nos três estados de ferramentas.
- [ ] 1.2 SVGs de marca vendorizados com `LICENSES.md` e componente de logo cobrindo `claude`, `codex`, `copilot`, `cursor-agent`, `agy`, `pi`, `openrouter` e monograma de reserva (D2). Verificar: teste unitário do mapeamento id → SVG/monograma.
- [ ] 1.3 Aplicar o logo em todos os pontos listados no design (seletor de motor, provedores, MCP, fontes e faixa de agentes de código, importação do Claude Code, cartões que citam runtime, seletores de modelo). Verificar: teste de render do seletor de motor com ícone por item; captura de tela do app de dev de cada tela tocada.

## 2. GitHub pelo gh

- [ ] 2.1 Fonte de token `gh` e conta `github_cli`: detecção, conexão automática só sem conta existente, leitura ativa e escrita desligada, token lido a cada chamada e nunca persistido (D3). Verificar: testes de integração com `gh` falso no PATH e servidor HTTP falso no lugar de `api.github.com` (conecta; não conecta com conta da App; não conecta sem `gh`; nenhuma escrita no armazenamento de tokens).
- [ ] 2.2 Desconectar grava `githubCliAutoConnect = false`; botão "Conectar pelo GitHub CLI"; `gh auth logout` vira `reconnect_required`; pacotes e aprovação de escrita como na conta da App. Verificar: testes de integração dos cenários da spec `github-cli-connection`.
- [ ] 2.3 UI de Plugins mostra "via gh" e o botão (impeccable), i18n pt-BR/en; ADR nova (adendo à ADR-0036) e doc do conector. Verificar: teste de componente da linha do GitHub nos estados conectado via gh e desconectado com botão.

## 3. Uso do computador local

- [ ] 3.1 Remover os gates de plano, visão e rollout do status e da ativação; remover a ferramenta do catálogo nativo e o código que ficar sem chamador (D4). Verificar: teste de integração de `status_inner` pronto sem plano, sem visão e com rollout inacessível; catálogo nativo sem `computer_use`; testes de `computer_use_tests` verdes.
- [ ] 3.2 UI e textos: estados removidos do `ComputerUseControl`, privacy tip diz que a captura vai ao CLI do turno; ADR nova (adendo à ADR-0028). Verificar: teste de componente sem os estados `pro`/`needs_vision`/`rollout_disabled`.

## 4. Prova

- [ ] 4.1 `make verify` sem falhas.
- [ ] 4.2 App de dev (perfil `-dev`): capturas da tela inicial e do composer sem rodapé/aviso, seletor de motor e Configurações com ícones; Plugins com GitHub "via gh" conectado sozinho (com o `gh` do dono logado) e uma leitura real (`list_repositories`) por um turno Claude Code; uso do computador "Pronto" sem conta Pro. Evidência em `.tmp/verify/runtimes-cli-na-ui/run-<id>/report.md`.

## Workflow follow-up

- Revisão por harness diferente do autor, aceite do dono, `/openspec-archive-change`, `/sync` e `/cleanup`.
