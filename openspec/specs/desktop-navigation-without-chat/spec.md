# desktop-navigation-without-chat Specification

## Purpose

Define a navegação do Clovy como aplicativo de Atividade, notas, reuniões, projetos, ditado e rotinas, com as conversas feitas por clientes externos.

## Requirements

### Requirement: Interface sem chat interno
O Clovy SHALL abrir em Hoje e apresentar suas áreas de Atividade, notas de reunião, projetos, ditado, rotinas e configurações. O shell, menu nativo, bandeja, atalhos, busca, abas, notificações, notas e projetos MUST deixar de oferecer abertura ou criação de chat. Navegação legada de chat SHALL redirecionar para Hoje sem iniciar um turno.

#### Scenario: Abre e navega
- **WHEN** o usuário abre o app, abre uma aba ou segue um destino legado de chat
- **THEN** ele alcança uma vista suportada do app, sem composer, conversa ou início de turno

### Requirement: Atividade e rotinas disponíveis
Captura, classificação, resumos, linha do tempo, provedores e leitura de agentes de código da Atividade SHALL continuar disponíveis. A configuração, execução e consulta de resultados de rotinas MUST funcionar em Rotinas sem interface de conversa.

#### Scenario: Vê resultado de rotina
- **WHEN** o usuário abre uma execução no histórico de Rotinas
- **THEN** vê o estado e resultado ou o erro em uma visualização sem campo de envio

### Requirement: Configurações sem agente interno
As configurações SHALL deixar de oferecer página Agente, seleção de motor para chat e geração de imagem/vídeo. Destinos antigos dessa página SHALL abrir uma configuração válida. Motores CLI, skills e sons ainda usados por rotinas, notas ou Atividade MUST permanecer funcionais.

#### Scenario: Destino antigo de Agente
- **WHEN** o usuário abre uma aba persistida ou um atalho antigo para Agente
- **THEN** abre Geral sem painel vazio, chat ou turno iniciado, e pode configurar sons de rotinas preservando sua preferência anterior

#### Scenario: CLIs continuam disponíveis
- **WHEN** o usuário configura ou executa notas, Atividade ou rotinas após a atualização
- **THEN** detecção, autenticação, seleção e execução dos motores suportados continuam disponíveis, sem seletor de chat interno

### Requirement: Integrações preservadas e gerenciáveis
O sistema SHALL manter integrações, conectores e servidores MCP personalizados com seus controles de configuração e autoridade existentes. A remoção da página Agente MUST NOT desconectar serviços, revogar credenciais ou apagar dados de integrações.

#### Scenario: Integração existente após a limpeza
- **WHEN** o usuário abre Plugins ou um agente externo chama uma integração configurada
- **THEN** a configuração e as ferramentas permanecem disponíveis com as mesmas permissões e aprovações; a interface não descreve sessões de chat que deixaram de existir
