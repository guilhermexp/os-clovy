# Spec Delta

## MODIFIED Requirements

### Requirement: Disponibilidade só local
O uso do computador SHALL ficar pronto quando usuário o ativou, agent-desktop pinado está empacotado e os pré-requisitos reais da plataforma estão atendidos. macOS exige Acessibilidade/Gravação de Tela pertinentes; Windows exige sessão/desktop interativos, identidade e integridade compatíveis, sem inventar TCC ou elevar UAC. Status MUST NOT depender de conta, plano, serviço remoto Clovy ou catálogo de modelos. Ausência/falha MUST informar causa real.

#### Scenario: Sem conta Clovy
- **WHEN** usuário sem plano Pro/Max ativa uso do computador com motor e permissões/pré-requisitos locais corretos
- **THEN** status fica pronto independentemente de conta ou catálogo remoto
- Test: integration — estados locais e native por plataforma

#### Scenario: Clovy API fora do ar
- **WHEN** Clovy API não responde
- **THEN** status e ativação locais não mudam
- Test: integration — sem dependência de rollout ou modelos remotos

### Requirement: Só pelos CLIs com MCP do Clovy
A ferramenta `computer_use` SHALL estar disponível apenas a turnos CLI vinculados/autorizados através de clovy-mcp. Cada ação MUST respeitar política efetiva de aprovação e grants de app/janela, mesmo em bypass. Capturas voltam ao CLI, nunca a outro modelo/provedor por causa desta capacidade.

#### Scenario: Turno no Claude Code
- **WHEN** Claude Code assistido e autorizado chama `computer_use`
- **THEN** política efetiva de aprovação é respeitada e captura da janela correta retorna como bloco MCP
- Test: integration — catálogo/dispatch; native — approvals e bypass explícito

#### Scenario: Turno no agente nativo
- **WHEN** um caminho não CLI tenta oferecer `computer_use`
- **THEN** a ferramenta não está na superfície disponível
- Test: integration — ausência fora de turno CLI vinculado

#### Scenario: Turno no Codex
- **WHEN** Codex assistido e autorizado chama `computer_use`
- **THEN** política efetiva é respeitada e captura da janela correta retorna sem MCP upstream irrestrito
- Test: integration e native Codex

## ADDED Requirements

### Requirement: Background primeiro e escalação sem replay
A automação SHALL preservar segundo plano sempre que operação for suportada. Quando não houver suporte background, o modo visível pode ser usado conforme autorização do dono, exibindo modalidade efetiva e mantendo app/janela exatos. Essa escalação MUST NOT conceder privilégio de administrador, permissão do SO ou login; MUST NOT repetir mutação entregue ou incerta. Coordenadas, escala, refs e campos sensíveis continuam sujeitos ao broker.

#### Scenario: Operação sem suporte background
- **WHEN** ação é classificada antes do efeito como não suportada em segundo plano
- **THEN** execução pode usar modo visível no alvo autorizado, nunca outro app/janela ou replay de efeito incerto
- Test: native — pointer/frontmost observer, duas janelas, operação semântica e física

### Requirement: Helper privado e Stop forte
O motor SHALL executar atrás do helper privado autenticado do Clovy, com identidade de peer, lifetime/refs por run e Stop sticky. Stop/revogação/shutdown MUST encerrar todos os processos/controladores de tarefa e impedir mutação tardia ou reuso de autorização. Build/versão/origem/licença MUST ser verificáveis e o backend antigo não terá fallback automático.

#### Scenario: Cancelar ação em voo
- **WHEN** usuário para ou revoga durante uma chamada
- **THEN** controle termina sem helper/host órfão autorizado, sem segunda mutação e com disposição de entrega preservada
- Test: integration — supervisor/epoch e native Mac/Windows assinado
