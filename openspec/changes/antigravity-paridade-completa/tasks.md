# Tasks

Plano aprovado explicitamente pelo dono antes da construção. Executor escolhido nas duas perguntas nativas: box agents-us, Codex Sol alto (`gpt-6.1-sol` high). O worker isolado executa a implementação; o orquestrador retém gateway, aceite, provas de CLI e app no Mac. Ler `proposal.md`, `design.md` e ambos os deltas antes de executar. Primeiro turno `clovy-gateway-sol-20261103` sobre main8b6c39d7, somente2.1/2.2/4.2; tarefas restantes continuam no escopo e só recebem caixa marcada após aceite da prova.

Restrições globais: reutilizar transporte/registro/broker existentes; nenhuma edição de configuração global, fake HOME, cópia de credenciais, bypass, dependência nova ou segunda memória. Dados e mensagens de prova são sintéticos. Cargo jobs=2/sccache; comandos nativos sem heavy-run. Stop/migração56 é base separada, não reimplementar. Preservar escolhas explícitas e alterações concorrentes.

Foco de revisão: configuração compartilhada/symlink e crash; servidor anterior sobrevivendo à retomada; regras globais permissivas; resultado de ferramenta perdido numa troca; retorno ao motor anterior restaurando contexto ou efeito rejeitado. Os testes abaixo pertencem à tarefa que altera cada caminho, não a uma etapa tardia genérica.

## 1. Contrato nativo do Antigravity

- [ ] 1.1 Qualificar o CLI instalado com canários temporários e conteúdo sintético: descoberta do MCP local com nome exclusivo, dois turnos retomados com troca de vínculo, timeout900, regras `--settings` efetivas e precedência sobre configuração permissiva, negação de escrita/comando/atuação não autorizados e efeitos de hooks/plugins/MCPs globais. Produzir `.tmp/verify/antigravity-paridade-completa/native-capabilities/report.md` com comandos, versão/identidade do executável, eventos sanitizados, efeitos observados e limites; conexão de texto já comprovada não será repetida como se fosse novidade. Não editar configurações globais para obter sucesso. Uma capacidade indispensável ausente bloqueia o aceite e retorna ao orquestrador com prova; não autoriza entregar modo reduzido.

## 2. Pool comum e transporte de turno

- [ ] 2.1 Em `src-tauri/src/chat_engine/{cli.rs,turn.rs}` e, se necessário, módulo local específico de guarda de configuração, conectar Agy ao `McpLaunch`/`TurnRegistry` existentes. RED/GREEN de integração deve demonstrar leitura real do gateway, escrita pendente sem efeito, aprovação/negação e revogação usando o broker real; nenhum catálogo ou handler duplicado por motor. A guarda prepara configuração local privada com nome único e timeout900, preserva a unidade preexistente e remove somente sua entrada por identidade. Cobrir sucesso, spawn/preparo falho, cancelamento, crash/recovery, symlink, edição concorrente e dois turnos no mesmo workspace. Atualizar o contrato de transporte em `docs/llm-providers.md` e a afirmação obsoleta no ADR0063 sem reescrever seu histórico.
- [ ] 2.2 Em `chat_engine/cli.rs` e testes próximos, ajustar somente diferenças comprovadas do protocolo Agy: argumentos/resultados/erros, resposta apenas no resultado final, verdict terminal e ausência de resultado. RED/GREEN deve assegurar efeito único, resposta não vazia real, falha terminal preservada, erro de aprovação não suprimido e hidratação/ordem comuns. Usar registros sanitizados da tarefa1.1, não novo parser por ferramenta ou resultado inventado.

## 3. Política comum e rotinas

- [ ] 3.1 Em `chat_engine/{policy.rs,policy_tests.rs,mod.rs}`, `routines.rs` e `providers/mod.rs`, implementar a política Agy conforme as capacidades nativas qualificadas e separar elegibilidade não assistida da preferência padrão. RED/GREEN com repositório/broker reais deve provar leitura permitida, escrita não assistida recusada sem efeito/cartão pendente, regra global incapaz de ampliar a autoridade, turnos separados por sessão/run/superfície e Stop/revogação. Preservar as flags proibidas e os comportamentos Claude/Codex; documentar a linha Agy e o alcance comum sem declarar contenção pela presença da flag.

