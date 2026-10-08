# Design

## Context

Ver `proposal.md` e as cinco specs. O desktop grava áudio local e usa os endpoints Clovy `/v1/dictate` e `/v1/notes/transcribe`; o retorno atual contém texto, idioma opcional e provider, não segmentos. `ProviderModelSettings` também coordena os motores CLI/endpoints, embora nomes Venice apareçam em seletores genéricos. No backend, `OpenAiTranscriber` contém multipart utilizável, enquanto texto, imagem, vídeo e web só têm adapters Venice de produção. Catálogo/preço vivo e BYOK são igualmente Venice.

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

Manter os endpoints Clovy e os traits dos serviços. O multipart ASR existente passa a um adapter Groq verdadeiro, com identificação `groq` e base `https://api.groq.com/openai/v1`; não usar um client chamado OpenAI apontando para Groq como shim. Todos os novos áudios usam Whisper Groq, com `whisper-large-v3-turbo` padrão e `whisper-large-v3` quando a seleção válida requer a variante de qualidade. Preservar o contrato de texto/idioma, prompt/contexto autorizado, bytes/formato/filename canônico e boundaries existentes; Groq não recebe título da nota. Limites do prompt Groq e erros devem ser demonstrados, não silenciosamente mascarados.

Texto remoto passa a OpenRouter por `/api/v1/chat/completions` e catálogo `/api/v1/models`, mantendo JSON/SSE, tool calls, uso/cobrança, geração de nota e cleanup. Capacidades vêm de modalidades/supported_parameters reais e preços do catálogo, não de nome/traits ou listas Venice. A seleção automática do Clovy continua comportamento do Clovy e resolve um modelo concreto compatível; não virar um alias que suponha preço/capacidade de um modelo terceiro de roteamento. Defaults concretos devem ser conferidos no catálogo público atual, sem adivinhar novos IDs.

fal implementa os traits de imagem, edição e vídeo com endpoints oficiais próprios de cada ação, submit/status/result e download final limitado. Manter o padrão atual de catálogo curado de mídia, substituindo IDs/provider/metadata pelos endpoints fal efetivamente suportados e alinhando allowlists/preço do backend; não expandir `ModelKind` áudio/texto só para unificar estruturas distintas. Escolher defaults reais que cubram gerar/editar imagem, gerar vídeo e animar imagem. Input de imagem usa mecanismo permitido por fal sem introduzir upload público lateral. A preferência de segurança só é oferecida quando o endpoint tem semântica real correspondente. Jobs preservam identidade/estado/erro; completed com erro ou sem asset não é sucesso.

Tavily substitui web search/fetch por `/search` e `/extract`, normalizando successful/failed results no retorno existente. Remover seletores/enum Brave/Google que só indicavam o aggregator Venice e migrar todos os callers para Tavily; histórico de ações antigas não é shim de execução. Preservar proteções de URL, limites, erro e metering, sem crawlers ou research APIs adicionais.

Alternativa descartada: apenas mudar base_url da OpenAI/renomear labels Venice; deixaria identidade, credenciais, catálogo e capacidades falsas. Também não serão removidas funções exclusivas: o dono escolheu preservá-las com substitutos reais.

### Configuração, credenciais e financiamento

`UpstreamsConfig` terá configuração explícita de Groq, OpenRouter, fal e Tavily com defaults oficiais, placeholders detectáveis e exemplos de ambiente. Nenhuma chave Venice é válida para outro serviço. Preservar o mecanismo de credencial própria do usuário e chave de serviço, com isolamento por capacidade/modelo: áudio→Groq; texto→OpenRouter; imagem/vídeo→fal; web→Tavily. O cliente só encaminha a credencial correspondente e o backend confere o alvo autorizado antes de usar credencial ou decidir funding; nunca mandar todas as chaves em uma chamada.

O backend integra os tipos privados de credencial e headers próprios no estilo existente, eliminando `x-venice-api-key` e todos os métodos/branches Venice. DTOs públicos mostram somente estado configurado por provedor. O desktop reutiliza o store canônico de segredos dos providers/endpoints para novos segredos, com contas distintas, não cria outro JSON plaintext. Provisionamento acontece pelo fluxo normal do usuário/app; testes usam stores isolados. Main/worker não acessam/modificam o Keychain real e não transferem segredo legado. O código do corte não pode regravar segredos de endpoints existentes.

Catálogo/pricing deve reconhecer modelo válido no fluxo de chave própria e no de chave de serviço; ausência de chave não é autorização para devolver sucesso nem para repetir TCC. Preservar política de cobrança já existente: operação com chave própria válida não cobra inferência do Clovy como se a chave fosse do serviço; operações com chave de serviço mantêm autorização/hold/settlement/error, usando rates reais. fal deixa de depender de quote Venice: rates/estimativas ficam vinculados a endpoints/opções reais e têm origem documentada. Preço variável que não pode ser estimado não ganha número fabricado.

### Migração limpa de settings e superfícies

