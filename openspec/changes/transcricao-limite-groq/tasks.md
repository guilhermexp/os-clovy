# Tasks

## 1. Plano

- [x] 1.1 Change OpenSpec (proposal com a causa raiz, design, delta de `llm-providers`, tasks); commit do plano (`d0ab8c0f`).

## 2. 429 com espera (D1)

- [x] 2.1 RED: servidor falso com cabeçalhos; 429 com `retry-after` dá `retryAfterMs` sem nova tentativa imediata; `retry-after` curto espera e repete; `x-ratelimit-reset-requests` sem `retry-after`.
- [x] 2.2 GREEN: `send_direct` lê os cabeçalhos.

## 3. Ritmo, espera e pausa (D2, D3)

- [x] 3.1 RED: `RequestPacer` (18 por 60 s, pausa comum); turno com 429 curto espera e sai com texto; 429 sem cabeçalho espera 60 s; 429 longo para o stream com `transcription_rate_limited`, deixa os outros pendentes e não roda o fallback; a rodada seguinte, no banco, só envia o que falta.
- [x] 3.2 GREEN: pacer, transcritor com ritmo, marcador de pausa no stream e no fallback, frase de pausa, retomada automática em `commands.rs`.

## 4. Tamanho dos pedaços (D4)

- [x] 4.1 RED: teto do turno abaixo de 25 MB; sessão com turnos prontos de 30 s mantém o layout antigo.
- [x] 4.2 GREEN: teto e escolha do layout.

## 5. Mensagens (D5)

- [x] 5.1 RED: `note-failure-banner.test.tsx` traduz a frase de limite e a de pausa em pt-BR.
- [x] 5.2 GREEN: chaves em `notes.ts` e reconhecimento no banner.

> Os RED de 2.1, 3.1, 4.1 e 5.1 saíram juntos no `48cef792`; o ponta a ponta reproduzia a falha relatada (`transcription_partially_failed`).

## 6. Revisão do orquestrador: 10 min, ASH e prévia (D2, D4, D6)

- [x] 6.1 Teto de 600 s, corte com 10 s de sobreposição e remoção das palavras repetidas.
- [x] 6.2 Orçamento de 7.200 s de áudio por hora no pacer; espera longa vira pausa.
- [x] 6.3 Pacotes de turnos por faixa com `verbose_json` e tempos de segmento, só com a chave Groq.
- [x] 6.4 Prévia ao vivo pelo mesmo pacer, sem espera e com no máximo 3.600 s por hora.

> A revisão chegou com a implementação em andamento, então os testes de 6.1 a 6.4 vieram depois do código, no mesmo commit GREEN (`6bf8541a`). Cada um foi conferido por mutação: com os pacotes desligados e com o orçamento de áudio desligado, os testes de pacote e de orçamento falham (4 falhas); com o código de volta, passam.

## 8. Revisão 1 (G1–G4)

Revisão do `7bb9ae04` bloqueada (BLOCK) com quatro achados.

- [x] 8.0 RED dos quatro (`7b2681fe`): o teste do G1 reproduz a nota que nunca termina, e o do G4 mostra dois vencedores na reivindicação.
- [x] 8.1 G2 GREEN (`d7560bf7`): pacer no relógio de parede injetável, protegido contra volta, olhando o relógio a cada 30 s.
- [x] 8.2 G3 GREEN (`c248fadf`): um pacer por conta Groq (impressão digital SHA-256) e um próprio da rota do Clovy API, escolhidos por requisição.
- [x] 8.3 G1 GREEN (`c4307809`): `note_transcription_chunks` (migração 51) guarda cada pedaço pronto de um job dividido.
- [x] 8.4 G4 GREEN (`3ce54680`): reivindicação atômica da pausa e revalidação ao pegar o lock.
- [ ] 8.5 `make verify` numa worktree limpa, CI, report.

> Os RED dos quatro saíram num commit só porque os testes ficam no mesmo arquivo; os GREEN saíram um por achado, cada um fazendo passar só os seus testes. O teste do G1 usa o relógio do G2, então o G2 veio primeiro.

## 7. Documentação e verificação

- [x] 7.1 `docs/llm-providers.md`, adendo datado da ADR-0070, `docs/audio-pipeline.md`, termo `Turn pack` no `CONTEXT.md`.
- [x] 7.2 `make verify` numa worktree limpa, CI `desktop.yml`, evidência em `.tmp/verify/transcricao-limite-groq/report.md`.

> 7.2: `make verify` no `e0fdeb0c`: a primeira rodada caiu por timeout de 5 s em `cargo-clovy-runner.test.mjs` com a máquina carregada (passa isolado, 2/2); a segunda deu EXIT=0 (vitest 1867/1867, Rust 2014 + integração). CI run 40606541165 só com os vermelhos conhecidos (vitest `mcp-server-dev-runner`, clippy `dictation.rs:211`/`notion.rs`, clipboard `activity::windows`); por eles os testes Rust do macOS não rodaram no CI. Sem chamada real à Groq e sem prova nativa no app.