## 4. Seleção e alternância transparentes

- [ ] 4.1 Atualizar `llm/registry.rs`, `providers/mod.rs`, `src/lib/chat-engine.ts` e seletor existente para qualquer um dos três instalado sozinho, respeitando escolha explícita e sem fallback silencioso em falha. RED/GREEN deve testar seleção efetiva de sessão/nota/rotina elegível com cada instalação única e persistência da escolha ao ter os três, não comparação de arrays ou textos internos. Interface segue o padrão existente e os dois idiomas; atualizar a documentação de escolha padrão.
- [ ] 4.2 Qualificar e corrigir o replay em `chat_engine/turn.rs` e testes de integração existentes: manter IDs só numa sequência contínua do mesmo motor e construir contexto canônico com referências/resultados relevantes ao trocar. RED/GREEN nas seis trocas direcionadas e A→B→A deve preservar evidência de ferramenta, tentativas substituídas, orçamento/contexto e histórico sem repetir efeito nem compartilhar token/aprovação. Reutilizar leitores/resumos existentes, sem segundo sistema de contexto. Registrar o comportamento em `docs/llm-providers.md`.

## 5. Geração pontual e recursos de texto

- [ ] 5.1 Qualificar e corrigir, apenas onde houver lacuna, `llm/{cli.rs,mod.rs,registry.rs}`, `providers/notes.rs` e os consumidores existentes de atividade. RED/GREEN deve provar schema exigido, erro/timeout sem órfãos, ausência de efeito e de troca de provedor, e exclusão de autoingestão. Executar geração de nota/resumo de atividade com texto sintético e Antigravity como único agente elegível; não gravar microfone, não ler transcrições pessoais nem modificar as rotas independentes de ASR/ditado. Atualizar a matriz factual de provedores.

## 6. Aceite integrado, revisão e destino

- [ ] 6.1 Rodar uma vez os gates canônicos do delta (`pnpm check`, `pnpm typecheck`, `pnpm test`, fmt, Clippy e Rust nativo aplicáveis), revisar o diff completo e todos os contratos/callers. Antes do commit final usar `/Users/guilhermevarela/orchestrator.v5/scripts/review-pair.sh --gate`; a indisponibilidade já comprovada exige contingência manual completa e registrada sem repetir o gate conhecido. Manter scanner integral com classificação pontual, sem allowlist genérica. Registrar árvore/hash, achados/tratamento e evidências em `.tmp/verify/antigravity-paridade-completa/` e o recibo da modalidade realmente executada aqui.
- [ ] 6.2 Integrar sem force na main autorizada e provar a fonte em bundle Mac próprio assinado: matriz Claude/Codex/Antigravity com catálogo normalizado do gateway, contexto referenciado, leitura, escrita aprovada e negada, clarificação, Stop, rotina, troca/retorno de motor e reabertura do histórico. A prova de instalação única deve não iniciar os outros CLIs, sem desinstalar o ambiente do usuário. Conector/mídia com pré-requisito ausente deve gerar o mesmo erro nos três, não efeito simulado. Atualizar a matriz e as tarefas com IDs de sessão/run, efeitos, capturas e CI da fonte; cota/auth/host indisponível permanece bloqueio explícito, não checkbox aprovado. Windows só recebe aceite com host Windows real; não publicar release/deploy nem substituir o app real.

## Workflow follow-up

- Com todos os aceites concluídos, arquivar esta change pelo CLI, sincronizar somente AGENTS.md e executar cleanup seletivo dos recursos próprios, com bytes/recibos e retenções justificadas.
- Preservar dados, perfis, configurações, provas e trabalho não integrado; não somar novamente limpezas anteriores nem tocar Macro ou seus mounts.
- Checkpoint apenas documental do plano aprovado e arquivo do Dock: revisão manual de contingência14 em `.tmp/verify/melhorias-agente-audio-automacao/manual-review/14/summary.md`, scanner em `.tmp/verify/T7/dock-archive-parity-plan-20261103/full-scan/classification.json`. Não conclui6.1 nem antecipa implementação, provas nativas ou elegibilidade não assistida.
