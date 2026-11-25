# Design

## Context

Ver proposal.md. `transcription_model.rs` possui um manifesto singleton e pasta fixa Ultra; os comandos, eventos e DTOs não identificam modelo. `local_transcriber.rs` envia `version: ultra` e o helper Swift força esse modelo. `AppSettings` duplica `ModelRow` e radios de `LocalTranscriptionSettings`. A seleção de backend já é capturada por operação e os turnos locais são individuais: preservar essas garantias. O usuário confirmou substituir apenas a parte de áudio do plano antigo e iniciar uma sessão nova do Codex.

## Goals / Non-Goals

**Goals:** contratos únicos por modelo, instalação verificável e remoção isolada; fluxo curto para experimentar e voltar; inferência local nativa reutilizável; OpenRouter ASR independente de texto.

**Non-Goals:** novo modelo além dos quatro, suporte local Windows/Linux/Intel, pós-processamento novo, chat, Atividade, MCP, release, perfil ou credenciais reais do dono. Os números de WER pesquisados não são garantias de versões Core ML e não viram propaganda na interface.

## Decisions

### Catálogo gerenciado e compatibilidade

Identidades estáveis próprias e registro curado: Ultra existente, Whisper Turbo, Pro, Phonon-2.5. Manifestos incluem repositório/revisão fixa, caminho permitido, tamanho, SHA-256 e atribuição. Manter a pasta Ultra reconhecida. Estado, evento, cancelamento e diretório são qualificados por modelo; validar caminhos e não seguir symlinks para apagar fora da raiz. Conservar verificações de integridade, retomada e cancelamento do downloader atual, sem instalar outros modelos automaticamente.

### Runtimes nativos no helper existente

Usar FluidAudio v0.18.3 (07/11/2026, `c8b41f98eae668794dd0083bbe028fe7e6585051`) para Ultra e `.phonon25` GPU. Pro usa seus componentes pelo inicializador público de `AsrModels`, com semântica da família v3 e seu próprio preprocessor/vocabulário; validar frames/decoder nativamente. Não substituir FluidAudio pelo fork Pro que não contém Ultra/Phonon.

Whisper usa WhisperKit v1.1.1 (08/10/2026), `modelFolder` e `tokenizerFolder` gerenciados, `download: false`, sem Python e sem download implícito durante inferência. Fixar também todos os arquivos auxiliares do tokenizer antes do manifesto final. A alternativa descartada é instalar Python/ambientes ou carregar um CLI externo a cada ditado. Preservar macOS 14 para Ultra; elegibilidade dos demais é explícita por modelo, sem aumentar globalmente o mínimo do app. Atualizar locks, preparo, `pin.json` e licenças juntos.

Pins dos pesos encontrados por metadados públicos:
- Ultra: manter manifesto e revisão atuais `95eaa59a39d4394f047a4dc5cce480388a60d1b6`.
- Pro: `FluidVoice-Labs/fluid-speech-pro-coreml@3b9d38a93d606f84011e5a0a669b7ea146375a22`, aproximadamente 652 MB.
- Phonon: `Muesli-HQ/phonon-2.5-coreml@a042ef89966d289df295709b58ffdabb50bd2510`, aproximadamente 290 MB.
- Whisper: `argmaxinc/whisperkit-coreml@0f63a7800b00dd0226abd051b906c246e1907482`, pasta `openai_whisper-large-v3-v20240930_turbo`, aproximadamente 1,64 GB mais tokenizer. Evitar a variante comprimida nesta primeira integração.

### Troca transacional e operação capturada

Separar escolha pendente da configuração efetiva. Selecionar ausente inicia download e registra intenção; ao concluir, só ativar se essa intenção ainda for a mais recente, o modelo estiver pronto e salvar funcionar. A operação mantém provedor/modelo próprios mesmo se o usuário editar Configurações. Serializar carga/descarte do helper e exclusão com leases de uso; a remoção em uso retorna erro recuperável. Remover modelo inativo nunca altera o selecionado; remover ativo confirma explicitamente a transição para Groq, sem inferência automática.

### OpenRouter de áudio

Backend específico em vez de reutilizar cliente de chat. Reutilizar chave OpenRouter do store canônico, IDs STT curados confirmados em `GET /api/v1/models?output_modalities=transcription`, POST dedicado `https://openrouter.ai/api/v1/audio/transcriptions`. Preferir multipart com `response_format=json`, idioma e vocabulário permitido; limites/timeout/redirects/erro conforme contrato existente e documentação. Não enviar chave Groq nem usar seu pacer. Timestamps variam por upstream: transcrever um trecho/turno por requisição e aplicar offsets já conhecidos, sem packs Groq nem retry pago de negociação. Áudio sem fala, falta/rejeição de chave e indisponibilidade mantêm erros claros, sem fallback.

### Interface

Refinar a superfície existente com Impeccable, preservando tokens e componentes. Um grupo Transcrição de áudio: Provedor e Modelo; estado compacto da instalação e ação contextual. Um gerenciamento expansível lista os quatro modelos com tamanho instalado e remoção individual. A seleção pendente mostra qual modelo continua ativo. Retirar os controles redundantes, manter geração de notas, idioma e limpeza em seus próprios controles. pt-BR/en, foco, leitor de tela e largura mínima do desktop são critérios de aceite. Não criar outro visual para o aplicativo inteiro.

## Risks / Trade-offs

- Mais runtimes e compilação Swift → dependências fixas, teste nativo dos quatro modelos e pin de fontes/licenças no preparo.
- Pro tem nomes de componentes diferentes → adaptador explícito, fixture portuguesa e teste de protocolo; não renomear às cegas.
- Downloads podem ultrapassar GB → exibir tamanho antes de baixar e progresso real; remover modelos e parciais próprios.
- API OpenRouter varia segmentos por upstream → pedidos individuais com resposta de texto estável.
- Box Linux não executa Core ML nem a biblioteca Rust nativa do app → frontend/checks portáveis pela box; build Swift/Rust e inferência por Mac Apple Silicon isolado, com a limitação explícita.
- Instalação antiga do Ultra → migração por identidade sem baixar novamente, teste com arquivos existentes.

## Migration Plan

Manter Groq como padrão e todas as chaves existentes. Preferências Local antigas resolvem para Ultra. Inicializar novas seleções somente quando não existirem. Preservar o estado salvo se qualquer mudança falhar; não reescrever histórico. Acrescentar adendo ao ADR0074 e atualizar docs vigentes. O plano anterior deixa de definir áudio, preservando suas outras capacidades.

## Verification

RED/GREEN dos contratos e testes existentes. `pnpm check`, `pnpm typecheck` e `pnpm test` na box por `box-test`; Rust/Swift nativos em Mac compatível com Cargo jobs=2 e sccache. Testar os quatro motores com fala portuguesa sintética em diretório isolado, carregamento reutilizado, troca e remoção; nenhuma captura real ou perfil pessoal. OpenRouter via HTTP falso e catálogo público, sem inferência paga. QA visual limitado às duas rodadas da skill, com capturas antes/depois e estados críticos. Gate final `/Users/guilhermevarela/orchestrator.v5/scripts/review-pair.sh --staged --gate`, conferência por `--verify-receipt`; contingência manual se exit4. Relatórios e recibos duráveis em `.tmp/verify/selecionar-modelos-transcricao/` do checkout principal. Integração, sync e cleanup cabem ao orquestrador; sem release.
