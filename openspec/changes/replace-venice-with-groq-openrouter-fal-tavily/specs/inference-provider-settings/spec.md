# Spec Delta

## Purpose

Selecionar, configurar e migrar os provedores de inferência preservando o perfil e retirando totalmente caminhos ativos de Venice.

## ADDED Requirements

### Requirement: Corte limpo Venice
O sistema SHALL remover configuração, IPC/DTOs, adapters, branding, headers, aliases e fallbacks ativos de Venice e migrar todos os consumidores sem reexport ou shim legado.

#### Scenario: Cliente atualizado
- **WHEN** o usuário abre a versão atualizada
- **THEN** pickers e ações usam Groq/OpenRouter/Tavily e não exibem controles Venice nem chamam endpoints Venice
- Test: integration — chamadas das superfícies atualizadas com observação de destinos; busca exaustiva de código ativo é evidência suplementar, não teste de texto-fonte.

### Requirement: Migração preserva perfil e história
O sistema SHALL migrar seleções globais e overrides legados para modelos válidos dos novos provedores sem perder CLI/endpoints, preferências independentes ou registros históricos de provider e conteúdo.

#### Scenario: Perfil legado e override
- **WHEN** um perfil com modelos Venice, overrides e escolhas CLI existentes é atualizado
- **THEN** áudio legado migra conforme `selecionar-modelos-transcricao` preservando seleções válidas, texto remoto a OpenRouter e preferências de mídia inativas, mantendo CLI/endpoints e história intactos após save/restart
- Test: unit — migração consumer-visible com mapa de overrides e histórico preservado; e2e em cópia sandbox com restart/read-back.

#### Scenario: Controle do aprimoramento visível e escolha preservada
- **WHEN** o usuário abre Modelos > Transcrição de áudio ou Ditado > Estilo
- **THEN** as superfícies compartilham a preferência nativa existente, preservam exatamente `cleanupEnabled`, `cleanupModel` e estilo, e trocar somente modelo não habilita o aprimoramento
- Test: component/integration — navegar entre as duas apresentações, desligar/religar, atualizar modelo enquanto OFF e reabrir, usando store isolado.

#### Scenario: Leitura desconhecida ou salvamento concorrente
- **WHEN** a leitura ainda não terminou, falha, ou uma gravação está em andamento
- **THEN** a UI não afirma OFF como confirmado, bloqueia mutações conflitantes e informa o estado; falha de save mantém o último estado confirmado, e respostas tardias de idioma/microfone/atalho não sobrescrevem a escolha de aprimoramento
- Test: component/integration — promises controladas, falhas, clique rápido, troca de aba durante save e respostas fora de ordem; sem perfil pessoal.

### Requirement: Credenciais ficam na fronteira correta
O sistema SHALL preservar a possibilidade de credencial própria do usuário e configuração do serviço para os provedores substitutos, sem copiar chave Venice, expor segredos no DTO/log ou transmitir chave a provedor não selecionado.

#### Scenario: Chave própria e chave do serviço
- **WHEN** existe uma credencial própria para o provedor da ação ou somente a configuração do serviço
- **THEN** a ação usa a credencial apropriada e seu funding pertinente sem fallback entre provedores; a UI só expõe estado configurado e a ausência bloqueia a operação com recuperação clara
- Test: integration — seleção de credential/funding e isolamento entre áudio/texto/web com stores de teste, incluindo falta de chave e rejeição.

### Requirement: Defaults e catálogos consistentes
O sistema SHALL oferecer defaults e catálogos válidos por ação, sem modelo salvo que seja apresentado como disponível quando a rota necessária não está configurada.

#### Scenario: Dev sem chaves
- **WHEN** um dev sem as chaves necessárias é iniciado
- **THEN** permissões concedidas continuam reconhecidas, mas a configuração ausente é indicada separadamente e nenhuma transcrição, geração ou busca é fingida como funcional
- Test: integration — inicialização/listagem/erro sem provider key e UI nativa; setup real é prerequisito explícito do aceite externo.
