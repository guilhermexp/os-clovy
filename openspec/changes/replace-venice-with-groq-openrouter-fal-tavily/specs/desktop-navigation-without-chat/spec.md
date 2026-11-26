# Spec Delta

## ADDED Requirements

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
