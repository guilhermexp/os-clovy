# Tasks

Main atualiza checkboxes somente após confrontar a prova. Gates/build/Cargo são serializados; workers não executam checks durante edição concorrente da onda. Prova em sandbox/HTTP não conta como serviço externo/native real. Tasks externas ficam abertas sem chaves/ação ou autorização de custo do dono.

## 1. Contratos compartilhados e backend áudio/texto

- [ ] 1.1 Migrar domínio/config/headers/credenciais/funding para Groq, OpenRouter e Tavily, retirando branches Venice e exemplos antigos; verificar isolamento de credencial e cobrança própria/serviço com regressões comportamentais e docs/config atualizados, sem ler valores reais.
- [ ] 1.3 Implementar texto OpenRouter e catálogo/preços/capacidades reais para ferramentas, geração de notas e cleanup; verificar JSON/SSE/uso/erros e seleção compatível, documentando política de privacidade sem claims Venice.
- [ ] 1.4 Integrar services/HTTP/bootstrap/metering e novos adapters web após os contratos compartilhados; verificar contrato dos endpoints existentes e ausência de rotas Venice, mantendo todas as capacidades do pedido.

## 2. Web Tavily

- [ ] 2.3 Implementar Tavily search e extract nas ferramentas existentes, removendo o aggregator Venice; verificar resultados/failed-results/erro/credencial/URL e limits em sandbox, com docs de configuração e prova externa qualificadas.

## 3. Desktop, migração e superfícies

- [ ] 3.1 Migrar comandos/DTOs/registro IPC e todos os callsites de settings/modelos/funding/privacy sem aliases Venice; verificar references de exports, testes consumer-visible e os modos de áudio/texto ainda vigentes do picker, com docs dos novos nomes e interfaces.
- [ ] 3.2 Migrar settings globais e overrides legados para defaults/modelos válidos, preservando CLI/endpoints, segredos de endpoint e história; verificar leitura/save/restart em store/cópia sandbox e documentar o tratamento de seleção/credencial legadas sem transferir segredo Venice.
- [ ] 3.3 Integrar configuração/estado de credenciais dos provedores vigentes no store canônico e forwarding somente para a ação correspondente; verificar falta/rejeição de chave, metering/BYOK e DTO sem segredo com stores isolados, sem tocar Keychain real por Main/worker.
- [ ] 3.4 Atualizar catálogos/logos/i18n/sugestões/companion e os fluxos de ditado/notas/rotinas/web; verificar UI efetiva com dados sandbox e remover caminhos ativos Venice sem reescrever ADRs e histórico aceitos.

## 4. Captura estável e integração no dev

- [ ] 4.1 Corrigir o comparador OCR que admite ciclos com tolerância pairwise, mantendo ordem de leitura top-to-bottom/left-to-right sem suppress/catch; obter RED de ordem de leitura e GREEN de permutations/linhas ancoradas, atualizar doc comment e recipe de comprovação da captura contínua.
- [ ] 4.2 Main confrontar fontes/reports/review, executar busca exaustiva qualificada de dependências Venice e refresh do Graft; verificar cada cenário sem usar ausência de string como teste permanente ou apagar história.
- [ ] 4.3 Integrar o escopo completo em main, capturar gate/build/verify-cli-worker final no mesmo fingerprint e validar as três changes OpenSpec e Test stamps; preservar falhas históricas e não declarar PASS-UNPROVEN como aceite.
- [ ] 4.4 Restaurar preparo dev/certificado/ids estáveis após produção/gate e abrir dev normal para o dono; observar janela, helper, grants, continuidade da captura/MCP, restart/rebuild/HMR e skills em perfil vazio sem fixture no perfil normal.

## 5. Prova externa completa e aceite

- [ ] 5.2 Executar com ação/autorização de custo do dono a prova real OpenRouter de ferramentas, geração de nota e cleanup remoto; verificar outputs/estado/persistência e que CLI/endpoints preservados continuam funcionando.
- [ ] 5.4 Executar com ação/autorização de custo do dono search/extract Tavily e observar os retornos reais pelas ferramentas, sem mascarar ausências de chave/serviço por mocks.
- [ ] 5.5 Concluir review de risco/privacidade/credenciais proporcional, eixos code/review/reality e verify --record por sessão independente; Main aceitar somente o pedido completo e critérios anteriores de captura/MCP/opt-outs, com destino dos recursos e publicação apenas autorizada.

## Escopo de áudio substituído em 2026-11-25

As tarefas de implementação e prova de áudio passaram para `selecionar-modelos-transcricao/tasks.md`, por confirmação do dono. As demais tarefas permanecem abertas; integração ou prova dessa nova change não aceita este plano inteiro.

## 6. Correção delimitada do Ditado em 2026-11-26

Esta rodada executa somente os itens abaixo. Não marca nem aceita as tarefas anteriores por aproximação e não autoriza testes pagos ou mudança de perfil/TCC do usuário. Provas desta rodada ficam em `.tmp/verify/dictation-regression-20261126/` no checkout durável.

