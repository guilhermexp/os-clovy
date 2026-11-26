# Proposal

## Why

O dono determinou a retirada completa da dependência de Venice, preservando as capacidades ainda vigentes por provedores substitutos; geração de mídia e superfícies do chat foram retiradas pelo dono em 2026-11-26. No dev real, Fn já abre o componente e produz áudio, mas a transcrição é recusada com `clovy_request_failed / model_not_priced`: a API local carregou zero modelos após filtrar provedores sem chaves configuradas.

## What Changes

- **BREAKING**: remover implementações, configuração, seleção, IPC/DTOs, credenciais/header, aliases, catálogos, branding e fallbacks ativos de Venice; migrar todos os consumidores, sem shims. Histórico e ADRs aceitos continuam históricos, não dependências ativas.
- O escopo de transcrição de áudio foi substituído em 2026-11-25 pela change `selecionar-modelos-transcricao`, autorizada pelo dono: Groq, OpenRouter e quatro modelos locais. Esta change conserva somente as demais capacidades do corte de provedores.
- Correção delimitada autorizada em 2026-11-26: tornar a preferência existente de aprimoramento do ditado visível em Modelos > Transcrição de áudio, preservar sua escolha e corrigir carregamento, salvamento e respostas concorrentes. OFF continua sem LLM; ON usa o modelo próprio do ditado no OpenRouter. A correção não aceita nem executa as tarefas anteriores deste plano.
- Diferenciar falha de Acessibilidade da indisponibilidade do destino de colagem, com recuperação pela ação existente e metadados técnicos mínimos, sem enfraquecer os gates de foco/permissão ou alterar a transcrição selecionada.
- Usar OpenRouter para texto remoto: geração de notas, limpeza do ditado e consumidores de inferência textual; preservar motores CLI/endpoints já existentes e os usos CLI sobreviventes do dono.
- Usar Tavily para busca web e extração de páginas, preservando os resultados consumidos pelas ferramentas existentes.
- Migrar seleções legadas e overrides para modelos válidos dos novos catálogos, sem reescrever notas, conversas, transcrições ou histórico de providers. Chaves Venice não se tornam chaves dos novos serviços.
- Declarar configuração necessária, preços/capacidades/privacidade reais e erros úteis quando faltar chave/modelo; não afirmar TEE/zero retention dos antigos serviços sem suporte dos novos.

## Capabilities

### New Capabilities

- `remote-text-inference`: inferência de texto OpenRouter, catálogo/capacidades/preço e continuidade das rotas CLI/endpoints.
- `web-augmentation`: busca e extração Tavily com resultados, erros e limites das ferramentas existentes.
- `inference-provider-settings`: seleção, configuração e migração limpa dos provedores sem dependência ativa de Venice.

### Modified Capabilities

`media-generation`, `clovy-mcp-server`, `desktop-navigation-without-chat` e `llm-providers` recebem deltas pela retirada de mídia e configurações do antigo chat. Integrações, sua configuração e MCP continuam disponíveis. Neste plano, a transcrição vigente é regida pelas specs canônicas `local-transcription` e `llm-providers`, com delta em `selecionar-modelos-transcricao`. As changes anteriores de captura/MCP e permissões dev continuam abertas e seus critérios não são reduzidos por este corte.

## Impact

- `clovy-api`: domínio/configuração, adapters, bootstrap, catálogo/pricing/metering e handlers/services de áudio, texto e web; remoção das implementações exclusivas de geração de mídia; exemplos de ambiente e contratos HTTP/testes.
- Desktop Rust: `providers`, `clovy_api`, ditado, processamento de notas, registro LLM, comandos/DTOs e seleção por perfil; helper de áudio perde o rótulo legado `venice_recording`, sem alteração indevida de permissões.
- Frontend: settings, pickers/catálogos, funding/privacy/branding, notas/rotinas e callsites companion; retirada de superfícies obsoletas de chat/agente; atualizar todos os consumidores dos símbolos exportados.
- Documentação vigente e recipes de comprovação; ADRs aceitos recebem addenda, não reescrita histórica.
- Serviços externos escolhidos pelo dono: [Groq](https://console.groq.com/docs/speech-to-text), [OpenRouter](https://openrouter.ai/docs/api_reference/overview) e [Tavily](https://docs.tavily.com/documentation/api-reference/introduction). Provas externas reais exigem chaves configuradas localmente e ação/autorização de custo do dono; contrato em sandbox não equivale a execução paga nem a aceite completo.
- O panic observado da captura por comparação sem ordem total é uma correção independente necessária ao fluxo real; não será mascarado nem atribuído a uma falha de TCC.

## Correção do Ditado de 2026-11-26

O pedido atual corrige a descoberta e a integridade de um controle já exigido pela spec canônica `dictation`; não torna aprimoramento obrigatório, não altera defaults/migrações nem executa inferência paga. As capturas e os eventos técnicos da instalação 0.1.8 localizaram sete falhas em `accessibility_permission_missing`, depois de texto gerado, sem evidência de falha do Parakeet. A concessão de Acessibilidade permanece uma decisão do usuário no macOS. A correção do Ditado fica delimitada às tarefas 6.1–6.7; a limpeza adicional abaixo fica em 7.x. As outras tarefas mantêm seu estado anterior.

## Limpeza autorizada de 2026-11-26

O dono retirou completamente geração/edição de imagem e geração/animação de vídeo, incluindo comandos, provedores fal, catálogos e as três ferramentas de mídia do MCP. Retiram-se a página Agente e opções exclusivas do chat, preservando detecção/execução/autenticação CLI para notas, Atividade e rotinas. A última correção explícita mantém integrações/conectores/custom MCP e seus controles de configuração; Plugins permanece como gerenciamento das integrações. Skills e sons de rotinas sobrevivem, com o controle de sons realocado para Geral. História, assets, perfis e credenciais existentes não são apagados. Vídeo de reunião, imagens de aparência e leitura de arquivos não são geração de mídia.

As antigas tarefas 2.1, 2.2 e 5.3 e a delta generated-media foram retiradas, não aceitas como concluídas. Esta rodada executa apenas 7.x depois do Ditado 6.x, em nova sessão na box. Nenhuma nova release está autorizada para estes ajustes; sem instalar/reiniciar o app do dono.
