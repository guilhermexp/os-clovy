# Spec Delta

## MODIFIED Requirements

### Requirement: Seleção de provedor por uso
O Clovy SHALL separar provedores de rotinas, notas, ditado, áudio e Atividade. Rotinas usam CLI; notas aceitam CLI, endpoint ou OpenRouter; cleanup usa OpenRouter; áudio aceita Groq (padrão), OpenRouter ou Local (quatro modelos instalados compatíveis). APIs usam chave própria e MUST NOT passar pelo Clovy API. Atividade começa sem provedor e mantém IA desligada sem seleção. Chat interno e seu seletor não existem.

#### Scenario: Atividade sem provedor
- **WHEN** nenhum provedor de atividade foi escolhido e chega a hora de gerar o resumo do dia
- **THEN** nenhum texto de atividade sai da máquina e a interface mostra que o recurso precisa de um provedor configurado
- Test: integration — agendador com provedor "nenhum" não realiza chamada e registra estado desligado

#### Scenario: Nota gerada por CLI
- **WHEN** o provedor de notas é `claude` e uma gravação termina de ser transcrita
- **THEN** a nota estruturada é gerada pelo CLI e aparece no editor como as notas geradas pela OpenRouter
- Test: integration — CLI falso devolvendo nota e verificação da nota persistida

#### Scenario: Perfil salvo com uso Clovy
- **WHEN** um `provider-settings.json` salvo tem `notes` ou `dictationCleanup` com `kind: "clovy"` e o app abre
- **THEN** notas passam a OpenRouter e a limpeza segue na OpenRouter, sem chamada ao Clovy API, e o arquivo é regravado com o valor novo
- Test: integration — fixture de `provider-settings.json`, carga, gravação e recarga

#### Scenario: Transcrição Local escolhida
- **WHEN** o modelo local está pronto e o usuário escolhe Local na linha de Transcrição
- **THEN** `transcriptionProvider` é gravado como `local`, o modelo exibido é o do pacote instalado, e ditado, notas e prévia passam a usar o motor local
- Test: integration — gravação, recarga e leitura do motor efetivo

#### Scenario: Override de perfil sem modelo pronto
- **WHEN** um perfil tem override de transcrição para `local` e o modelo não está pronto
- **THEN** a transcrição desse perfil falha com `local_model_not_installed`, sem requisição à Groq, e a nota ou o ditado mostra a mensagem do motor local
- Test: unit — `effective_settings_for_profile` com estado do modelo falso; integration — servidor sentinela Groq com zero requisições

#### Scenario: Seleção de áudio independente de texto
- **WHEN** o usuário seleciona OpenRouter e Whisper para transcrição
- **THEN** a escolha de geração de notas, Atividade e limpeza de ditado permanece como estava
- Test: unit — persistência isolada e frontend.
