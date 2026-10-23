# Tasks

## 1. Reprodução no Windows de prova (D1)

- [x] 1.1 Instalar a `v0.1.0` publicada no `cert` (perfil `cert-`), com `codex` instalado pelo npm além do nativo; registrar com capturas: janelas de console na abertura, erro real do Codex no chat (stderr ou mensagem da execução), erro real ao salvar um atalho personalizado (mensagem e `dictation-settings.json`), janela que não volta pela bandeja e pela segunda abertura. Registro em `.tmp/verify/corrigir-windows-v0-1-1/reproducao.md`.

## 2. Processos sem console e sondagem compartilhada (D2, D3)

- [x] 2.1 RED: teste que falha com `Command::new` fora de `hide_console`, e teste de sondagem concorrente contando uma sondagem por CLI; commit RED.
- [x] 2.2 GREEN: `hide_console` em todos os pontos de criação de processo (incluindo sidecar) e `detect_all` compartilhado; testes de 2.1 passam.

## 3. Árvore de processos no Windows (D4)

- [x] 3.1 RED: teste Windows com CLI falso `.cmd` que inicia filho longo; cancelar e tempo limite deixam o filho vivo; commit RED.
- [ ] 3.2 GREEN: Job Object com encerramento ao fechar, usado pelo chat e pela geração pontual; teste de 3.1 passa no job Windows do CI.

## 4. Codex no chat no Windows (D1)

- [ ] 4.1 RED com a causa encontrada em 1.1 (teste que reproduz a argv/ambiente/saída que falha); commit RED.
- [ ] 4.2 GREEN: correção da causa; teste passa; mensagem clara se o problema for configuração do Codex no Windows.

## 5. Codex e Antigravity em notas, texto e atividade (D5)

- [x] 5.1 RED: testes da invocação pontual do `codex` e do `agy` (flags, pasta temporária, sem sessão) e da seleção por uso aceita; `cursor-agent` continua recusado; commit RED.
- [x] 5.2 GREEN: invocações, regras de uso e UI de provedores sem acinzentar os dois; mensagem "neste Mac" corrigida para o Windows; docs de provedores atualizadas.

## 6. Atalhos no Windows (D6)

- [x] 6.1 RED: testes da validação Rust (tecla sozinha e modificadora sozinha aceitas no Windows, Fn recusado), da máquina de estados do gancho (Ctrl direito sozinho dispara; Ctrl direito+C não), da captura de fonte única, da mensagem de tecla não vista e do erro que não some; commit RED.
- [x] 6.2 GREEN: `hotkeys.rs`, `dictation.rs`, `use-shortcut-capture.ts`, `AppSettings.tsx`, padrão Ctrl direito no Windows para quem não tem atalho salvo, e a causa de "não salva nenhum atalho" encontrada em 1.1; testes passam.
- [x] 6.3 Interface da linha de atalho revisada com `/impeccable` (pt-BR e inglês); adendo datado na ADR-0020.

## 7. Janela principal (D7)

- [ ] 7.1 RED: testes do caminho único de mostrar janela (recria quando não existe) e do fechar que esconde em todos os sistemas; commit RED.
- [ ] 7.2 GREEN: `show_main_window` único usado por bandeja (clique esquerdo e "Abrir Clovy"), instância única, notificações, HUDs e menus; `CloseRequested` esconde em todos os sistemas; testes passam.

## 8. Prova

- [ ] 8.1 `make verify` com exit 0 numa worktree limpa; saída em `.tmp/verify/corrigir-windows-v0-1-1/`.
- [ ] 8.2 Ensaio do `release.yml` com `dry_run=true` no HEAD final; os quatro jobs verdes.
- [ ] 8.3 Windows de prova com o instalador do ensaio: nenhuma janela de console na abertura nem ao usar chat e notas; Codex responde no chat e gera resumo de nota; Antigravity aceito em atividade (se instalado no `cert`; senão registrar); atalho Ctrl+Shift+D e Ctrl direito salvos e iniciando o ditado; Fn mostra a mensagem; fechar e reabrir pela bandeja e pelo menu Iniciar; cancelar chat não deixa `node`/`codex.exe`. Capturas e registro em `.tmp/verify/corrigir-windows-v0-1-1/windows.md`; desinstalar e limpar no fim.
- [ ] 8.4 macOS: smoke do caminho de janela (fechar e reabrir pela barra de menus e pelo Dock) e de uma geração de nota com `codex`, em app de prova com identificador próprio, sem Keychain.

## Workflow follow-up

- Revisão de código, merge, `pnpm version:bump 0.1.1`, tag `v0.1.1`, release; conferir a atualização automática da `0.1.0` no Windows do dono.
- Arquivar, `/sync`, `/cleanup`.
