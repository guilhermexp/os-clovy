# Design

## Context

O código vigente em 28445275 já chama diretamente Groq/OpenRouter e transcrição Local, com motores CLI/endpoints compartilhados por notas, Atividade e rotinas. Geração fal e superfícies obsoletas de chat ainda existem e foram retiradas pelo dono em 2026-11-26. As descrições de falhas Venice abaixo são contexto histórico da origem deste plano, não diagnóstico da instalação atual.

Observação nativa preservada em `run-main-native-runtime-failures-20261008T161215Z/observations.json`: Fn produziu `recording_ready` e erro posterior `model_not_priced`; não é falha de TCC/atalho. O serviço dev posteriormente saiu0. Recibos do `00c896b0` são históricos, incluindo gate inicialmente vermelho por timeout e uma segunda rodada verde/PASS-UNPROVEN; não aceitam o novo corte nem o panic de captura.

## Goals / Non-Goals

**Goals:**
- Uma rota real por capacidade escolhida pelo dono, preservando contratos e consumidores sem convenção paralela.
- Separar chaves/modelos/preços/capacidades por provedor, inclusive credencial própria e configuração de serviço existentes, sem forwarding de chave entre provedores.
- Preservar dados, CLI/endpoints, overrides e comportamento de defaults/opt-outs da captura/MCP.
- Demonstrar contratos e erros em sandbox e reservar prova externa/native real para configuração e custo autorizados.

**Non-Goals:**
- Novo formato de transcrição com timestamps/segments, mecanismo de retry, telemetry, roteador/plugin genérico ou versão compatível Venice.
- Alterar Keychain do dono pelo Main/worker, ler fragmentos de chaves, desbloquear serviços, redefinir TCC ou usar o perfil normal como fixture.
- Deploy/push, release instalada/aberta, nova branch/worktree ou aceite implícito de prova parcial.

## Decisions

### Fronteiras existentes; adapters substituídos

Manter os endpoints Clovy e os traits dos serviços. O multipart ASR existente passa a um adapter Groq verdadeiro, com identificação `groq` e base `https://api.groq.com/openai/v1`; não usar um client chamado OpenAI apontando para Groq como shim. Esta decisão de áudio foi substituída pela change `selecionar-modelos-transcricao`: seleção explícita entre Groq, OpenRouter e quatro modelos locais; não existe mais obrigação de enviar todo áudio à Groq. Preservar o contrato de texto/idioma, prompt/contexto autorizado, bytes/formato/filename canônico e boundaries existentes; Groq não recebe título da nota. Limites do prompt Groq e erros devem ser demonstrados, não silenciosamente mascarados.

O hint ASR do desktop é distinto do contexto completo de correção: somente termos inteiros do dicionário, na ordem existente, separados por `, `, até 200 bytes UTF-8 incluindo delimitadores. Pular um termo que não cabe e considerar os seguintes; sem termo, omitir prompt. Esse envelope local conservador não é contador exato de 224 tokens nem prova da implementação interna Groq. Não cortar UTF-8/termos ou enviar instruções/cauda de transcrição como hint. O backend mantém o campo público `context` e o encaminha sem truncamento ou uma nova validação artificial de 200 bytes.

A correção existente de notas mantém contexto completo e os motores escolhidos, inclusive depois de ASR Groq; a decisão de remover esse passo foi vetada. O estilo remoto `note_transcript_cleanup` seleciona a instrução real de nota, não a de ditado. O aprimoramento do ditado é opcional e começa desligado, conforme a spec canônica `dictation`. A preferência salva do usuário prevalece. Desligado, o texto reconhecido segue sem chamada ao cleaner/LLM; ligado, mantém a única passada lógica existente de cleanup pelo modelo próprio do ditado no OpenRouter, preservando o bruto com aviso se falhar. O controle fica visível junto da seleção de transcrição em Modelos e compartilha estado com os detalhes em Ditado > Estilo. Trocar somente o modelo não pode ligar o aprimoramento. Não criar helper só de teste ou segunda passada para demonstrar o contrato.

Texto remoto passa a OpenRouter por `/api/v1/chat/completions` e catálogo `/api/v1/models`, mantendo JSON/SSE, tool calls, uso/cobrança, geração de nota e cleanup. Capacidades vêm de modalidades/supported_parameters reais e preços do catálogo, não de nome/traits ou listas Venice. A seleção automática do Clovy continua comportamento do Clovy e resolve um modelo concreto compatível; não virar um alias que suponha preço/capacidade de um modelo terceiro de roteamento. Defaults concretos devem ser conferidos no catálogo público atual, sem adivinhar novos IDs.

