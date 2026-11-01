# Tasks

## 1. Preferência no Rust

- [ ] 1.1 Criar `app_preferences.rs` com `hide_from_dock` (padrão `false`), estado gerenciado, arquivo `app-preferences.json` atômico e comandos `app_preferences_get`/`app_preferences_set` registrados em `lib.rs`; verificar com testes de ida e volta, arquivo ausente e arquivo malformado.

## 2. Política de ativação

- [ ] 2.1 Adicionar `desired_policy(hide_from_dock, window_visible)` e o aplicador macOS em `main_window.rs`; verificar com teste unitário das quatro combinações.
- [ ] 2.2 Aplicar a política após esconder a janela (fechar e Cmd+Shift+W, por uma função de esconder compartilhada) e antes de mostrar em `AppSlot::reveal`; verificar com `cargo test main_window` (incluindo o teste de ponto único de exibição) e atualizar o comentário do topo de `main_window.rs`.

## 3. Configurações

- [x] 3.1 Adicionar tipo e wrappers em `src/lib/tauri.ts`, a linha "Ocultar do Dock" em Configurações → Geral (só macOS, independente do autostart) e as strings en/pt-BR em `settings.ts` seguindo `spec/sentence-case.md` e `spec/no-typographic-dashes.md`; verificar com teste de componente da linha (ligar chama o comando, erro aparece) e `src/test/tauri-command-registry.test.ts`.

## 4. Integração

- [ ] 4.1 Rodar `pnpm check`, `pnpm typecheck`, `pnpm test`, `cargo fmt --check`, `cargo clippy -D warnings` e `pnpm test:rust` (via `heavy-run`); todos verdes.
- [ ] 4.2 No app de desenvolvimento, com Computer Use: ligar a preferência, fechar a janela (ícone some do Dock e do Cmd-Tab, barra de menus continua), reabrir pela barra de menus (janela com foco, ícone de volta, Cmd+W funciona), desligar e fechar (ícone fica); registrar capturas no relatório da entrega.

## Workflow follow-up

- Gate `review-pair.sh --gate` antes do commit final.
- Arquivar a change, `/sync` e `/cleanup`.
