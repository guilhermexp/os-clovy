# Tasks

## 1. Plano

- [x] 1.1 Proposal, design, spec delta `meeting-detection` e tasks; `openspec validate deteccao-reuniao-windows --strict`; commit do plano.

## 2. Sonda e lista no Windows (D2, D3, D4, D5)

- [ ] 2.1 RED: testes do parser (`windows_input_processes`: sessão WASAPI ativa, subida Chromium e WebView2, exclusão de `owned_pids` depois da subida, reforço do registro com e sem processo, entradas fora da lista ignoradas), da lista e dos rótulos do Windows, e da máquina de estados (aviso e fim) com processos do Windows; commit RED.
- [ ] 2.2 GREEN: parser puro em `meeting_detection/windows_probe.rs`, tabela `WINDOWS_MIC_APPS`, rótulo sem `.exe`; testes de 2.1 passam.

## 3. Leitura no Windows e monitor (D1, D6)

- [ ] 3.1 Leitura WASAPI, Toolhelp e registro sob `cfg(target_os = "windows")`; `active_input_processes` do Windows usa o parser.
- [ ] 3.2 `spawn_monitor`, `observe_meeting_end` e `setup` ligados no macOS e no Windows.

## 4. Docs

- [ ] 4.1 `docs/audio-pipeline.md` descreve a sonda do Windows e o reforço do registro.

## 5. Verificação

- [ ] 5.1 `make verify` numa worktree limpa.
- [ ] 5.2 CI Windows (`desktop.yml`) verde, salvo os vermelhos conhecidos.
- [ ] 5.3 Windows de prova: binário de teste com captura do microfone visto pela sonda, o próprio processo excluído, fim observado; relatório em `.tmp/verify/deteccao-reuniao-windows/report.md`.
