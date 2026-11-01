# Spec Delta

## ADDED Requirements

### Requirement: Contexto e guia correspondem à autoridade real
Turno vinculado SHALL receber contexto estruturado de conversa/run/engine/superfície/projeto/partição/workspace e políticas efetiva/configurada obtidos pelo host, sem tokens/segredos. Cliente externo recebe só contexto autorizado, nunca identidade arbitrária. Guia/catálogo SHALL distinguir leitura externa, mutações bound, custos e aprovações reais, sem declarar toda ferramenta read-only.

#### Scenario: Contexto externo e bound
- **WHEN** externo e turno vinculado pedem contexto/guia
- **THEN** externo não recebe dados de conversa e bound recebe sua identidade/política real, mesmo após troca de partição na UI
- Test: integration — recursos/despacho e escopo após espera

### Requirement: Semântica temporal e erros preservados
As ferramentas SHALL explicar que intervalos de atividade retornam registros completos sobrepostos com totais recortados e `active` é estado materializado, não watermark instantâneo. Falhas MUST preservar origem/código/classificação/disposição de entrega disponível; consulta de status MUST NOT ativar recurso/login/controle.

#### Scenario: Gap sobreposto e recurso desligado
- **WHEN** consulta retorna gap iniciado antes da janela ou status de recurso desligado
- **THEN** limites originais e totais recortados são claros, e nenhuma permissão é concedida
- Test: integration — fixture temporal e leitura sem efeito
