# Tasks

## 1. Reprodução no Windows de prova (D1)

- [x] 1.1 Instalar a `v0.1.0` publicada no `cert` (perfil `cert-`), com `codex` instalado pelo npm além do nativo; registrar com capturas: janelas de console na abertura, erro real do Codex no chat (stderr ou mensagem da execução), erro real ao salvar um atalho personalizado (mensagem e `dictation-settings.json`), janela que não volta pela bandeja e pela segunda abertura. Registro em `.tmp/verify/corrigir-windows-v0-1-1/reproducao.md`.

## 2. Processos sem console e sondagem compartilhada (D2, D3)

- [x] 2.1 RED: teste que falha com `Command::new` fora de `hide_console`, e teste de sondagem concorrente contando uma sondagem por CLI; commit RED.
- [x] 2.2 GREEN: `hide_console` em todos os pontos de criação de processo (incluindo sidecar) e `detect_all` compartilhado; testes de 2.1 passam.

## 3. Árvore de processos no Windows (D4)

- [x] 3.1 RED: teste Windows com CLI falso `.cmd` que inicia filho longo; cancelar e tempo limite deixam o filho vivo; commit RED.
- [x] 3.2 GREEN: Job Object com encerramento ao fechar, usado pelo chat e pela geração pontual; teste de 3.1 passa no job Windows do CI.

## 4. Codex no chat no Windows (D1)

- [ ] 4.1 RED com a causa encontrada em 1.1 (teste que reproduz a argv/ambiente/saída que falha); commit RED.
- [ ] 4.2 GREEN: correção da causa; teste passa; mensagem clara se o problema for configuração do Codex no Windows.

> Em aberto (2026-10-23): no `cert`, a v0.1.0 respondeu no chat com o Codex nativo e com o `codex.cmd` do npm (`.tmp/verify/corrigir-windows-v0-1-1/reproducao.md`, 2b). Não há causa reproduzida para um RED, e D1 proíbe corrigir por hipótese. O que se reproduziu no chat (consoles e processos órfãos ao cancelar) foi corrigido nos grupos 2 e 3. Falta o erro real do Windows do dono (mensagem na tela e o `[windows] sandbox` do `~/.codex/config.toml` dele).

## 5. Codex e Antigravity em notas, texto e atividade (D5)

- [x] 5.1 RED: testes da invocação pontual do `codex` e do `agy` (flags, pasta temporária, sem sessão) e da seleção por uso aceita; `cursor-agent` continua recusado; commit RED.
- [x] 5.2 GREEN: invocações, regras de uso e UI de provedores sem acinzentar os dois; mensagem "neste Mac" corrigida para o Windows; docs de provedores atualizadas.

## 6. Atalhos no Windows (D6)

- [x] 6.1 RED: testes da validação Rust (tecla sozinha e modificadora sozinha aceitas no Windows, Fn recusado), da máquina de estados do gancho (Ctrl direito sozinho dispara; Ctrl direito+C não), da captura de fonte única, da mensagem de tecla não vista e do erro que não some; commit RED.
- [x] 6.2 GREEN: `hotkeys.rs`, `dictation.rs`, `use-shortcut-capture.ts`, `AppSettings.tsx`, padrão Ctrl direito no Windows para quem não tem atalho salvo, e a causa de "não salva nenhum atalho" encontrada em 1.1; testes passam.
- [x] 6.3 Interface da linha de atalho revisada com `/impeccable` (pt-BR e inglês); adendo datado na ADR-0020.

## 7. Janela principal (D7)

- [x] 7.1 RED: testes do caminho único de mostrar janela (recria quando não existe) e do fechar que esconde em todos os sistemas; commit RED.
- [x] 7.2 GREEN: `show_main_window` único usado por bandeja (clique esquerdo e "Abrir Clovy"), instância única, notificações, HUDs e menus; `CloseRequested` esconde em todos os sistemas; testes passam.

## 8. Prova

- [x] 8.1 `make verify` com exit 0 numa worktree limpa; saída em `.tmp/verify/corrigir-windows-v0-1-1/`.
- [x] 8.2 Ensaio do `release.yml` com `dry_run=true` no HEAD final; os quatro jobs verdes.
- [ ] 8.3 Windows de prova com o instalador do ensaio: nenhuma janela de console na abertura nem ao usar chat e notas; Codex responde no chat e gera resumo de nota; Antigravity aceito em atividade (se instalado no `cert`; senão registrar); atalho Ctrl+Shift+D e Ctrl direito salvos e iniciando o ditado; Fn mostra a mensagem; fechar e reabrir pela bandeja e pelo menu Iniciar; cancelar chat não deixa `node`/`codex.exe`. Capturas e registro em `.tmp/verify/corrigir-windows-v0-1-1/windows.md`; desinstalar e limpar no fim.

