# Tasks

## 1. Base no Windows: build, banco cifrado e runtime

- [x] 1.1 Preparar o `cert` (perfil do usuário `cert-`): rustup `stable-x86_64-pc-windows-msvc`, Strawberry Perl, NASM, pnpm via corepack, clone do repositório; registrar versões em `.tmp/verify/atividade-windows/cert-setup.md`.
- [x] 1.2 RED: teste de integração do store que abre o banco cifrado e confirma que SQLite comum não lê o arquivo, rodando no Windows; verificar que falha no `cert` com `CipherUnavailable`.
- [x] 1.3 GREEN: SQLCipher vendored no alvo Windows (`Cargo.toml`), checagem de Perl/NASM em `scripts/build-windows.ps1`, instalação no job `windows-rust` de `.github/workflows/desktop.yml` com `cargo test` do módulo `activity`; teste de 1.2 passa no `cert`. Sintaxe YAML/PowerShell validada; `workflow_dispatch` pendente da integração pelo dono, conforme exceção deste mandato.
- [x] 1.4 RED: teste de setup que espera runtime de atividade no Windows com plataforma falsa e chave em cofre simulado; verificar que falha.
- [ ] 1.5 GREEN: esqueleto `activity/windows/` com `WinActivityPlatform` (permissões `Granted`, demais métodos devolvendo vazio), `setup`/`start` no Windows com `KeychainKeyStore`; teste de 1.4 passa e o app de desenvolvimento no `cert` mostra captura ligada sem quadros.

## 2. Janela, identidade e listas

- [x] 2.1 RED: testes unitários de identidade (nome do executável em minúsculas, `FileDescription`, AppUserModelID) e das listas Windows em `filter.rs`, `timeline/context.rs`, `timeline/categorize.rs` e `timeline/builder.rs` (`msedge.exe` é navegador, `code.exe` é editor, `lockapp.exe` nunca é útil); verificar que falham.
- [ ] 2.2 GREEN: `focused_window` (D2) e listas; testes de 2.1 passam; no `cert`, quadros mostram app e título corretos no export de depuração.

## 3. Texto, URL e OCR

- [x] 3.1 RED: testes do extrator UIA com árvore simulada (orçamento de nós e tempo, URL do controle de endereço, janela privada); verificar que falham.
- [ ] 3.2 GREEN: `accessibility_text` e URL por UI Automation (D3); testes de 3.1 passam; no `cert`, quadro do Edge com URL e texto e origem "acessibilidade".
- [x] 3.3 RED: teste de integração do OCR com imagem de teste reconhecida pelo `Windows.Media.Ocr`, rodando no `cert`; verificar que falha.
- [ ] 3.4 GREEN: `ocr_text` com Windows Graphics Capture e `Windows.Media.Ocr` (D4), sem arquivo em disco; teste de 3.3 passa; no `cert`, janela sem árvore UIA gera quadro com origem "OCR" e nenhum arquivo de imagem novo.

## 4. Entrada, área de transferência, telas, disco e ociosidade

- [x] 4.1 RED: testes de normalização do Raw Input (só instantes de mouse down e key down, sem código de tecla) e do filtro de formatos ocultos da área de transferência; verificar que falham. `system_idle_ms` excluído desta rodada pelo limite registrado em 4.2.
- [ ] 4.2 GREEN: `set_input_capture`/`drain_input_events` (D5), `clipboard_text_if_changed`, `secondary_windows`, `free_disk_bytes` (D6) e `system_idle_ms`; testes de 4.1 passam; no `cert`, cliques, teclas e cópia aparecem em `input_events` sem conteúdo digitado.
  - Limite desta rodada: falta system_idle_ms, depende da outra change. Não adicionar `frames.input_idle_ms` nem migração 6.
- [ ] 4.3 Atualizar `docs/activity-capture.md` e `docs/activity-timeline.md` com o adaptador Windows, o banco cifrado no Windows e os nomes de tela bloqueada; conferir com o código.

## 5. Agentes de código e embeddings

- [x] 5.1 RED: testes de `SourceRoots` no Windows (perfil e `%APPDATA%`) e da recusa de ponto de reparo ao abrir arquivos; verificar que falham.
- [ ] 5.2 GREEN: raízes e abertura segura no Windows (D10); testes de 5.1 passam; no `cert`, uma sessão real de Claude Code ou Codex aparece na faixa de agentes. Atualizar `docs/coding-agent-sessions.md`.
- [ ] 5.3 Candle e tokenizers no alvo Windows (D9); teste do embedder BGE passa no `cert`; tempo de embedding de uma hora medido e registrado. Se inaceitável, parar e voltar ao dono. Atualizar `docs/day-intelligence.md`.
  - RED do teste existente com o modelo real observado no `cert`: preparo chegou a `Ready`, mas `embed` retornou `None` e a asserção `vectors` falhou; 1 falha, 13,39 s de teste. Log `5.3-bge-real-model-red.log`.

## 6. Interface

- [x] 6.1 RED: testes de componente para a aba Atividade visível com `supported`, seção sem linhas de permissão fora do macOS, vista "Hoje" com estado "não suportado" e ícone de app no Windows; verificar que falham.
- [ ] 6.2 GREEN: `settings-config.ts`, `AppSettings.tsx`, `ActivitySettingsSection.tsx`, `ActivityTimelineView.tsx`, mensagens e `app_icon.rs` no Windows (D11), seguindo `/impeccable`; testes de 6.1 passam. Atualizar a lista do que o Windows cobre no `README.md`.

## 7. Verificação

- [ ] 7.1 `make verify` verde no Mac (sem mudança de comportamento no macOS) e `cargo test` do módulo `activity`, `coding_agents` e `day_intelligence` verde no `cert`; contagens em `.tmp/verify/atividade-windows/`.
- [ ] 7.2 Prova ponta a ponta no `cert` com o app de desenvolvimento na sessão de console: captura ligada por 2 horas de uso real, "Hoje" com Focado/Ocioso/Ausente, categorias e principais apps com ícones, frentes geradas para as horas fechadas, faixa de agentes, janela InPrivate descartada, Windows bloqueado fora do foco, banco ilegível sem chave; capturas de tela e export em `.tmp/verify/atividade-windows/`.
- [ ] 7.3 Limpar o `cert`: parar o app, remover a tarefa agendada e o clone; manter as ferramentas só se o dono quiser.

## Workflow follow-up

- Arquivar `add-activity-intelligence` e `atividade-ocioso-e-minutos` antes desta change.
- Revisão de código, aceite do dono, arquivamento, `/sync` e `/cleanup`.