Renomear APIs/DTOs genéricos (`list/set provider model`, `ProviderModelDto`) em todos os consumidores; `set_groq_model` não serve para chat/imagem/vídeo. Remover os comandos, aliases, branding e i18n Venice antigos em vez de reexportá-los. References de símbolos exportados antes de editar; atualizar Rust IPC registration, frontend, agente, note-chat, funding/privacy, companion e testes juntos.

Na carga/salvamento normal, converter modelos/provedores legados de áudio para Whisper/Groq válido; texto remoto para OpenRouter válido; mídia para catálogo fal válido, incluindo profile overrides. Preservar `llmUsage`, CLI/endpoints, níveis, opções independentes e conteúdos. O chat Claude/atividade Claude do dono permanecem CLI; notas e dictationCleanup que estavam `kind=clovy` continuam remotos, agora OpenRouter. Não converter esses usos para Claude sem decisão do dono. Registros históricos podem manter provider `venice` como história; não reescrever transcrições/notas. Campos de credencial Venice saem do schema ativo, sem copiar valor para novo segredo e sem escrita direta do Main no arquivo real.

Atualizar docs vigentes/config examples e registrar o corte em addenda de ADRs aceitos, preservando corpo histórico. Busca exaustiva deve distinguir código/config/docs vigentes de ADRs históricos, banco do dono, recibos e graph gerado; refresh do graph após a mudança grande.

### Ownership e sequência de integração

Main detém OpenSpec, ticket, integração e prova nativa. Implementação é delegada ao ciclo autorizado; o worker retido não tem nova autorização para editar antes do brief/preflight. Slices reais: (1) backend integração/domínio/config/serviços/HTTP e adapters Groq/OpenRouter; (2) adapters fal/Tavily com suas provas, em arquivos owned distintos; (3) desktop/UI/registro/migração/consumidores. Um único dono de integração do backend assina os tipos compartilhados e o bootstrap; siblings não editam os mesmos arquivos. Compartilhar o checkout é intencional se houver novos workers, sem criar branch/worktree não autorizada. Gates/build/Cargo são serializados e executados depois da onda, nunca em concorrência.

Panic OCR é fix separado, no arquivo `src-tauri/src/activity/macos/ocr.rs`, com regressão de ordem de leitura. A comparação pairwise com tolerância admite ciclos. Ordenação total vertical seguida de linhas ancoradas e ordenação horizontal total em slices elimina a causa sem catch/unwind, validação arbitrária ou cópias/alocações adicionais de observações. Provar permutations do caso de alturas0.1/y.90,.86,.82/x.3,.2,.1 e contrato existing, depois captura real contínua. Sem reexecutar a tentativa do dono só para confirmar seu relato.

## Risks / Trade-offs

- [As novas APIs custam dinheiro e precisam de chaves] → stores/config locais e boundary smoke sem chamadas pagas; dono faz/autoriza prova externa. Sem chave/prova real, o critério permanece pendente, não PASS global.
- [OpenRouter/fal/Tavily não herdam o TEE/zero retention Venice] → remover claims/headers legados e exibir política real; model capabilities/cost não se inferem por branding.
- [Protocolos de fila/preço fal variam por modelo] → defaults/endpoints documentados e modelos efetivamente implementados; validar gerar/editar/animar separadamente até o asset, sem reaproveitar contrato errado.
- [Contrato Groq de contexto/idioma difere do Venice] → provas dos boundaries e limites oficiais com erros úteis; sem enviar dados adicionais alheios ao áudio.
- [Migração pode apagar escolhas/dados] → testes consumer-visible em cópia sandbox, preservar mapa inteiro e CLI/endpoints; nunca reescrever perfil normal como fixture.
- [Produção/gate reescrevem helpers em .tauri-helper] → um slot Cargo; depois reconstruir/preparar dev com o certificado já selecionado e observar grants/startup reais. Não atribuir o erro ASR model_not_priced a assinatura.
- [Timeout de teste anterior não é flake comprovado] → manter recibo vermelho e investigar somente se continuar relevante à fonte final, sem retentar até verde nem aumentar timeout para encobrir regressão.

## Migration Plan

1. Main fecha artefatos/contratos e verifica o plano canônico e ownership; evidencia nova falha nativa e decisões do dono.
2. Worker(s) implementam corte completo com regressões comportamentais e mocks somente em sandbox/testes; nenhum app do dono ou serviço pago é acionado pelo worker.
3. Main integra os callsites, confere todos os cenários, serializa gate/build no fingerprint final e audita recibos sem reutilizar os de00c para a fonte nova.
4. Restaurar/preparar o dev normal com identidade estável, abrir janela real e conferir grants/helper/captura/MCP/restart/HMR. Provas com fixtures vão a perfil/cópia isolada, jamais o perfil normal.
5. Dono configura chaves localmente pelo fluxo normal; prova externa real de áudio, texto, imagem/edição, vídeo/animação e search/extract exige autorização de custo explícita ou ação do dono. Main não coleta segredos no chat nem alega execução não observada.
6. Review proporcional, verificador independente em sessão distinta, closeout/verify --record e aceite completos; publicação somente se autorizada. Rollback é decisão/integracão explícita de commits, nunca shim/fallback Venice nem restauração automática de segredo/TCC.