- [x] 6.1 Conciliar os cinco artefatos afetados e `docs/dictation.md` com aprimoramento opcional OFF por padrão e localização visível em Modelos; preservar a correção de notas, canônicas e tarefas anteriores.
- [x] 6.2 Tornar atualização nativa de modelo independente de enabled sob o lock existente, manter wrapper booleano compatível e provar toggle/model-only/validação/falha em store isolado, sem nova migração ou alteração do perfil real.
- [x] 6.3 Centralizar preferências no estado de AppSettings, controlar Estilo por props, proteger leitura/salvamento/merges e acrescentar controle sempre visível em Modelos com i18n, acessibilidade e escolha atual preservadas.
- [x] 6.4 Diferenciar AX missing de destino indisponível no HUD com texto curto e recuperação pela ação existente; registrar somente os novos metadados técnicos seguros e preservar gates, clipboard e destino original.
- [x] 6.5 Provar ON/OFF, desconhecido/read failure, falhas de save, respostas fora de ordem e modelo sem religamento; preservar OFF sem LLM e fallback bruto, acrescentar regressões de erro/recuperação/logger e revisão visual isolada em pt-BR/en nas duas localizações.
- [x] 6.6 Conferir scanner/revisão da árvore exata e checks completos na box/GitHub, com build nativo quando selecionado; integrar o patch autorizado sem aceitar as tarefas antigas, preservando recibos e dados.
- [ ] 6.7 Confirmar com o usuário a autorização de Acessibilidade do helper e a colagem no campo real; registrar separadamente o que depende da instalação do usuário, sem inferência paga nem anunciar que mocks comprovam o campo consumindo texto.

## 7. Remover resíduos do chat, preservando integrações

A retirada de 2.1, 2.2 e 5.3 é desistência explícita, não conclusão. A correção posterior do dono mantém integrações e seus controles. Partir do commit de fonte revisado do Ditado e confirmar seu CI nativo antes de editar 7.x, evitando conflito. Integrar na main somente após aceite das duas entregas e checks. Sem release por enquanto.

- [x] 7.1 Conciliar proposal/design/specs/tasks, remover requisitos pendentes de geração e criar deltas REMOVED da canônica de mídia; preservar 6.x e tarefas não autorizadas.
- [x] 7.2 Remover geração/edição/animação de mídia, fal ativo, catálogos, comandos/DTOs/bindings e três tools de mídia do MCP; manter leitura de histórico/assets e não apagar credenciais/dados.
- [x] 7.3 Remover mídia do frontend e seletor/opções exclusivas de chat; preservar detecção/teste/autenticação e escolha dos motores CLI para notas/Atividade/rotinas.
- [x] 7.4 Retirar página/navegação Agente, recuperar destinos antigos em Geral e realocar sons de rotinas preservando escolha; manter skills e Plugins/integrações/custom MCP operantes com linguagem coerente.
- [x] 7.5 Provar ausência funcional de mídia no registry/catálogo/dispatch, compatibilidade de fixtures legadas e continuidade de notas, áudio/Ditado, Atividade, rotinas, skills, integrações e vídeo de reunião; revisão visual pt-BR/en na box.
- [x] 7.6 Conferir árvore exata, scanner/gate/revisão independente e suites completas Linux/macOS GitHub; integrar por commits do escopo e preservar recibos. Nenhuma tag/release/instalação/reinício.
- [x] 7.7 Sincronizar somente documentação vigente/specs aceitas e memória; limpar recursos exclusivos das duas entregas preservando dados, provas e configuração compartilhada, com medição e relato do que ficou retido.

Provas do Ditado 6.1–6.5: `.tmp/verify/dictation-regression-20261126/acceptance-source.md`, `manual-review/2/summary.md`, recibos CI45826494314. Pipeline geral falhou na assinatura Windows ausente; checks completos macOS/Linux passaram. Integração remota6.6 concluída; prova real6.7 depende da permissão e confirmação do usuário. Planejamento7.1 validado estrito; implementação7.2–7.5 aceita e integração/fechamento7.6–7.7 concluídos no recorte técnico.

Provas7.2–7.5: `.tmp/verify/chat-settings-cleanup-20261126/acceptance-source.md`, `manual-review/1/summary.md` e CI45835707565 (fonte88ae2cc9). Fonte macOS/Linux aprovada; workflowoverallfailure por assinaturaWindows indisponível, testes/pacoteWindows semvalidação. Quatro deltas canônicos sincronizados com recibo `canonical-sync-receipt.json`; demais deltas históricos não aceitos nem sincronizados.

Fechamento 2026-11-26: código bfe028e2/88ae2cc9 e specs aceitas integrados/pushados na main remota7b7ca081; recibos em `.tmp/verify/chat-settings-cleanup-20261126/`. Memória oficial salva, relida e indexada na página424f2513-14fe-8155-94c7-ebccb988fed9. Cinco worktrees da box, duas sessões e dois Vite próprios encerrados; ganho livre observado4510273536bytes, caches compartilhados e trabalho anterior de reuniões preservados. Foram preservados5925itens de prova/148670551bytes regulares por hashes. Mirrors do Mac são retirados após o commit de fechamento. O checkout principal local permanece no HEAD anterior com16alterações alheias de ferramentas/Trail/AGENTS intactas; rascunhos próprios foram preservados em recibo e retirados dali. Branches remotas de teste permanecem por retenção segura, sem exclusão remota sem CAS. Tasks históricas não executadas e prova realAX6.7 permanecem abertas. Nenhuma release autorizada para estes ajustes; sem instalar/reiniciar app ou inferência paga.
