# Tasks

## 1. Catálogo e instalação por modelo

- [x] 1.1 Generalizar manifesto, identidades, elegibilidade, DTOs/comandos/eventos e persistência para quatro modelos, preservando Ultra instalado e preferências antigas; RED/GREEN de migração, plataforma e seleção com arquivos existentes; atualizar docs/audio-pipeline.md.
- [x] 1.2 Implementar download, integridade, cancelamento e retomada por modelo com manifestos e auxiliares fixos; RED/GREEN contra HTTP falso para quatro modelos, corrupção, parcial e cancelamento isolado; documentar pins/licenças e ocupação.
- [x] 1.3 Implementar remoção isolada, caches próprios, bloqueio durante uso e troca efetiva transacional; RED/GREEN de dois modelos, remoção ativa/inativa, gravação que falha e respostas fora de ordem; documentar fluxo de recuperação.

## 2. Execução nativa local

- [x] 2.1 Atualizar helper, FluidAudio elegível, locks, preparo/pin/licenças para Ultra e Phonon-2.5 GPU; verificar build Swift e testes de protocolo/carga no Mac, preservando mínimo macOS por modelo e docs de preparo.
- [x] 2.2 Integrar componentes Core ML próprios do Fluid Speech Pro por adaptador sem substituir o SDK por fork incompatível; verificar carga, preprocessor/decoder/frames e inferência portuguesa nativa em perfil isolado; documentar limite real observado.
- [x] 2.3 Integrar WhisperKit Turbo com pesos e tokenizer gerenciados, downloads implícitos desativados e reutilização de carga; verificar build, duas inferências portuguesas, erro de arquivo ausente e descarte; documentar manifestos e memória/caches próprios.

## 3. OpenRouter para transcrição

- [x] 3.1 Adicionar backend/modelos STT e seleção OpenRouter independente, chave existente e API de transcrição dedicada; RED/GREEN HTTP de idioma/hint/modelo/limites/sem fala/401/403/timeout, catálogo sem modelos de chat e zero envio para outro provedor; atualizar docs e mensagens pt-BR/en.
- [x] 3.2 Propagar identidade capturada por ditado/notas/prévia, trechos individuais OpenRouter e todos os modelos locais, preservando pacer/packs Groq e cleanup existente; RED/GREEN de atribuição de dois turnos, mudança durante operação, falta de chave e transcrição Local sem rede; atualizar contratos e ADR0074 com adendo.

## 4. Seleção e gerenciamento na interface

- [x] 4.1 Substituir controles duplicados por grupo único provedor/modelo, seleção que instala e gerenciamento por modelo com tamanho/estado/remover; RED/GREEN frontend para sucesso, falha, cancelamento, troca concorrente, remoção e disponibilidade por sistema, mantendo controles de idioma/limpeza independentes.
- [x] 4.2 Verificar teclado, acessibilidade, pt-BR/en e estados críticos em largura mínima/ampla com Impeccable; entregar capturas antes/depois e detector dos arquivos alterados, no máximo duas rodadas visuais; registrar prova de UI sem chamá-la inferência nativa.

## 5. Integração e entrega

- [x] 5.1 Executar pnpm check/typecheck/test na box por box-test; cargo fmt/clippy e testes Rust pertinentes no Mac com jobs=2/sccache; provar quatro inferências locais em português sem microfone real, carga reutilizada, troca e remoção isoladas; registrar logs, falhas da baseline e limites da plataforma sem alegar chamada paga OpenRouter.
- [x] 5.2 Revisar árvore completa com scanner e review-pair.sh --staged --gate, tratar achados e conferir --verify-receipt antes/depois do commit; entregar relatório único com hashes e critérios/provas ao orquestrador. Recibos duráveis previstos: checkout principal .tmp/verify/selecionar-modelos-transcricao/review-pair/1/summary.md ou manual-review/1/summary.md.

## Workflow follow-up

- O orquestrador aceita a entrega e marca tarefas somente após conferir provas; mantém faltantes abertos.
- Arquivar a change aceita e atualizar specs pelo CLI; sincronizar instruções/memória, revisar o delta de fechamento e integrar na main sem reescrever histórico.
- Não publicar release nem instalar build de produção. Nenhuma inferência paga OpenRouter foi autorizada como teste; provas HTTP locais e catálogo público têm esse limite declarado.
- Limpar os serviços, worktree/branch comprovadamente integradas e builds/caches exclusivos, inclusive box-test clean. Preservar dados e recibos no checkout principal, com espaço antes/depois e retenções justificadas.

## Aceite e fechamento

Aceite do orquestrador: checkout principal `.tmp/verify/selecionar-modelos-transcricao/acceptance.md`. Revisão pareada rodadas1–4 mais revisão manual de contingência remota do delta final em `manual-review/1/summary.md`; commit36fff4a confere árvoree28b16b8. Última validação focada na agents-us:74testes aprovados. As provas nativas anteriores à restrição estão preservadas; não executar novos testes/compilações pesados no Mac. Arquivamento/sync e integração usam revisão manual remota de fechamento em `manual-review/2/summary.md`; recibos e dados continuam no checkout durável.
