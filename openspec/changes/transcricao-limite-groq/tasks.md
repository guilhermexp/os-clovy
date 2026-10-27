# Tasks

## 1. Plano

- [x] 1.1 Change OpenSpec (proposal com a causa raiz, design, delta de `llm-providers`, tasks); commit do plano.

## 2. 429 com espera (D1)

- [ ] 2.1 RED: servidor falso com cabeçalhos; 429 com `retry-after` dá `retryAfterMs` sem nova tentativa imediata; `retry-after` curto espera e repete; `x-ratelimit-reset-requests` sem `retry-after`.
- [ ] 2.2 GREEN: `send_direct` lê os cabeçalhos.

## 3. Ritmo, espera e pausa (D2, D3)

- [ ] 3.1 RED: `RequestPacer` (18 por 60 s, pausa comum); turno com 429 curto espera e sai com texto; 429 longo para o stream com `transcription_rate_limited`, deixa os outros pendentes e não roda o fallback; a rodada seguinte só envia o que falta.
- [ ] 3.2 GREEN: pacer, transcritor com ritmo, marcador de pausa no stream e no fallback, frase de pausa, retomada automática em `commands.rs`.

## 4. Turnos de até 120 s (D4)

- [ ] 4.1 RED: agrupamento e corte em 120 s, abaixo de 25 MB; sessão com turnos prontos de 30 s mantém o layout antigo.
- [ ] 4.2 GREEN: `FINAL_TRANSCRIPTION_SPAN_MS` e escolha do layout.

## 5. Mensagens (D5)

- [ ] 5.1 RED: `note-failure-banner.test.tsx` traduz a frase de limite e a de pausa em pt-BR.
- [ ] 5.2 GREEN: chaves em `notes.ts` e reconhecimento no banner.

## 6. Documentação e verificação

- [ ] 6.1 `docs/llm-providers.md`, adendo datado da ADR-0070, `docs/audio-pipeline.md`.
- [ ] 6.2 `make verify` numa worktree limpa, CI `desktop.yml`, evidência em `.tmp/verify/transcricao-limite-groq/report.md`.
