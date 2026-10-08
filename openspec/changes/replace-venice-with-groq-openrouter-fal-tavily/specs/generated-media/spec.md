# Spec Delta

## Purpose

Preservar geração e edição de imagens e geração e animação de vídeos com endpoints fal reais até assets utilizáveis no Clovy.

## ADDED Requirements

### Requirement: Imagem e edição fal
O sistema SHALL gerar e editar imagens por modelos fal que suportem a ação, preservando entrada, preferências de segurança e resultado utilizável sem lista de modelos Venice ou sucesso simulado.

#### Scenario: Gerar imagem
- **WHEN** o usuário escolhe um modelo fal disponível e solicita uma imagem
- **THEN** a fila é concluída e o asset final é validado e disponibilizado no fluxo existente com formato real e erro útil se a geração falhar
- Test: integration — submit/status/result/download e limites em sandbox; prova externa/UI requer chave e custo autorizados.

#### Scenario: Editar imagem
- **WHEN** o usuário fornece imagem e instrução de edição a um modelo fal que suporta edição
- **THEN** a imagem é enviada pelo contrato permitido e a edição produz asset final sem upload público adicional não autorizado ou fallback Venice
- Test: integration — entrada real de edição, payload/result e falhas; prova externa em cópia sandbox autorizada.

### Requirement: Vídeo e animação fal
O sistema SHALL executar geração de vídeo e animação de imagem por fal preservando os estados de job, parâmetros suportados e asset final, sem confundir status concluído com resultado válido.

#### Scenario: Vídeo assíncrono
- **WHEN** uma geração de vídeo transita pela fila para execução e conclusão
- **THEN** o Clovy mantém o job, consulta seu estado e entrega o vídeo completo ou o erro final sem duplicar a geração
- Test: integration — transições queued/running/completed/error e resultado/download, mais prova externa/UI autorizada.

#### Scenario: Animar imagem
- **WHEN** o usuário pede animação de uma imagem com modelo fal compatível
- **THEN** recebe o vídeo final com os parâmetros aceitos e não uma chamada de text-to-video disfarçada
- Test: integration — endpoint/input de animação distinto e job/result; prova externa em sandbox.

### Requirement: Limites e cobrança de mídia
O sistema SHALL preservar limites de entrada/download, cancelamento e erro pertinentes e SHALL usar preços/estimativas defensáveis dos endpoints selecionados, sem falsa cotação herdada de Venice.

#### Scenario: Falha ou asset excede limite
- **WHEN** fal retorna erro final, URL inválida ou asset maior que o limite vigente
- **THEN** o sistema não persiste asset inválido nem cobra uma conclusão fictícia e informa a falha sem vazar chave/URL sensível
- Test: integration — transições de erro, validação de URL/tamanho e ciclo de autorização/settlement existentes.
