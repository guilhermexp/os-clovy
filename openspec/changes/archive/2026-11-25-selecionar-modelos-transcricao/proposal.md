# Proposal

## Why

A configuração de transcrição duplica controles e o motor local está fixo no Parakeet Ultra. O usuário quer experimentar quatro modelos em português, instalar somente os escolhidos e remover os que não usa, além de aproveitar sua chave OpenRouter para Whisper.

## What Changes

- Reunir provedor e modelo de transcrição em uma seção coerente, comum a ditado, notas e prévia: Local, Groq ou OpenRouter.
- Oferecer exatamente Whisper large-v3-turbo, Parakeet Ultra, Fluid Speech Pro e Phonon-2.5 no catálogo local do Mac compatível; não incluir modelos de dezenas de GB.
- Selecionar um modelo ausente inicia instalação verificável com progresso/cancelamento. A seleção efetiva muda somente após sucesso; erro, cancelamento ou escolha posterior preservam o motor ativo.
- Gerenciar instalação e remoção por modelo, preservando os outros arquivos, o Parakeet já instalado e as preferências existentes.
- Adicionar transcrição Whisper pela API dedicada da OpenRouter, reutilizando a credencial existente e mantendo a Groq disponível.
- Substituir somente o escopo de transcrição da change aberta `replace-venice-with-groq-openrouter-fal-tavily`; manter seus outros requisitos e pendências.

## Capabilities

### New Capabilities

Nenhuma.

### Modified Capabilities

- `local-transcription`: catálogo de quatro modelos, disponibilidade por modelo, instalação/remoção isolada, seleção transacional e execução pelo modelo escolhido.
- `llm-providers`: seleção OpenRouter para áudio, separada de texto e limpeza, com sua chave própria e sem fallback entre provedores.

## Impact

Frontend de configurações/modelos, bindings Tauri, catálogo e persistência de provedores, downloader/manifestos, helper Swift, preparo/licenças/pins, ditado e processamento de notas/prévia, testes e documentação. FluidAudio elegível e WhisperKit nativo substituem o hardcode do helper, com adaptador próprio para os componentes Core ML do Pro. Local permanece restrito às plataformas compatíveis; Groq e OpenRouter ficam disponíveis nas plataformas já suportadas pelo app. Não altera o chat removido, MCP, Atividade, pós-processamento nem publica release.
