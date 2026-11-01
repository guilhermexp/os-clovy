# Spec Delta

## Purpose

Permitir que o agente opere as capacidades existentes do aplicativo com ferramentas tipadas, isolamento e resultados completos coerentes com a interface.

## ADDED Requirements

### Requirement: Gestão autorizada do produto
Turnos Clovy autorizados SHALL gerir conversas, projetos, notas, rotinas, memória, atividade, ditado, gravações e skills conforme o contrato D3. Mutações MUST revalidar escopo antes/depois da aprovação e refletir na UI. Externos/rotinas preservam seus grants; paths e partições arbitrários são recusados.

#### Scenario: Ciclo de gestão
- **WHEN** agente cria fixture, lê, edita e exclui em conversa autorizada
- **THEN** releitura e UI mostram os mesmos IDs/valores e a exclusão não remove objeto alheio
- Test: integration + UI — fixture por domínio, Claude/Codex

#### Scenario: Operações completas
- **WHEN** o agente usa o catálogo autorizado de gestão
- **THEN** pode listar/ler/criar/renomear/arquivar/restaurar/excluir conversas, ler/pesquisar mensagens e artefatos paginados, acompanhar/cancelar outros runs, gerir projetos/associações/notas, executar e configurar rotinas com histórico, editar/pesquisar memória e controlar as capacidades do contrato D3
- Test: integration — cobertura explícita por operação e domínio

#### Scenario: UI muda de partição
- **WHEN** uma chamada aguarda aprovação e a UI muda a partição ativa
- **THEN** a chamada mantém o escopo host-owned da conversa e não toca dados da nova partição
- Test: integration — duas partições/projetos e associação cruzada recusada

### Requirement: Notas e artefatos completos sem duplicação
Leitura SHALL incluir turnos definitivos de todas as sessões/fontes, com identidade, tempos, status, ordem determinística e paginação. Legados MUST continuar acessíveis sem duplicar agregado/turnos. Busca MUST apontar trecho correspondente; artefato exige conversa/ID; edição de nota exige conflito atômico por revisão esperada.

#### Scenario: Termo exclusivo no primeiro turno
- **WHEN** nota tem várias sessões Microphone/System e termo só no primeiro turno
- **THEN** paginação recupera todos os turnos uma vez e a busca aponta esse primeiro trecho
- Test: integration — legados, falhos/substituídos, paginação e conflito concorrente

### Requirement: Lifecycle tipado de gravação e ditado
As ferramentas SHALL preservar IDs de nota/sessão/tomada/destino, fontes, duração e estado efetivo, com pausa/resume/recovery/retry tipados. Stop de ditado entrega apenas ao destino previamente autorizado; recovery distingue validar de descartar. Não SHALL expor passthrough do helper, gravação invisível de teste ou elevação por enabledToolsets.

#### Scenario: Stop e retry correlacionados
- **WHEN** uma fixture autoriza operação de gravação/ditado
- **THEN** resposta identifica operação e estado reais; outra tomada/sessão não é controlada e resposta tardia não duplica efeito
- Test: integration — bridge de produção com fixture e native Mac/Windows quando aplicável

### Requirement: Skills ativas e administráveis consistentes
Descoberta, leitura e administração SHALL compartilhar identidade, precedência e raízes autorizadas. Skills ocultas/desabilitadas e backups objetivos MUST NOT aparecer como ativas; backup MUST NOT ser apagado. Skill global read-only não vira editável. Ler presença MUST NOT executar a skill.

#### Scenario: Catálogo com backup e alias
- **WHEN** raízes contêm skill válida, oculta, backup reconhecido, alias e desabilitada
- **THEN** UI e MCP concordam nas identidades e editabilidade e nenhuma leitura escapa da raiz
- Test: integration — catálogo/discovery/admin compartilhados com fixtures de symlink e overrides
