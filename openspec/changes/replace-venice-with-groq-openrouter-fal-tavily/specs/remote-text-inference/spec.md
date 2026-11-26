# Spec Delta

## Purpose

Preservar inferência textual remota por OpenRouter e a escolha dos motores CLI/endpoints existentes sem dependência ativa de Venice.

## ADDED Requirements

### Requirement: Texto remoto OpenRouter
O sistema SHALL executar geração de notas, aprimoramento de ditado explicitamente habilitado e demais consumidores de inferência textual por OpenRouter, usando modelos reais e preservando os resultados de cada fluxo.

#### Scenario: Nota e limpeza de ditado
- **WHEN** o usuário solicita geração de nota ou limpeza textual com a rota remota selecionada
- **THEN** OpenRouter produz o resultado pertinente sem trocar o provedor de transcrição selecionado, sem perder histórico e sem consumir modo CLI escolhido
- Test: integration — serviços de nota/cleanup com sucesso, erro e metering; prova nativa/external dos dois fluxos em sandbox autorizado.

#### Scenario: Ditado com aprimoramento desligado
- **WHEN** o usuário mantém ou desliga o aprimoramento do ditado
- **THEN** a próxima execução ainda não iniciada usa o texto reconhecido sem cleaner/LLM, preserva o provedor de áudio e não altera modelo/estilo salvos
- Test: unit/integration — chamada do pipeline com spy que rejeita qualquer cleaner; transcrito pontuado pelo ASR continua íntegro, sem chamada paga.

#### Scenario: Falha do aprimoramento explicitamente ligado
- **WHEN** o aprimoramento está ligado e a chave está ausente, ocorre timeout ou a resposta do modelo é inválida
- **THEN** o sistema preserva o transcrito original com aviso, sem fallback de motor e sem alterar a preferência salva
- Test: unit/integration — falhas controladas e texto original preservado; não executar inferência paga como parte do patch.

### Requirement: Catálogo e capacidades reais
O sistema SHALL apresentar modelos, capacidades, limites e preços provenientes do catálogo OpenRouter e SHALL resolver a seleção automática apenas para modelos que satisfaçam os requisitos da ação.

#### Scenario: Seleção com visão ou ferramentas
- **WHEN** a ação precisa de visão ou tool calling
- **THEN** o seletor usa capacidades reais do catálogo e não oferece ou roteia um modelo incompatível por inferência baseada em nome/traits
- Test: integration — parser do catálogo e resolução com casos positivos/negativos de modalities e supported_parameters.

### Requirement: Continuidade dos motores existentes
O sistema SHALL preservar motores CLI/endpoints, sua seleção e seus segredos, sem converter escolhas CLI válidas de notas, Atividade e rotinas para OpenRouter durante a migração.

#### Scenario: Perfil com Claude CLI
- **WHEN** um perfil com motores CLI de notas, rotinas e atividade recebe a atualização
- **THEN** essas escolhas continuam válidas e os usos remotos legados de notas/cleanup passam para OpenRouter sem regravar chaves de endpoint
- Test: unit — migração de settings e registro LLM usando stores isolados, incluindo usos mistos e overrides.

### Requirement: Privacidade textual verdadeira
O sistema SHALL apresentar a política efetiva do novo provedor sem herdar afirmações Venice de TEE, rota privada ou zero retention que OpenRouter não comprove.

#### Scenario: Modelo sem garantia de rota privada
- **WHEN** um modelo OpenRouter não tem a garantia exibida anteriormente por Venice
- **THEN** o UI/contrato não o rotula como TEE ou zero retention garantidos e mantém a escolha e consentimento pertinentes
- Test: integration — catálogo/privacy normalizado e UI com propriedades reais; revisão de headers e documentação.