Cada POST Tavily é enviado uma vez; erros e limites permanecem explícitos. A geração de mídia fal foi retirada pelo dono em 2026-11-26, incluindo submit, poll e download de jobs.

Somente os clientes de upstream e inferência do bootstrap recusam redirects antes de enviar ao destino e desabilitam replay de protocolo do reqwest; erro de construção propaga em vez de devolver cliente sem essas policies. Preservar factories de OS Accounts, JWKS e issue reports, timeout/pool e clientes independentes ainda usados por outros recursos. A prova de 307/308 observa um único POST na origem e zero acessos ao destino; erro final isolado não prova ausência de reenvio.

Tavily substitui web search/fetch por `/search` e `/extract`, normalizando successful/failed results no retorno existente. Remover seletores/enum Brave/Google que só indicavam o aggregator Venice e migrar todos os callers para Tavily; histórico de ações antigas não é shim de execução. Preservar proteções de URL, limites, erro e metering, sem crawlers ou research APIs adicionais.

Alternativa descartada: apenas mudar base_url da OpenAI/renomear labels Venice; deixaria identidade, credenciais, catálogo e capacidades falsas. A preservação genérica de funções não se aplica à geração de mídia nem às opções exclusivas do chat, retiradas explicitamente em 2026-11-26.

### Configuração, credenciais e financiamento

`UpstreamsConfig` terá configuração explícita de Groq, OpenRouter e Tavily com defaults oficiais, placeholders detectáveis e exemplos de ambiente. Nenhuma chave Venice é válida para outro serviço. Preservar o mecanismo de credencial própria do usuário e chave de serviço, com isolamento por capacidade/modelo: áudio→provedor selecionado, conforme `selecionar-modelos-transcricao`; texto→OpenRouter; web→Tavily. O cliente só encaminha a credencial correspondente e o backend confere o alvo autorizado antes de usar credencial ou decidir funding; nunca mandar todas as chaves em uma chamada.

O backend integra os tipos privados de credencial e headers próprios no estilo existente, eliminando `x-venice-api-key` e todos os métodos/branches Venice. DTOs públicos mostram somente estado configurado por provedor. O desktop reutiliza o store canônico de segredos dos providers/endpoints para novos segredos, com contas distintas, não cria outro JSON plaintext. Provisionamento acontece pelo fluxo normal do usuário/app; testes usam stores isolados. Main/worker não acessam/modificam o Keychain real e não transferem segredo legado. O código do corte não pode regravar segredos de endpoints existentes.

Catálogo/pricing deve reconhecer modelo válido no fluxo de chave própria e no de chave de serviço; ausência de chave não é autorização para devolver sucesso nem para repetir TCC. Preservar política de cobrança já existente: operação com chave própria válida não cobra inferência do Clovy como se a chave fosse do serviço; operações com chave de serviço mantêm autorização/hold/settlement/error, usando rates reais. Preço variável que não pode ser estimado não ganha número fabricado.

### Migração limpa de settings e superfícies

Renomear APIs/DTOs genéricos (`list/set provider model`, `ProviderModelDto`) em todos os consumidores; `set_groq_model` não serve para os demais usos. Remover os comandos, aliases, branding e i18n Venice antigos em vez de reexportá-los. References de símbolos exportados antes de editar; atualizar Rust IPC registration, frontend, notas/rotinas, funding/privacy, companion e testes juntos.

Preservar paths não basta para preservar a fronteira HTTP pública `/v1`: seguir a política de compatibilidade vigente e confrontar fixtures históricas sem reescrevê-las para acomodar quebra. Tratar entrada histórica não autoriza executar Venice nem manter aliases de execução. Catálogos/pickers e companion usam disponibilidade de serviço e estado correto da chave própria, sem catálogo paralelo ou key-state fictício. OpenRouter deve cumprir parâmetros/capacidades na rota efetiva ou falhar utilmente, sem deduzir isso só de metadata agregada.

Na carga/salvamento normal, preservar a migração legada de áudio e as seleções válidas conforme `selecionar-modelos-transcricao`; texto remoto para OpenRouter válido; preferências legadas de mídia deixam de ser selecionáveis sem apagar assets ou segredos. Preservar `llmUsage`, CLI/endpoints, níveis, opções independentes e conteúdos. Os motores CLI de Atividade/notas/rotinas do dono permanecem disponíveis; notas e dictationCleanup que estavam `kind=clovy` continuam remotos, agora OpenRouter. Não converter esses usos para Claude sem decisão do dono. Registros históricos podem manter provider `venice` como história; não reescrever transcrições/notas. Campos de credencial Venice saem do schema ativo, sem copiar valor para novo segredo e sem escrita direta do Main no arquivo real.

