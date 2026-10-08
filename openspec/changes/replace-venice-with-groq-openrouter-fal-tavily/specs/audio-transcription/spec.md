# Spec Delta

## Purpose

Transcrever áudio de ditado e notas com Groq/Whisper, preservando o fluxo local de gravação e os dados já existentes do usuário.

## ADDED Requirements

### Requirement: Groq Whisper para todo áudio novo
O sistema SHALL transcrever ditado e notas por Groq, com Whisper multilíngue válido e padrão whisper-large-v3-turbo, sem rota ou fallback Venice.

#### Scenario: Ditado com áudio real
- **WHEN** o usuário segura Fn, fala e solta com Groq configurado
- **THEN** a gravação é transcrita, a resposta identifica Groq e o texto é inserido no alvo segundo o fluxo existente
- Test: e2e — prova nativa pelo dono após grants e configuração, com eventos de gravação, transcrição e entrega; contrato local não substitui voz real.

#### Scenario: Nota e preview
- **WHEN** uma nota contém áudio válido e o usuário solicita transcrição ou preview autorizado
- **THEN** o sistema processa áudio/chunks/preview e persiste texto, idioma opcional e provider Groq sem perder a gravação ou os demais dados
- Test: integration — caminho de processamento/HTTP com erros e boundaries existentes; prova nativa de nota em perfil sandbox com configuração real.

### Requirement: Idioma e contexto de áudio
O sistema SHALL preservar o idioma opcional e o contexto/dicionário já construídos no contrato de áudio, respeitando limites do provedor, sem enviar título da nota ou dados alheios ao contexto autorizado.

#### Scenario: Áudio em português com contexto
- **WHEN** o usuário escolhe português e fornece vocabulário no dicionário
- **THEN** Groq recebe idioma/contexto apropriados e o resultado mantém o contrato existente de texto e idioma opcional, sem novo contrato de segmentos ou palavras
- Test: integration — boundary multipart/parser com idioma, contexto e ausência de título, mais prova externa autorizada com vocabulário do dono.

### Requirement: Erro de ASR observável
O sistema SHALL informar falta de configuração, modelo inválido, rejeição do provedor, áudio sem fala e falha de transcrição sem fingir sucesso ou culpar permissões concedidas.

#### Scenario: Chave ausente ou inválida
- **WHEN** não existe credencial Groq válida ou o provedor recusa a credencial
- **THEN** o fluxo termina com erro útil sem transcrição/paste falsos, sem vazar segredo e sem pedir novamente TCC para mascarar a falha
- Test: integration — erros HTTP e ausência de configuração exercitados sem chamada paga; UI nativa mostra o erro correspondente.
