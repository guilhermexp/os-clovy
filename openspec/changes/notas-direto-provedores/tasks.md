# Tasks

## 1. Plano

- [ ] 1.1 Change OpenSpec (proposal, design, delta de `llm-providers`, tasks) e ADR-0070 estendendo a ADR-0065 §5 para notas; commit do plano.

## 2. Transcrição de nota direta na Groq (D1, D2, D5)

- [ ] 2.1 RED: testes em `clovy_api_notes_tests.rs` para direto com chave (turno final e prévia), nova tentativa em 503 e nenhuma em 400, `no_speech`, 401/403 rejeitada, limite de 25 MB, fallback ao Clovy API sem chave e `groq_api_key_missing` com porta fechada; commit RED.
- [ ] 2.2 GREEN: `transcribe_saved_audio_via` e `transcribe_saved_audio` lendo a chave Groq; testes de 2.1 passam.

## 3. Geração e limpeza diretas na OpenRouter (D3, D4, D5)

- [ ] 3.1 RED: testes para geração direta com chave, Auto e sem chave pelo Clovy API, `openrouter_api_key_missing` com porta fechada e limpeza `note_transcript_cleanup` com o contrato do servidor; teste em `processing.rs` para a frase da geração sem chave; commit RED.
- [ ] 3.2 GREEN: `generate_note_via`, `cleanup_text` por `dictate_cleanup_via` com o contrato da nota; testes de 3.1 passam.

## 4. Mensagem na nota (D5)

- [ ] 4.1 RED: `note-failure-banner.test.tsx` traduz as frases novas em pt-BR; commit RED.
- [ ] 4.2 GREEN: chaves `notes.failure.*` novas em en e pt-BR e `STORED_FAILURE_KEYS`.

## 5. Documentação e verificação

- [ ] 5.1 `docs/llm-providers.md`, `docs/configuration.md`, `AGENTS.md` e README/PRIVACY se citarem o fluxo.
- [ ] 5.2 `make verify` numa worktree limpa, CI `desktop.yml` na branch, evidência em `.tmp/verify/notas-direto-provedores/report.md`.