Atualizar docs vigentes/config examples e registrar o corte em addenda de ADRs aceitos, preservando corpo histórico. Busca exaustiva deve distinguir código/config/docs vigentes de ADRs históricos, banco do dono, recibos e graph gerado; refresh do graph após a mudança grande.

### Ownership e sequência de integração

Main detém OpenSpec, ticket, integração e prova nativa. Implementação é delegada ao ciclo autorizado; o worker retido não tem nova autorização para editar antes do brief/preflight. Slices reais: (1) backend integração/domínio/config/serviços/HTTP e adapters Groq/OpenRouter; (2) adapter Tavily com suas provas, em arquivos owned distintos; (3) desktop/UI/registro/migração/consumidores. Um único dono de integração do backend assina os tipos compartilhados e o bootstrap; siblings não editam os mesmos arquivos. Compartilhar o checkout é intencional se houver novos workers, sem criar branch/worktree não autorizada. Gates/build/Cargo são serializados e executados depois da onda, nunca em concorrência.

Panic OCR é fix separado, no arquivo `src-tauri/src/activity/macos/ocr.rs`, com regressão de ordem de leitura. A comparação pairwise com tolerância admite ciclos. Ordenação total vertical seguida de linhas ancoradas e ordenação horizontal total em slices elimina o ciclo demonstrado sem catch/unwind, validação arbitrária ou cópias/alocações adicionais de observações; sem backtrace, a atribuição do panic nativo a esse comparador continua não provada. Provar permutations do caso de alturas0.1/y.90,.86,.82/x.3,.2,.1, tolerância estrita sem encadeamento, coordenadas empatadas/altura/texto e contrato existing, depois captura real contínua. Sem reexecutar a tentativa do dono só para confirmar seu relato.

## Risks / Trade-offs

- [As novas APIs custam dinheiro e precisam de chaves] → stores/config locais e boundary smoke sem chamadas pagas; dono faz/autoriza prova externa. Sem chave/prova real, o critério permanece pendente, não PASS global.
- [OpenRouter/Tavily não herdam o TEE/zero retention Venice] → remover claims/headers legados e exibir política real; model capabilities/cost não se inferem por branding.
- [Contrato Groq de contexto/idioma difere do Venice] → provas dos boundaries e limites oficiais com erros úteis; sem enviar dados adicionais alheios ao áudio.
- [Migração pode apagar escolhas/dados] → testes consumer-visible em cópia sandbox, preservar mapa inteiro e CLI/endpoints; nunca reescrever perfil normal como fixture.
- [Produção/gate reescrevem helpers em .tauri-helper] → um slot Cargo; depois reconstruir/preparar dev com o certificado já selecionado e observar grants/startup reais. Não atribuir o erro ASR model_not_priced a assinatura.
- [Timeout de teste anterior não é flake comprovado] → manter recibo vermelho e investigar somente se continuar relevante à fonte final, sem retentar até verde nem aumentar timeout para encobrir regressão.

## Migration Plan

1. Main fecha artefatos/contratos e verifica o plano canônico e ownership; evidencia nova falha nativa e decisões do dono.
2. Worker(s) implementam corte completo com regressões comportamentais e mocks somente em sandbox/testes; nenhum app do dono ou serviço pago é acionado pelo worker.
3. Main integra os callsites, confere todos os cenários, serializa gate/build no fingerprint final e audita recibos sem reutilizar os de00c para a fonte nova.
4. Restaurar/preparar o dev normal com identidade estável, abrir janela real e conferir grants/helper/captura/MCP/restart/HMR. Provas com fixtures vão a perfil/cópia isolada, jamais o perfil normal.
5. Dono configura chaves localmente pelo fluxo normal; prova externa real de áudio, texto e search/extract exige autorização de custo explícita ou ação do dono. Main não coleta segredos no chat nem alega execução não observada.
6. Review proporcional, verificador independente em sessão distinta, closeout/verify --record e aceite completos; publicação somente se autorizada. Rollback é decisão/integracão explícita de commits, nunca shim/fallback Venice nem restauração automática de segredo/TCC.

## Escopo transferido em 2026-11-25

