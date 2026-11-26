# Spec Delta

## REMOVED Requirements

### Requirement: Ferramentas de leitura e mídia sem broker
**Reason**: O requisito misturava leituras preservadas com geração de mídia retirada pelo dono. O cenário de geração deixa de ser aplicável.
**Migration**: Leituras continuam no requisito Ferramentas de leitura sem broker abaixo; clientes de mídia recebem ferramenta indisponível sem efeito/custo. Histórico e arquivos não são apagados.

## MODIFIED Requirements

### Requirement: Ferramentas para agentes externos
O relay SHALL reutilizar os handlers e a validação do catálogo do Clovy para todas as ferramentas aplicáveis a um agente externo. O host MUST fornecer contexto isolado para ferramentas que dele precisem, anunciar apenas ferramentas realmente habilitadas e nunca vincular chamadas externas a conversas internas por ID fornecido pelo cliente. Extração de dados, alterações, conectores e automação devem continuar sujeitos aos escopos, permissões e pré-condições do produto.

#### Scenario: Descoberta e chamada sem chat
- **WHEN** o agente externo chama as ferramentas anunciadas através do relay
- **THEN** recebe resultados no cliente externo, com aprovação no Clovy quando aplicável, sem abrir ou criar conversa no app

### Requirement: Catálogo de paridade com nomes fixos
O servidor SHALL oferecer a turnos vinculados as ferramentas de leitura e alteração do catálogo vigente, com os mesmos argumentos, limites, gates e erros das ferramentas nativas equivalentes, salvo onde indicado. Nomes MUST ser exatamente os das tabelas; este delta é o dono deles.

#### Scenario: Paridade de leitura paginada
- **WHEN** um turno vinculado lê uma nota com conteúdo maior que uma página e um ditado longo por id
- **THEN** recebe páginas sucessivas com `nextContentOffset` e `nextTextOffset` até o fim, sem perder caracteres
- Test: unit — banco de teste com textos multibyte longos, comparando a concatenação com o original

## ADDED Requirements

### Requirement: Ferramentas de leitura sem broker
As ferramentas get_dictation, get_note e recording_status SHALL executar sem aprovação do broker, preservando paginação, retenção e argumentos existentes. A retirada de geração de mídia MUST NOT retirar essas leituras.

#### Scenario: Leitura sem geração
- **WHEN** um cliente autorizado lê um ditado ou nota longa e consulta o gravador
- **THEN** recebe as páginas e o estado com os limites existentes, sem aprovação de alteração ou chamada a geração de mídia

### Requirement: Geração de mídia retirada do MCP
O servidor MUST NOT anunciar nem executar generate_image, edit_image ou generate_video. Chamadas antigas SHALL falhar como ferramenta indisponível antes de requisição, cobrança ou geração de artefato. Demais ferramentas e suas fronteiras de autorização SHALL permanecer disponíveis.

#### Scenario: Cliente externo com catálogo antigo
- **WHEN** um cliente chama um dos três nomes de geração retirados
- **THEN** recebe erro de ferramenta indisponível sem requisição externa ou alteração de dados, enquanto consultas de notas, rotinas e integrações continuam operantes
