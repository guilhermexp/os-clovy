# Spec Delta

## MODIFIED Requirements

### Requirement: Pedido de aprovação visível na sessão
Uma ferramenta que exige aprovação SHALL registrar no broker um pedido ligado ao vínculo de turno. No modo aprovações, o app MUST gravar interrupção, pôr execução em `waiting_for_user` e emitir evento existente com cartão e prévia sanitizada gerada pelo host. No bypass explícito efetivo do run, o app MUST persistir recibo correlacionado de autorização por política antes de liberar a chamada, sem inventar aprovação humana ou aguardar cartão.

#### Scenario: Cartão aparece
- **WHEN** turno com aprovações chama mutação protegida
- **THEN** sessão mostra pedido pendente e execução aguarda
- Test: integration — broker/item/evento reais

#### Scenario: Bypass com recibo
- **WHEN** turno assistido com bypass efetivo chama mutação protegida
- **THEN** recibo identifica origem de política, sessão/run/tool/call corretos, e falha ao persistir impede efeito
- Test: integration — recibo antes do dispatch e falha de armazenamento

### Requirement: Decisão única
O broker SHALL aceitar uma única decisão por pedido. Aprovação `once` libera a chamada; negação devolve `user_denied`. Escopos `session` e `always` MUST ser recusados explicitamente enquanto não implementados, nunca reduzidos silenciosamente a `once`. Segunda decisão MUST falhar sem efeito.

#### Scenario: Duas respostas simultâneas
- **WHEN** duas respostas concorrem pelo mesmo pedido
- **THEN** só uma decisão libera a ferramenta
- Test: integration — resolução concorrente e uma mutação

#### Scenario: Escopo não implementado
- **WHEN** cliente envia `session` ou `always`
- **THEN** recebe erro de resolução inválida e a ação não é liberada
- Test: unit — escopo recusado

## ADDED Requirements

### Requirement: Escolha explícita e vigência por execução
O app SHALL oferecer Aprovações/Bypass na conversa existente e Home, guardar preferência por conversa e capturar política efetiva ao iniciar cada execução. Alteração durante run vale no próximo; a UI MUST mostrar essa vigência e oferecer Stop para encerramento imediato do run atual. Run pendente MUST NOT ser liberado retroativamente. Conversas antigas/novas, imports, branches, rotinas e externos MUST NOT herdar bypass de outra conversa. O modelo MUST NOT mudar a própria política.

#### Scenario: Trocar durante cartão pendente
- **WHEN** usuário liga bypass enquanto run aguarda decisão
- **THEN** preferência é salva para próximo run, cartão atual continua pendente e nenhuma chamada duplica
- Test: integration/component — toggle, próximo run, reabrir e outra conversa

### Requirement: Bypass não concede capacidades
Bypass SHALL atuar apenas na camada de pedidos do broker, preservando clarificações, restrições reais do CLI, capabilities, app/janela/aba, campos sensíveis, autenticação, recibos consumíveis e permissões do SO. Cancelamento/revogação MUST bloquear novos efeitos; entrega incerta MUST NOT ser repetida automaticamente.

#### Scenario: Clarificação e recurso desligado
- **WHEN** run em bypass pede esclarecimento ou usa recurso desabilitado
- **THEN** pergunta continua humana e recurso permanece bloqueado
- Test: integration — ambos CLIs, rotina/external negados e recibo de conector não reutilizável
