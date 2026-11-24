---
title: Distinguir rejeição confirmada de entrega incerta antes de repetir uma ação
author: Guilhermexp
date: 2026-11-24
branch: feat/retirar-chat-do-clovy
id: l-8e300ad5
scope: repo
status: active
src: agent
touches:
  - openspec/changes/retirar-chat-do-clovy/tasks.md
  - src-tauri/src/connectors/google.rs
  - src-tauri/src/mcp_server/tools/connectors.rs
---

## Decided
Uma resposta HTTP 4xx recebida é rejeição conhecida mesmo se a leitura do corpo falhar. Falha de transporte sem resposta, resposta de sucesso ilegível ou erro de upstream após uma escrita continuam com efeito desconhecido: o cliente deve conferir a conta antes de repetir. Preserve o status estruturado; não deduza pelo texto de erro.

## Tried and ruled out
Classificar todo erro do Google como entrega incerta ocultava rejeições 403/429. Ler o corpo antes de classificar o status também convertia uma rejeição já conhecida em erro de rede quando o corpo vinha truncado. A fixture de loopback reproduziu essa diferença.

## Watch out
Uma pergunta do revisor sobre envio seguido de leitura não prova esse caminho: o envio atual faz um único POST e retorna a resposta. Se o handler ganhar etapas posteriores ou repetição automática, reavaliar a fronteira de incerteza. Fixtures do relay devem ignorar notificações sem id; EOF de ciclo de vida não é uma chamada de ferramenta.

In the code: src-tauri/src/connectors/google.rs::send_request e send_email; src-tauri/src/mcp_server/tools/connectors.rs; testes google_rejection_status_survives_an_incomplete_response_body e external_connector_failed_write_response_prevents_unsafe_retry; mcp_server/channel/transport_tests.rs.