> 8.3 feita em parte (2026-10-23, `.tmp/verify/corrigir-windows-v0-1-1/windows.md`): provados sem console na abertura, no chat e na chamada pontual; Codex do npm respondendo no chat; cancelar sem `node`/`codex.exe` órfãos; Codex liberado e testado para notas e atividade; Ctrl direito e Ctrl+Shift+D salvos e registrados; mensagem de tecla que não chega ao Windows; fechar esconde e a segunda abertura traz a janela. Sem prova, por a sessão do `cert` estar bloqueada ou faltar dependência: ditado iniciado pelo teclado físico, clique no ícone da bandeja e "Abrir Clovy", resumo de nota com Codex (precisa de áudio e da Clovy API local), Antigravity (não instalado no `cert`).
- [ ] 8.4 macOS: smoke do caminho de janela (fechar e reabrir pela barra de menus e pelo Dock) e de uma geração de nota com `codex`, em app de prova com identificador próprio, sem Keychain.

> 8.4 não feita: um app de prova com identificador próprio cria na abertura a chave do `activity.sqlite3` no Keychain (`activity/key.rs`), e esta execução não pode criar itens no Keychain. Cobertura no macOS: testes de `main_window` e `menu_bar`, e `make verify`.

## Workflow follow-up

- Revisão de código, merge, `pnpm version:bump 0.1.1`, tag `v0.1.1`, release; conferir a atualização automática da `0.1.0` no Windows do dono.
- Arquivar, `/sync`, `/cleanup`.

## 9. Revisão 1 (W1–W5)

- [x] 9.1 W5 RED: `last_message.txt` maior que 8 MiB é lido inteiro; commit RED.
- [x] 9.2 W5 GREEN: o arquivo é lido com o limite de `MAX_CAPTURE_BYTES` e a saída maior é recusada.
- [x] 9.3 W2 RED: o `codex exec` pontual sobe os MCPs e os plugins do usuário; o `agy --print` também; commit RED.
- [x] 9.4 W2 GREEN: o Codex pontual desliga plugins, apps, hooks, `notify` e cada servidor MCP resolvido pelo próprio `codex mcp list --json`, mantendo a config (sandbox do Windows) e a autenticação; o `agy` pontual é recusado com mensagem clara enquanto houver MCP ou plugin ativo (o `agy` 1.3 não tem como desligá-los por chamada); prova com o codex real no Mac (`revisao-1/w2-green-real.txt`: nenhum MCP, plugin ou `notify` sob a chamada).
- [x] 9.5 W3 RED: o prompt inteiro do `agy` no argv passa do limite de linha de comando do Windows sem aviso; commit RED.
- [x] 9.6 W3 GREEN: tamanho da linha de comando do Windows contado em UTF-16 com o escape; acima do limite, erro claro antes de criar o processo, na geração pontual e no chat. O `agy` 1.3 não lê o prompt pela entrada padrão no modo texto ("empty prompt", testado no Mac), por isso ele segue no argv.
- [ ] 9.7 W4 RED: Ctrl+F13, Shift+Insert e Ctrl+Pause são recusados na validação e na normalização do Windows; commit RED.
- [ ] 9.8 W4 GREEN: essas combinações valem no Windows, sem mudar o macOS.
- [ ] 9.9 W1 RED: um filho criado entre o `spawn()` e a adoção escapa do Job Object; commit RED.
- [ ] 9.10 W1 GREEN: o processo nasce suspenso, entra no job e só então é retomado; falha de atribuição vira erro visível (chat, geração pontual, ferramenta de shell).
- [ ] 9.11 Grupo 4 RED: falha de sandbox do Codex no Windows aparece como erro genérico no chat; commit RED.
- [ ] 9.12 Grupo 4 GREEN: o chat mostra o que configurar.
- [ ] 9.13 `make verify` com exit 0 numa worktree limpa; CI Windows sem falhas novas; ensaio do `release.yml` com `dry_run=true`, os quatro jobs verdes.
- [ ] 9.14 `cert` com o instalador do ensaio: chat com Codex e cancelamento sem órfãos; Codex pontual sem MCP subir; limpeza no fim.
