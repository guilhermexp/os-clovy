# Tasks

## 1. Decisão e docs base

- [ ] 1.1 Escrever `docs/adr/0071-*.md` (helper de vídeo e exceção ao isolamento de CLI), atualizar `docs/llm-providers.md` (contrato de isolamento) e `CONTEXT.md` (termo "vídeo da reunião"). Verificar: textos citam as flags e caminhos da decisão 6 do design.

## 2. Detecção da janela de reunião

- [ ] 2.1 Enumerar janelas na tela com `SCShareableContent` e escolher a janela de reunião (tokens, app dono do microfone, janela da frente); comando `detect_meeting_window` registrado em `lib.rs` e em `src/lib/tauri.ts`. Verificar: testes Rust da escolha com listas de janelas simuladas (Meet no Chrome, Zoom, nada casado, dois candidatos); `tauri-command-registry.test.ts` passa.

## 3. Helper de vídeo

- [ ] 3.1 Helper `Clovy Video Capture.app` (Swift, ScreenCaptureKit + AVAssetWriter, MP4 fragmentado, até 5 quadros/s, lado até 1920 px, sem áudio, pausa/retomada/fim por sinais, `status.json`) compilado e assinado em `build.rs` e copiado por `tauri.macos.conf.json`; `Info.plist` com o texto novo de gravação de tela. Verificar: helper grava uma janela de teste por 30 s com uma pausa, e `ffprobe` lê o arquivo completo e o arquivo truncado.
- [ ] 3.2 Cliente Rust do helper (iniciar, pausar, retomar, parar, estado) no modelo de `system_macos.rs`. Verificar: teste de integração com helper falso (início, permissão negada, encerramento cedo).

## 4. Sessão de gravação com vídeo

- [ ] 4.1 Migração `video_artifacts` e repositório (criar, finalizar, falhar, apagar com a nota e com a gravação, recuperação do parcial). Verificar: testes Rust de migração, remoção e recuperação.
- [ ] 4.2 `start_recording`/`start_meeting_recording` com `video: Option<{windowId}>`; vídeo inicia após o áudio, registra o deslocamento, segue pausa/retomada/fim; erro do vídeo nunca falha o áudio. Verificar: teste de integração com helper falso que falha no início e no meio, com áudio completo nos dois casos.

## 5. Geração da nota com vídeo

- [ ] 5.1 Concessão de ferramentas em `llm::GenerateRequest`, aceita só pela invocação do `claude` (outros CLIs recusam), com as flags conferidas no `claude --help` instalado, tempo limite de 20 min e pasta de evidências temporária apagada ao fim. Verificar: testes unitários dos argumentos com e sem vídeo e da recusa para outro CLI.
- [ ] 5.2 `GenerationRequest.video`, checagem de pré-requisitos (SKILL.md, script, `python3`, `ffmpeg`, `ffprobe` pelo PATH do shell de login), prompt com SKILL.md, transcrição e deslocamento, retorno à transcrição em falha ou tempo esgotado, e registro do resultado da análise em `generation_results` (migração). Verificar: testes com CLI falso para análise feita, skill ausente, motor não `claude`, falha e tempo esgotado.

## 6. Interface

- [ ] 6.1 Opção "Gravar vídeo da reunião" no painel de opções da gravação (`NoteEditor.tsx`) e no aviso de reunião detectada, com app e título, pedido de permissão, e i18n en e pt-BR, seguindo `~/.agents/skills/impeccable/SKILL.md`. Verificar: testes de componente (com e sem janela, permissão negada); capturas claro e escuro.
- [ ] 6.2 Aviso "Vídeo não analisado: <motivo>" na nota e indicação de vídeo analisado. Verificar: teste de componente para cada motivo em en e pt-BR.

## 7. Docs de pipeline e privacidade

- [ ] 7.1 Atualizar `docs/audio-pipeline.md` (captura, arquivos, recuperação) e `PRIVACY.md` (o que é gravado e onde fica). Verificar: texto bate com o código.

## 8. Verificação integrada

- [ ] 8.1 Gates: `pnpm check`, `pnpm typecheck`, `NODE_OPTIONS=--no-experimental-webstorage pnpm test`, `cargo fmt --manifest-path src-tauri/Cargo.toml --all --check`, `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings`, `pnpm test:rust`. Verificar: saída sem falhas novas.
- [ ] 8.2 Prova no app de dev (identificador próprio, sem tocar no perfil de release): call de teste no Google Meet com slides, gravação com vídeo, pausa e retomada, nota com o conteúdo dos slides e instantes; segunda gravação com motor de Notas Clovy mostra o aviso; apagar a nota remove o vídeo. Evidência em `.tmp/verify/gravacao-video-reuniao/`.
- [ ] 8.3 Rodar `/Users/guilhermevarela/orchestrator.v5/scripts/review-pair.sh --gate` antes do commit final e tratar os achados. Verificar: exit 0 ou achados refutados registrados aqui com o caminho do `summary.md`.

## Workflow follow-up

- Arquivar a change depois da revisão e atualizar o `AGENTS.md` do projeto.
