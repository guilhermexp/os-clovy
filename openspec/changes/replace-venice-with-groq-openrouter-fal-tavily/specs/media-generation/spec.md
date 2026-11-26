# Spec Delta

## REMOVED Requirements

### Requirement: Mídia direto na fal com chave própria
**Reason**: O dono retirou completamente geração e edição de imagens, geração e animação de vídeos do Clovy após retirar o chat interno.
**Migration**: As ações e catálogos de geração deixam de existir, inclusive no MCP. Arquivos, histórico e credenciais previamente armazenados não são apagados. Captura e análise de vídeo de reunião continuam disponíveis.

### Requirement: Mídia sem chave fal
**Reason**: A capacidade de gerar mídia foi retirada; configurar uma chave fal não deve reativá-la.
**Migration**: Retirar comandos, configuração ativa e dependências exclusivas de geração sem remover segredos reais ou a configuração dos provedores sobreviventes.
