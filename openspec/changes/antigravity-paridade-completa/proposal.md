# Proposal

## Why

O Clovy deve oferecer o mesmo alcance de suas ferramentas quando o usuário possui apenas Claude Code, apenas Codex ou apenas Antigravity, e ao alternar entre os três numa conversa. O app é a autoridade e administra o contexto, o gateway, as conexões e as permissões; o runtime escolhido é apenas o motor de execução.

## What Changes

- Estabelecer um contrato comum de paridade para os três runtimes: chat principal e da nota, contexto referenciado, catálogo de ferramentas do Clovy e dos conectores autorizados, aprovações/clarificações, cancelamento, continuidade, rotinas e histórico.
- Reutilizar o gateway host-owned existente como pool único de ferramentas, sem catálogo, credenciais ou regras de autorização paralelos por runtime. Cada adaptador entrega ao motor o vínculo do turno e normaliza os eventos reais.
- Garantir operação com qualquer um dos três instalado isoladamente, respeitando a escolha explícita quando há mais de um. A preferência automática existente só resolve a ausência de escolha; não pode substituir a decisão do usuário.
- Tornar a troca de motor transparente no alcance autorizado e no contexto persistido, sem reutilizar um identificador de conversa incompatível, ressuscitar contexto antigo, duplicar efeitos ou carregar aprovações pendentes de outro turno.
- Completar a conexão MCP do Antigravity por configuração temporária do workspace, com vínculo de turno, nome único, timeout acima do broker e revogação no encerramento; sem editar configuração global nem copiar credenciais.
- Definir e provar a política explícita do Antigravity para turnos assistidos, rotinas e geração pontual; não converter `--sandbox` em prova de contenção nem liberar bypass para obter paridade.
- Qualificar os recursos de texto com schema e proteção contra autoingestão, distinguindo o runtime escolhido da leitura de suas transcrições. Atualizar documentação e aceites nos dois idiomas com prova nativa e matriz dos três motores.

## Capabilities

### New Capabilities

Nenhuma: a mudança completa capacidades existentes.

### Modified Capabilities

- `llm-providers`: runtime como motor intercambiável, operação isolada com qualquer um dos três, alternância transparente e gateway comum.
- `cli-engine-permissions`: mesma política host-owned para as ferramentas do Clovy, política explícita do Antigravity, rotinas e ciclo de vida da configuração temporária.

## Impact

Áreas existentes: `src-tauri/src/chat_engine/`, `llm/`, `providers/mod.rs`, `routines.rs`, `src/lib/chat-engine.ts`, seletor do agente e testes comportamentais próximos. Reutilizar `McpLaunch`, `TurnRegistry`, `TurnGrant`, broker e transporte nativo; não criar API pública de autorização paralela.

A referência oficial atual admite `<workspace>/.agents/mcp_config.json`; o texto do repositório que atribui somente configuração global ao Antigravity está desatualizado. O comando local também oferece `--settings` transitório, mas isso não comprova suporte a MCP dentro desse JSON nem exclusão de hooks/plugins.

Fora do escopo: substituir ASR Local/Groq, gerar áudio/imagens/vídeos diretamente com um CLI de texto, mudar a limpeza de ditado OpenRouter-only para todos os provedores, replicar ferramentas proprietárias internas de um CLI nos outros, alterar credenciais/perfis globais ou liberar ações sem autorização. Acesso a funções de mídia já oferecidas como ferramentas do Clovy faz parte do pool comum, com os mesmos pré-requisitos do provedor independentemente do motor. Windows exige prova no Windows, não compilação cruzada. A correção Stop/migração56 em qualificação é base separada, não será reimplementada aqui.