A implementação e a prova de áudio de 1.2 e 5.1 passaram integralmente para `selecionar-modelos-transcricao`; as decisões históricas de Groq acima descrevem sua rota, não exclusividade. O restante deste plano não foi autorizado de novo nem considerado concluído. Contexto/dicionário, idioma, erros e preservação do áudio continuam contratos vigentes na nova change e nas specs canônicas.

## Correção delimitada de 2026-11-26

`AppSettings` já possui o snapshot nativo do Ditado e será a única fonte para as duas apresentações do controle. Estilo passa a receber props. Leitura desconhecida/falhando não representa OFF confirmado; durante salvamento, manter o último estado confirmado e bloquear mutações concorrentes por trava síncrona. Publicar somente campos pertencentes à ação: microfone, atalho correspondente, idioma, flag de cleanup, modelo, estilo e HUD. Isso impede que uma resposta antiga de idioma/microfone/atalho reverta visualmente a escolha de aprimoramento.

`set_dictation_cleanup` aceita `enabled` opcional de forma aditiva. O wrapper TypeScript antigo continua enviando booleano; a atualização só de modelo omite a flag. A transação usa o snapshot mais recente sob o lock existente e só publica a memória após o salvamento. Preservar defaults, `cleanupMigrated`, modelo/estilo salvos e rotas independentes de Texto/Notas. Esta correção não promete atomicidade contra escrita parcial no disco nem reforma o store; em erro de save a UI conserva o último estado confirmado e informa a falha, sem inventar defaults.

Para colagem, manter os gates de Acessibilidade, destino original e orçamento de ativação. As falhas observadas na instalação 0.1.8 foram AX missing após texto gerado; não trocar provedor por esse diagnóstico. Separar as mensagens de AX e destino com cópia curta que diga que o texto foi copiado. O HUD permanece não interativo e não ganha foco; instrução de permissão recebe prazo suficiente, substituída normalmente por nova tomada. Reutilizar a ação de permissão existente na janela principal para registrar o helper e abrir os ajustes de Acessibilidade, sem mudar grants ou editar TCC. Identificar o helper quando a permissão for específica dele.

Diagnóstico novo fica restrito ao estado de Acessibilidade conhecido e à correlação validada da tomada. Mapear `granted`/`missing` para verdadeiro/falso e estado ausente/desconhecido para desconhecido; o erro `accessibility_permission_missing` comprova falso naquele evento. Não copiar payload inteiro nem acrescentar texto, títulos, destino, caminhos ou credenciais. Campos sensíveis já existentes no logger não são declarados eliminados por esta mudança.

Prova da correção distingue testes e uso real: regressões de preferências/concorrência e dos dois erros, revisão visual isolada e CI nativo completo. A recuperação da colagem na instalação do usuário exige sua concessão no macOS e confirmação do texto no campo; não será substituída por smoke de quatro respostas de erro, por `paste_completed` isolado ou por permissão simulada.

## Fronteira da limpeza de configurações

Partir do commit de fonte revisado do Ditado, pois ambas as entregas tocam AppSettings, i18n e testes. Nova worktree/sessão nessa base exata; confirmar CI nativo do commit antes de editar a limpeza. A integração final na main depende do aceite das duas entregas e de seus checks. Nunca dois writers no mesmo checkout. Remover a UI de mídia e as rotas de geração fal, os três tools MCP correspondentes e comandos/bindings/DTOs exclusivos. Manter o leitor de histórico e artefatos; não remover diretórios do usuário, registros ou credenciais fal armazenadas. Retirar fal do catálogo/configuração ativos sem fazer limpeza de Keychain.

O servidor MCP próprio, broker, conectores/custom MCP e sua UI Plugins permanecem funcionais. CLIs, chat_engine e agent_sessions têm consumidores de rotinas e dados históricos; remover somente o seletor/callback de chat do frontend, não contratos compartilhados por nome. Remover navegação/página Agente e redirecionar destino legado para Geral; mover som de rotinas para Geral preservando a preferência. Skills carregadas por rotinas/MCP e suas configurações/backend permanecem. Corrigir linguagem antiga de agente dentro de Plugins sem cortar integrações.

Remoção de geração não inclui gravação de reunião, mac-video-capture, video_client, video_window_selection, migration039/video_artifacts ou imagens de aparência/ícones. Conferir catálogo bound/externo e dispatch recusando os três nomes retirados antes de qualquer efeito; provar também notas, Atividade, rotinas, skills, integrações, áudio e vídeo de reunião. Suites completas Linux na box e Rust/macOS GitHub, nunca builds no Mac do dono. Não publicar release destes ajustes até nova orientação explícita.
