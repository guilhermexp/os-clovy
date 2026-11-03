# Design

## Context

Ver `proposal.md` e os dois deltas. O usuário confirmou que o app administra tudo e os três runtimes são motores intercambiáveis do mesmo pool de ferramentas, não integrações com alcance diferente.

O gateway, catálogo, partições, `TurnRegistry`, `TurnGrant`, broker, persistência e os adaptadores já existem. Agy está excluído em `chat_engine::cli::accepts_mcp`; `TurnPolicy` não recebe política Agy. A seleção padrão e a elegibilidade de rotinas compartilham a lista Claude/Codex. `previous_conversation` já impede retomar um ID de outro motor ou um ID anterior a uma troca; o replay atual inclui mensagens, steering e resumos, mas não resultados de ferramentas. Esse comportamento precisa ser qualificado, não substituído por uma conversa paralela.

Prova local: `agy models` respondeu; um turno real com Gemini 3.8 Flash Medium respondeu `AGY_QA_OK`, sem eventos de ferramenta e sem alterar settings. Isso não prova integração MCP nem contenção. Referências oficiais consultadas em 2026-11-03: [MCP](https://antigravity.google/docs/mcp?tab=cli), [settings](https://antigravity.google/docs/settings?tab=cli), [headless](https://antigravity.google/docs/cli/headless/) e [sandbox](https://antigravity.google/docs/sandbox/). Workspace MCP existe; `--settings` é transitório; `--sandbox` pode não impor contenção onde indisponível. Hooks possuem arquivo separado, não são desativados por uma chave ignorada de settings.

## Goals / Non-Goals

**Goals:** mesma autoridade e mesmo catálogo do Clovy para Claude, Codex e Antigravity; funcionamento com apenas um instalado; alternância com continuidade do contexto autorizado; rotinas e geração pontual seguras; nenhum efeito autorizado pelo simples fato de trocar runtime.

**Non-Goals:** novo agente nativo, loop de ferramentas proprietário, catálogo por runtime, proxy de modelo, cópia de credenciais, alteração de configuração global, paridade dos comandos internos proprietários de cada CLI ou motores novos de áudio/mídia. Ferramentas de mídia/conectores existentes permanecem no pool com os mesmos pré-requisitos host-owned.

## Decisions

### D1. Um gateway existente, adaptadores somente de transporte e eventos

Reutilizar o `McpLaunch` do turno e os mesmos handlers/catalogadores; o adaptador recebe um vínculo novo e entrega a conexão ao runtime. Ferramenta não pode desaparecer por uma whitelist específica do motor; diferenças legítimas são somente superfície, partição, política e pré-requisitos do app. Paridade será medida comparando o catálogo normalizado e exercitando leituras, escrita aprovada/negada, clarificação e cancelamento nos três motores. Não criar outro registro de ferramentas nem reencaminhar a chamada por Claude/Codex.

Alternativas descartadas: configuração global com `agy mcp add` interfere em sessões e credenciais do dono; implementar um loop próprio duplica a autoridade que já pertence ao Clovy. A única abordagem proposta é completar o adaptador existente.

### D2. Configuração Agy temporária, vinculada e preservável

Usar `<workspace>/.agents/mcp_config.json` com servidor de nome único por turno, argumentos existentes `--dir`/`--turn` e `timeoutSeconds:900`, acima dos 600 segundos do broker. Nome não é autoridade: o token continua validado e revogado pelo host.

O workspace pode ser informado pela API; não presumir propriedade exclusiva. A implementação terá uma guarda específica da configuração, com lease por workspace durante o turno, preservação da unidade completa antes da primeira escrita, escrita atômica de apenas sua entrada e remoção por identidade ao terminar. A lease serializa somente turnos Clovy que disputam esse mesmo arquivo, não projetos, builds ou a máquina. Não substituir symlinks nem alterar permissões de diretórios alheios. Ao restaurar, reler o arquivo e preservar entradas concorrentes; remover somente a entrada ainda idêntica à criada. Se o estado não permitir restauração segura, revogar o token, manter recibo/estado para recuperação e falhar explicitamente, nunca apagar o arquivo alheio. Arquivos de segredo temporários devem ser privados e logs não podem registrar tokens.

A guarda deve cobrir erro antes/depois do spawn, Stop, retorno normal e crash. A retomada Agy precisa carregar o novo servidor sem reutilizar a autoridade anterior. Verificar isso com CLI real antes de considerar o transporte entregue; documentação isolada não basta.

### D3. Política do app, não a configuração permissiva do runtime

Adicionar a linha Agy à tabela existente. Usar `--settings` transitório para fixar explicitamente modo e regras; permitir apenas o MCP do gateway vinculado nas exceções MCP do turno. A autorização de escrita continua exclusivamente no broker, inclusive em Bypass configurado pelo usuário. Não usar `--dangerously-skip-permissions`, `always-proceed`, `auto` ou configuração global para fazer testes passarem.

Rotinas usam a política não assistida existente e suas capacidades de leitura; a escrita que exige aprovação é recusada sem cartão pendente. Antes de admitir Agy em rotinas, uma prova nativa deve verificar precedência das regras sobre permissões globais, negação de escrita/comando/atuação não autorizados e ausência de passagem por extensões/hooks fora da política. Geração pontual recebe a mesma verificação de isolamento, schema e efeitos, sem presumir que a flag de sandbox desliga ferramentas. Uma limitação comprovada do CLI é bloqueio da entrega de paridade, não permissão para declarar um modo reduzido como completo; deve voltar ao usuário com evidência e alternativa concreta.

### D4. Seleção e contexto são independentes de elegibilidade

Separar a preferência padrão de chat da admissão não assistida; só incluir Agy nesta última depois da política qualificada. Respeitar o runtime escolhido e não trocar de motor em erro. Incluir Agy na descoberta/default do backend e do frontend; não mudar as rotas independentes de transcrição ou ditado.

Manter IDs de retomada restritos à sequência contínua do mesmo motor. Ao alternar, construir o contexto a partir da história canônica e das referências autorizadas do Clovy, incluindo evidência de ferramentas necessária para continuar, sem repetir suas ações nem restaurar tentativas rejeitadas. Reutilizar orçamento/resumos e leitores de contexto existentes; não criar um segundo sistema de memória nem prometer respostas idênticas entre modelos. Testar as seis trocas direcionadas e retorno A→B→A, com resultados e referências, não apenas uma frase ecoada.

### D5. Normalização e prova reais

Reaproveitar a correção Stop/migração56 em qualificação como base. O parser Agy deve preservar argumentos/outputs/erros reais, estado terminal e resposta final mesmo sem deltas intermediários, sem spinner permanente nem sucesso vazio. Usar fixtures sanitizadas do protocolo real e testar a mesma projeção/hidratação comum; não fabricar resultado para acomodar diferenças do CLI.

Provas de geração pontual cobrem schema efetivo, timeout, ausência de efeitos e não autoingestão. A leitura de sessões é opção separada: o toggle que o usuário ligou não configura o provedor. Não usar transcrições pessoais como fixtures. Prova nativa no Mac usa o bundle/perfil sintético existente e entradas próprias; Windows exige host real disponível.

## Risks / Trade-offs

- Configuração local compartilhada e processos Agy persistentes → lease específica, nome único, teste de retomada e token revogado; nunca confiar só na prioridade do nome.
- CLI novo aceita uma chave desconhecida e a ignora → observar configuração/negação efetivas em canários nativos, não aceitar argv como prova.
- Rotina herda permissões, hooks ou MCPs globais → qualificar isolamento e impedir admissão quando não houver garantia; não alterar os arquivos do dono.
- Troca de motor perde resultados anteriores ou repete ação → contexto canônico com evidências, nenhum replay de execução e regressão de efeito único.
- Cota/auth/Windows indisponível → registrar o runtime e a prova que faltam, sem atribuir ao resultado sintético aprovação nativa integral.

## Migration Plan

Sem nova migração de banco presumida. Preservar escolhas e histórico existentes; adicionar somente a resolução necessária de Antigravity quando não há escolha explícita. Atualizar documentação obsoleta sobre MCP global e separar default de suporte a rotinas. Atualizar o contexto durável do projeto somente depois de implementar.

Integrar após RED/GREEN, revisão final completa e scanner; revisão automática já indisponível usa contingência manual registrada, nunca aprovação fictícia. Build assinado e smoke no Mac devem usar a fonte integrada. Rollback de código não permite abrir o perfil sintético schema56 em binário737; preservar backup integral. Publicação na main sem force é autorizada; release/deploy e instalação sobre o app real não fazem parte desta change. Arquivar somente com todos os aceites; sincronizar AGENTS e fechar recursos próprios com recibos, preservando dados e provas.
