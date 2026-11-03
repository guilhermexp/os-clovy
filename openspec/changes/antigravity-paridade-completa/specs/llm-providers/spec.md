# Spec Delta

## ADDED Requirements

### Requirement: Motores intercambiáveis sem dependência cruzada
O Clovy SHALL funcionar com apenas Claude Code, apenas Codex ou apenas Antigravity instalado, em todas as superfícies que aceitam esses motores. A escolha explícita MUST prevalecer. Sem escolha explícita, a resolução automática MUST usar um motor elegível instalado, mantendo a preferência existente e incluindo Antigravity. Falta de autenticação MUST ser explicada, sem trocar de motor silenciosamente.

#### Scenario: Um único motor instalado
- **WHEN** somente um dos três motores está instalado
- **THEN** novas sessões, chats de nota e rotinas elegíveis usam esse motor sem iniciar nem exigir os outros dois

#### Scenario: Escolha explícita preservada
- **WHEN** o usuário já escolheu um dos três motores e muda a lista de instalados
- **THEN** a escolha é mantida e uma eventual indisponibilidade é explicada sem troca silenciosa

### Requirement: Pool único das ferramentas do Clovy
Claude Code, Codex e Antigravity SHALL receber do mesmo gateway o catálogo autorizado das ferramentas do Clovy e de seus conectores. O alcance MUST depender da superfície, do vínculo e da política do app, não do motor. Credenciais, efeitos, aprovações e clarificações MUST permanecer sob a autoridade do Clovy, sem catálogos ou autorizações paralelos por runtime.

#### Scenario: Contexto e ação aprovados
- **WHEN** uma conversa em qualquer um dos três motores referencia uma nota e solicita uma alteração com aprovação
- **THEN** a leitura usa o gateway do Clovy, a alteração aguarda a decisão no app e a aprovação produz exatamente um efeito e o resultado correspondente, com os mesmos contratos nos três motores

#### Scenario: Negação e cancelamento
- **WHEN** o usuário nega ou cancela enquanto uma ferramenta aguarda aprovação
- **THEN** a ferramenta não recebe autorização, a autoridade é revogada e o histórico termina sem indicador de execução permanente nem resultado fabricado

#### Scenario: Continuidade e histórico
- **WHEN** duas mensagens são enviadas e a conversa é reaberta
- **THEN** a segunda execução retoma a conversa do motor atual e os itens reais mantêm identidade, ordem e resultado

#### Scenario: Alternância preserva contexto e alcance
- **WHEN** o usuário alterna Claude Code, Codex e Antigravity entre mensagens da mesma conversa, inclusive retornando ao primeiro motor
- **THEN** o novo turno recebe a continuidade do contexto autorizado a partir do histórico persistido, usa o mesmo pool e não retoma uma conversa interna desatualizada nem reutiliza aprovações ou vínculos do motor anterior

#### Scenario: Conector e ferramenta com pré-requisito
- **WHEN** uma ferramenta do gateway depende de conector, credencial, permissão nativa ou provedor de mídia
- **THEN** o pré-requisito e o erro são iguais nos três motores; trocar o motor não concede autorização nem remove a ferramenta por restrição específica do adaptador

### Requirement: Antigravity nos recursos de texto
Antigravity SHALL poder executar a geração de notas e os recursos de atividade que já aceitam provedores CLI, após teste efetivo do schema necessário. Conteúdo enviado MUST ser tratado como dado não confiável e identificado como autoria do Clovy para evitar autoingestão. Esse suporte MUST NOT exigir a instalação de outro agente nem alterar o motor de transcrição ou a limpeza de ditado.

#### Scenario: Nota e atividade sem outros agentes
- **WHEN** Antigravity é o único agente instalado, é escolhido para Notas e passa no teste de schema para Atividade
- **THEN** uma transcrição sintética gera a nota e os dados sintéticos de atividade geram o resumo esperado sem chamada a Claude Code, Codex ou provedor alternativo

#### Scenario: Falha ou schema inválido
- **WHEN** Antigravity falha, excede o prazo ou não devolve o schema exigido
- **THEN** o uso falha com diagnóstico explícito, sem resultado falso, processo órfão ou troca de provedor

#### Scenario: Leitura de sessões é independente
- **WHEN** o usuário liga a leitura de sessões Antigravity
- **THEN** isso não altera por si só o provedor do chat ou da atividade, não modifica os arquivos originais e não reingere chamadas marcadas do próprio Clovy
