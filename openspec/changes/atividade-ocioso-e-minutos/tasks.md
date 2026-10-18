# Tasks

## 1. Tempo ocioso do sistema na captura

- [ ] 1.1 Confirmar a premissa de D1: no app de desenvolvimento (perfil `co.opensoftware.june-dev`) sem Monitoramento de Entrada, ler `CGEventSourceSecondsSinceLastEventType(HIDSystemState, AnyInput)` e observar o valor cair ao mover o mouse e crescer parado; registrar em `.tmp/verify/atividade-ocioso-e-minutos/`. Se exigir permissão, parar e voltar ao dono.
- [ ] 1.2 RED: testes do motor com plataforma falsa (tempo ocioso 500 ms e sem valor) e da migração 6 (coluna `frames.input_idle_ms`, catálogo append-only); verificar que falham.
- [ ] 1.3 GREEN: `ActivityPlatform::system_idle_ms`, implementação macOS, leitura no tick (`engine.rs`), `NewFrame`/`StoredFrame`/`insert_frame` e migração 6; testes de 1.2 passam e `cargo test` do módulo `activity` fica verde.
- [ ] 1.4 Atualizar `docs/activity-capture.md` (coluna nova, origem do dado, sem permissão nova) e conferir que o texto bate com o código.

## 2. Regra do quadro útil

- [ ] 2.1 RED: testes no `builder.rs` para leitura com rolagem (quadros iguais com tempo ocioso 1 s viram sessão), tela parada (tempo crescente vira lacuna ociosa), quadros sem o valor (classificação antiga) e tela bloqueada com entrada (continua não útil); verificar que falham.
- [ ] 2.2 GREEN: ETL e `StoredFrame` carregam `input_idle_ms`; `push_frame` aceita `input_idle_ms <= FRAME_SPAN + 1 s`; testes de 2.1 e os existentes do builder, do ETL e de `stats.rs` passam.
- [ ] 2.3 Atualizar `docs/activity-timeline.md` (regra de quadro útil) e conferir com o código.

## 3. Tempos por atividade em segundos

- [ ] 3.1 RED: testes de `normalize_seconds` (42 min com 30/30 → 21/21 min; 120 s com pesos 10/1/1 → três partes > 0 somando 120; peso zero ou inválido recebe parte > 0; `active_seconds < n`), da dobra e da leitura das frentes somando `seconds` com fallback para relatório antigo só com `minutes`; verificar que falham.
- [ ] 3.2 GREEN: `normalize_seconds`, `HourActivity.seconds`, `active_seconds` em `pipeline.rs`, soma em `workstreams.rs` e `db.rs`, DTO das frentes com segundos; testes de 3.1 e `pipeline_tests.rs` passam.
- [ ] 3.3 RED: testes de componente para atividade e frente de 40 s ("< 1 min"), total "ativos" incluindo segundos, e relatório da hora no `DaySummaryPanel` sem "0 min"; verificar que falham.
- [ ] 3.4 GREEN: `workstream-timeline-layout.ts`, `WorkstreamTimeline.tsx`, `DaySummaryPanel.tsx` e mensagens usam segundos e `formatDuration`; testes de 3.3 e os existentes de `workstream-timeline-*` e `day-intelligence` passam.
- [ ] 3.5 Atualizar `docs/day-intelligence.md` (repartição em segundos, parte mínima, "< 1 min") e conferir com o código.

## 4. Verificação

- [ ] 4.1 `make verify` verde no Mac (frontend, Rust desktop com clippy `-D warnings`, companion, clovy-api); contagens registradas em `.tmp/verify/atividade-ocioso-e-minutos/`.
- [ ] 4.2 Prova no app de desenvolvimento: ler 10 min rolando sem clicar não gera lacuna ociosa; uma hora concluída com atividade curta mostra "< 1 min" e nunca "0 min"; capturas de tela e números do export de depuração em `.tmp/verify/atividade-ocioso-e-minutos/`.

## Workflow follow-up

- Arquivar `add-activity-intelligence` antes de arquivar esta change.
- Revisão de código, aceite do dono, arquivamento, `/sync` e `/cleanup`.
