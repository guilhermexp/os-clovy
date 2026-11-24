# cli-approval-broker Specification

## Purpose
Define como uma ferramenta chamada por um turno CLI pede aprovação ou resposta ao usuário através do app, e como a decisão volta como resultado da ferramenta.

No fork, as decisões são locais ao app. O requisito anterior de aprovação pelo Companion foi superado pela retirada desse produto, já registrada na base desta change em `CONTEXT.md` e documentada em `docs/companion-protocol.md`; os testes históricos de protocolo não comprovam aprovação de um turno CLI pelo telefone.

## Requirements

### Requirement: Decisão única
O broker SHALL aceitar uma única decisão por pedido pelo comando de resolução existente. Aprovar MUST liberar a ferramenta; negar MUST devolver `isError` com `user_denied`. Escolhas `session` e `always` MUST valer como aprovação única. Uma segunda decisão MUST ser recusada sem efeito.

#### Scenario: Duas respostas simultâneas
- **WHEN** duas resoluções locais respondem ao mesmo pedido ao mesmo tempo
- **THEN** só a primeira decisão vale e a ferramenta executa no máximo uma vez
- Test: integration — duas resoluções concorrentes contra o mesmo item e contagem de execuções

### Requirement: Timeout, cancelamento e encerramento
O broker SHALL expirar pedidos sem resposta após `APPROVAL_TIMEOUT`, menor que o timeout de ferramenta passado ao CLI, devolvendo `approval_timed_out` e marcando o item como expirado. Cancelar a execução ou fechar o app MUST resolver pedidos pendentes como `run_cancelled`. Na abertura seguinte, pedidos CLI pendentes MUST ser marcados cancelados.

#### Scenario: Ninguém responde
- **WHEN** um pedido fica sem resposta até o timeout
- **THEN** a ferramenta devolve `approval_timed_out` e o cartão aparece como expirado
- Test: unit — relógio controlado no broker

#### Scenario: Cancelar durante a espera
- **WHEN** o usuário cancela a execução com um cartão pendente
- **THEN** a ferramenta devolve `run_cancelled`, o cartão fica cancelado e nenhum efeito acontece
- Test: integration — `cancel_agent_run` com CLI falso aguardando a ferramenta

### Requirement: Turnos não assistidos não pedem aprovação
Para vínculos não assistidos (rotinas), o broker MUST negar imediatamente com `unattended_requires_approval`, sem gravar cartão pendente.

#### Scenario: Rotina tenta escrever
- **WHEN** uma rotina em CLI chama `save_memory`
- **THEN** a ferramenta falha com `unattended_requires_approval` e o histórico da rotina registra a recusa
- Test: integration — vínculo com `attended=false` e banco de memórias inalterado

### Requirement: Clarificação pelo mesmo mecanismo
`request_clarification` SHALL criar um pedido de tipo clarificação com pergunta e opções e devolver `{answer}` como resultado da ferramenta quando o usuário responde.

#### Scenario: Pergunta respondida
- **WHEN** o turno chama `request_clarification` e o usuário escolhe uma opção
- **THEN** a ferramenta devolve exatamente a resposta escolhida
- Test: integration — broker real e resposta pelo comando de resolução

### Requirement: Prompts de permissão do Claude Code
Quando o spike S1 confirmar o suporte, turnos Claude SHALL encaminhar os pedidos de permissão das ferramentas próprias do CLI para `approve_action`, que MUST usar o broker e responder no formato esperado pelo CLI (permitir com a entrada, ou negar com mensagem). Sem suporte confirmado, o comportamento atual (`needsApproval` no item) MUST continuar.

#### Scenario: Shell negado
- **WHEN** o Claude pede permissão para rodar um comando e o usuário nega
- **THEN** o CLI recebe a negação e o item da ferramenta aparece como recusado pelo usuário
- Test: integration — CLI falso que chama `approve_action` como o Claude faria; smoke real com Claude na fase 3

### Requirement: Aprovação externa visível no Clovy
O broker SHALL disponibilizar aprovações de clientes MCP externos na área de MCP do Clovy, com indicador de pendências. A interface MUST identificar ação, alvo e perfil quando aplicável, oferecer uma decisão única e registrar o resultado. O efeito MUST respeitar a autorização emitida para a chamada.

#### Scenario: Ação solicitada de fora
- **WHEN** um cliente externo pede uma ação protegida e o usuário abre o pedido
- **THEN** ele vê a ação e seu alvo e pode autorizar uma vez ou negar, sem abrir chat

#### Scenario: Pedido cancelado
- **WHEN** a chamada termina antes da decisão
- **THEN** o pedido deixa de estar acionável e nenhuma aprovação tardia produz efeito
