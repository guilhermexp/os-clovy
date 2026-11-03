# Tasks

## 1. Preferência no Rust

- [x] 1.1 Criar `app_preferences.rs` com `hide_from_dock` (padrão `false`), estado gerenciado, arquivo `app-preferences.json` atômico e comandos `app_preferences_get`/`app_preferences_set` registrados em `lib.rs`; verificar com testes de ida e volta, arquivo ausente e arquivo malformado.

## 2. Política de ativação

- [x] 2.1 Adicionar `desired_policy(hide_from_dock, window_visible)` e o aplicador macOS em `main_window.rs`; verificar com teste unitário das quatro combinações.
- [x] 2.2 Aplicar a política após esconder a janela (fechar e Cmd+Shift+W, por uma função de esconder compartilhada) e antes de mostrar em `AppSlot::reveal`; verificar com `cargo test main_window` (incluindo o teste de ponto único de exibição) e atualizar o comentário do topo de `main_window.rs`.

## 3. Configurações

- [x] 3.1 Adicionar tipo e wrappers em `src/lib/tauri.ts`, a linha "Ocultar do Dock" em Configurações → Geral (só macOS, independente do autostart) e as strings en/pt-BR em `settings.ts` seguindo `spec/sentence-case.md` e `spec/no-typographic-dashes.md`; verificar com teste de componente da linha (ligar chama o comando, erro aparece) e `src/test/tauri-command-registry.test.ts`.

## 4. Integração

- [x] 4.1 Rodar `pnpm check`, `pnpm typecheck`, `pnpm test`, `cargo fmt --check`, `cargo clippy -D warnings` e `pnpm test:rust`; todos verdes. No Mac, comandos nativos sem a fila global removida; na box, preservar os locks Macro→Clovy.
- [ ] 4.2 No app de desenvolvimento, com Computer Use: ligar a preferência, fechar a janela (ícone some do Dock e do Cmd-Tab, barra de menus continua), reabrir pela barra de menus (janela com foco, ícone de volta, Cmd+W funciona), desligar e fechar (ícone fica); registrar capturas no relatório da entrega.

## Workflow follow-up

- Gate `review-pair.sh --gate` antes do commit final.
- Arquivar a change, `/sync` e `/cleanup`.

## Prova executada em 2026-11-03

Rust nativo: preferência ausente/malformada/round-trip e política das quatro combinações passaram na biblioteca de 2.302 testes da rodada 8. Frontend: componente (dois testes) e registry (um teste) dentro dos 2.069 aprovados. O CI automático 41582917424 da fonte `9bc475d0` aprovou os comandos subjacentes `cargo fmt`, Clippy e `cargo test` completo no macOS, com 2.474 testes únicos/16 ignorados preexistentes; não soma cobertura distinta à composição anterior. Recibos em `.tmp/verify/T7/mac-native-20261102/`, `.tmp/verify/T7/parent-checks-20261102/3/` e `.tmp/verify/T7/mac-signature-9bc475d0-20261103/ci-rust-job.log`.

No app próprio assinado da fonte `05d85a78`, PID1543 e DB próprios comprovados: política macOS Regular (0) antes; ligar e fechar levou a Accessory (1); reabrir pelo evento do aplicativo voltou a Regular (0); desligar e fechar manteve Regular (0), e a preferência própria persistiu false. Recibos `14/dock-{pid-before,closed-on,reopened-on,closed-off}.json`. Isso não conclui4.2: menu da barra, foco/Cmd+W e ciclo completo precisam de prova explícita. A fonte2dbe passou no CI41589762530; a revisão do checkpoint foi manual11, não aprovação automática. A sessão gráfica bloqueou na continuação737, impedindo os fluxos visuais restantes. Arquivo integral não é elegível enquanto4.2 estiver aberto.
