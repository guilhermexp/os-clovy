# Spec Delta

## Purpose

Define quando e como o agente nativo é apagado, o que sobrevive e como os usuários existentes atravessam o corte.

## ADDED Requirements

### Requirement: Critério de corte
A remoção SHALL começar só depois de: fases 0 e 2 concluídas; `make verify` sem falhas; testes de paridade do delta `clovy-mcp-server` verdes para Claude e Codex; smoke nativo isolado com turno real de Claude e de Codex usando ao menos uma ferramenta de cada grupo de escrita com aprovação e com negação; nenhum ponto de entrada do app iniciando `run.start`; e aceite explícito do dono.

#### Scenario: Corte bloqueado
- **WHEN** algum ponto de entrada ainda inicia o runtime nativo
- **THEN** a fase 4 não começa
- Test: static — busca por `run.start` e `AgentRuntimeHost` fora dos arquivos a apagar retorna vazio antes do corte

### Requirement: Nada do sidecar no produto
Depois do corte, o bundle, os scripts de build, o CI e o processo do app MUST NOT conter nem iniciar o sidecar do agente nativo.

#### Scenario: Bundle sem sidecar
- **WHEN** o app de release é montado
- **THEN** não existe `clovy-agent-runtime` no bundle e o app inicia sem esse processo
- Test: integration — build do bundle e listagem dos recursos; smoke nativo com lista de processos filhos

### Requirement: Histórico preservado
Sessões criadas pelo motor nativo SHALL continuar listáveis com itens e artefatos. Enviar ou repetir numa sessão nativa MUST usar o motor CLI padrão com o histórico como contexto. Execuções nativas que aguardavam o usuário MUST ser encerradas como canceladas por migração.

#### Scenario: Sessão antiga continua
- **WHEN** o usuário abre uma sessão nativa e envia uma mensagem
- **THEN** o histórico aparece completo e a resposta vem do CLI padrão com as mensagens anteriores no prompt
- Test: integration — banco com sessão nativa de fixture e CLI falso que grava o prompt

### Requirement: Configurações sem motor nativo
`llm_usage.chat` MUST aceitar apenas CLIs depois do corte; valores `clovy` e `endpoint` salvos MUST migrar uma vez para o CLI padrão. Endpoints registrados MUST continuar disponíveis para notas, limpeza e atividade.

#### Scenario: Perfil com chat no Clovy
- **WHEN** um perfil salvo com chat `clovy` abre após o corte
- **THEN** o chat passa ao CLI padrão uma única vez e a escolha de notas não muda
- Test: integration — `provider-settings.json` de fixture, carga, gravação e recarga
