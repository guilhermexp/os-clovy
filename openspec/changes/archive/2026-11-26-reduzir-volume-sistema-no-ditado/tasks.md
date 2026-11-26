# Tasks

## 1. Controle nativo da saída

- [x] 1.1 Criar política portável e adapter CoreAudio macOS em módulo próprio de redução de saída; provar RED/GREEN com fonte real para padrão off, escala relativa 0.2, master/canais, balanço, mute prévio, dispositivo sem suporte, erro parcial/readback/quantização, mudança manual inclusive redução durante aquisição, combinação de SET assíncrono com quantização, troca de UID e ID reaproveitado; não confirmar restauração em caso ambíguo e documentar em `docs/dictation.md` os limites reais de hardware e ausência de garantia após SIGKILL do host.
- [x] 1.2 Integrar eventos completos de tomada/geração ao executor serial no host, sem alterar Swift/protocolo ou `sync_capture_mic_duck`; provar testes de início duplicado, stop/error antigo, helper antigo morto, stop/off concorrente com aquisição, stop antes do ASR, mic-test sem efeito e teardown com restauração limitada. Verificar que callbacks HAL só enfileiram, sem bloquear captura ou mutex de settings.

## 2. Preferência e interface

- [x] 2.1 Adicionar `reduceSystemVolume` default false, DTO, setter transacional e wrapper/registro Tauri; provar falha de save sem mudança publicada, serialização compatível e preservação dos outros campos com testes Rust/contrato de comando.
- [x] 2.2 Acrescentar Toggle pt-BR/en em Ditado > Microfone, apenas no macOS, usando o snapshot confirmado/guard de sessão de AppSettings; verificar OFF/ON, save rejeitado, resposta obsoleta, reabertura, outra plataforma, desligar durante captura e ligar somente para próxima tomada. Rodar teste focado e prova visual sintética nas duas línguas/larguras; usar Impeccable sem redesenhar Configurações.

## 3. Integração da entrega

- [x] 3.1 Na box, executar `pnpm check`, `pnpm typecheck`, `pnpm test`, `pnpm build`, Rustfmt e harness portável da fonte real; conferir a árvore/diff/novos arquivos, SHA-256, Gitleaks integral dos arquivos alterados e gate `review-pair.sh --gate` ou contingência manual comprovada. Recibo previsto `.tmp/verify/output-ducking-20261126/manual-review/1/summary.md` no checkout durável; sem rotular mocks como prova de áudio real.
- [x] 3.2 Após revisão, publicar candidato em branch de teste sem force e conferir CI `desktop.yml` no SHA exato, incluindo compilação, Clippy e suíte Cargo macOS; separar status de cada job e assinatura Windows ausente de falhas de fonte. Testes físicos de saída/AX/colagem não estão autorizados por este item.

## Workflow follow-up

- Integrar código aceito à main sem force, preservar WIP externo do checkout principal; sem tag/release.
- Arquivar apenas com critérios satisfeitos, sincronizar instruções/specs/memória e limpar recursos próprios, preservando recibos e dados do usuário.
- Informar comparação Liv/Clovy e limites de validação separadamente do recurso implementado; não mudar pipeline ASR por comparação sem teste controlado.

## Prova final da candidata

CI `45876566162`, SHA `caafb127ddb1dd220aaa4451db2ebee27d1b432e`: macOS ARM64 Swift, protocolo, Clippy e Cargo integral aprovados (2678 testes, zero falhas,16ignorados); Ubuntu frontend1643/5pulados, Biome, formato e Companion aprovados. Workflow geral falhou só na assinatura Windows, sem validar testes/pacote Windows. Revisão manual de contingência nos recibos duráveis `.tmp/verify/combined-dictation-20261126/manual-review/1/summary.md` e `.tmp/verify/output-ducking-20261126/manual-review/1/summary.md`. Sem prova física de volume/AX/colagem, inferência paga ou release. Decisão de captura/cue pertence ao escopo separado e segue pendente.
