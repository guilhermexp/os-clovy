# Spec Delta

## MODIFIED Requirements

### Requirement: Seleção de provedor por uso
O sistema SHALL permitir escolher o provedor separadamente para: chat/agente, geração de notas, limpeza de ditado, transcrição de áudio e recursos de atividade (relatórios, resumo do dia, resumo de sessões de agentes). O chat MUST aceitar só motores CLI. As notas MUST aceitar CLI, endpoint cadastrado ou OpenRouter com a chave do usuário. A limpeza de ditado usa a OpenRouter. A transcrição MUST aceitar Groq ou OpenRouter (com a chave do próprio provedor), ou Local (um dos quatro modelos instalados e compatíveis na máquina); o padrão é Groq. Nenhum uso MUST passar pelo Clovy API. Para recursos de atividade, o padrão MUST ser "nenhum", e sem provedor escolhido esses recursos ficam desligados.

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

### Requirement: Notas com chave própria vão direto ao provedor
Com o motor de Notas = OpenRouter, o desktop SHALL mandar a transcrição de nota ao motor de transcrição escolhido (Groq ou OpenRouter direto com sua própria chave, ou o modelo Local escolhido no dispositivo) e a geração e a limpeza da nota à OpenRouter, sempre direto, com um modelo explícito. A requisição MUST levar só a chave do provedor chamado. Não existe rota de reserva: sem a chave, a chamada falha como descrito em "Falha clara sem chave nem servidor".

#### Scenario: Transcrição final com chave Groq
- **WHEN** o motor de transcrição é Groq, a chave Groq está salva e um turno da reunião é transcrito
- **THEN** a requisição vai a `/audio/transcriptions` da Groq com modelo, `response_format=json`, idioma, prompt do dicionário e nome de arquivo canônico
- Test: integration — Groq falsa confere o corpo multipart e os cabeçalhos

#### Scenario: Transcrição final com motor Local
- **WHEN** o motor de transcrição é Local e um turno da reunião é transcrito
- **THEN** o turno é transcrito pelo modelo local com a dica de idioma, e nenhuma requisição de transcrição sai da máquina
- Test: integration — transcritor local falso e Groq falsa com zero requisições

#### Scenario: Prévia ao vivo com chave Groq
- **WHEN** o motor de transcrição é Groq, a chave Groq está salva e a prévia ao vivo envia um trecho
- **THEN** o trecho vai direto à Groq
- Test: integration — pedido de prévia contra Groq falsa

#### Scenario: Prévia ao vivo com motor Local
- **WHEN** o motor de transcrição é Local, a prévia ao vivo está ligada e um trecho fica pronto
- **THEN** o trecho é transcrito localmente, sem orçamento de segundos por hora
- Test: integration — transcritor local falso

#### Scenario: Sem chave Groq
- **WHEN** o motor de transcrição é Groq, não há chave Groq salva e uma gravação começa e termina
- **THEN** a prévia ao vivo não inicia, a transcrição falha com `groq_api_key_missing` e nenhuma requisição de transcrição ou prévia é feita
- Test: unit — portão da prévia com e sem chave; integration — servidor sentinela em porta efêmera, injetada como a URL legada, com zero requisições

#### Scenario: Falha transitória da Groq
- **WHEN** a Groq responde 503 na primeira tentativa e 200 na segunda
- **THEN** a transcrição devolve o texto da segunda resposta depois de exatamente duas requisições; um 400 não é repetido
- Test: integration — Groq falsa com contador de chamadas

#### Scenario: Áudio sem fala
- **WHEN** a Groq devolve texto vazio, ou o modelo local devolve texto vazio
- **THEN** a transcrição falha com `no_speech`, que o pipeline trata como fonte sem fala
- Test: integration — Groq falsa; transcritor local falso

#### Scenario: Chave Groq rejeitada
- **WHEN** a Groq responde 401 ou 403
- **THEN** a transcrição falha com `groq_api_key_rejected` e a frase que manda atualizar a chave
- Test: integration — Groq falsa

#### Scenario: Áudio acima de 25 MB
- **WHEN** o motor é Groq e o arquivo do trecho tem mais de 26 214 400 bytes
- **THEN** a transcrição falha com `audio_too_large` sem nenhuma requisição; com motor Local esse limite não existe
- Test: integration — Groq falsa sem requisições; transcritor local falso com arquivo grande

#### Scenario: Geração com chave OpenRouter
- **WHEN** a chave OpenRouter está salva e o modelo de notas é explícito
- **THEN** a nota vem de uma chamada `/chat/completions` à OpenRouter com o prompt `note_generate.md` e os rótulos de fonte limpos
- Test: integration — OpenRouter falsa

#### Scenario: Limpeza da transcrição da nota com chave OpenRouter
- **WHEN** a chave OpenRouter está salva e o pipeline limpa a transcrição com o estilo `note_transcript_cleanup`
- **THEN** a chamada vai à OpenRouter com o prompt `note_transcript_cleanup.md` e `temperature: 0`
- Test: integration — OpenRouter falsa

#### Scenario: Geração com Auto ou sem chave
- **WHEN** o modelo de notas salvo é `open-software/auto`, ou não há chave OpenRouter
- **THEN** com Auto o app troca, uma vez, pelo modelo padrão explícito e a geração vai direto à OpenRouter; sem chave a geração falha com `openrouter_api_key_missing` sem requisição
- Test: unit (Rust) — carga de settings com Auto; integration — sem chave, zero requisições


#### Scenario: Transcrição OpenRouter por trechos
- **WHEN** OpenRouter é o motor de transcrição e uma nota possui turnos e prévia autorizada
- **THEN** os trechos são enviados individualmente à API de transcrição OpenRouter com seus offsets próprios, sem depender de timestamps opcionais nem do pacer exclusivo da Groq
- **AND** cada requisição leva apenas a chave OpenRouter e o áudio previsto pelo trecho, sem fallback ou negociação paga automática de formato
- Test: integration — OpenRouter falsa, dois turnos, prévia e sentinela Groq sem acessos.

### Requirement: Falha clara sem chave nem servidor
Quando falta a chave do provedor de que a nota precisa, a nota SHALL dizer, na língua da interface (pt-BR ou en), qual chave falta e onde salvá-la (Configurações > Modelos > Mais opções > Chaves de API de provedores), sem tentar outra rota, e MUST NOT mostrar o erro cru da biblioteca HTTP. A chave Groq só é exigida para transcrição Groq e a chave OpenRouter para transcrição OpenRouter; com Local nenhuma chave remota é exigida. O áudio fica salvo e "Tentar de novo" continua disponível. Sem a chave OpenRouter, a limpeza opcional da transcrição é pulada; a transcrição que escolheu OpenRouter falha claramente, sem trocar de rota.

#### Scenario: Transcrição sem chave Groq e sem servidor
- **WHEN** o motor de transcrição é Groq, não há chave Groq e uma gravação termina
- **THEN** a transcrição falha com `groq_api_key_missing` sem nenhuma requisição de rede, e a nota mostra traduzida a frase "Add your Groq API key in Settings > Models > More options > Provider API keys to transcribe recordings."
- Test: integration — servidor sentinela em porta efêmera, injetada como a URL legada, recebe zero requisições; unit — `note-failure-banner.test.tsx` traduz a frase em pt-BR

#### Scenario: Motor Local sem chave Groq
- **WHEN** o motor de transcrição é Local, não há chave Groq e uma gravação termina
- **THEN** a transcrição é feita localmente e nenhuma mensagem de chave Groq aparece
- Test: integration — transcritor local falso e servidor sentinela com zero requisições

#### Scenario: Geração sem chave OpenRouter e sem servidor
- **WHEN** o motor de Notas é OpenRouter e não há chave OpenRouter
- **THEN** a geração falha com `openrouter_api_key_missing` e a nota mostra a frase de transcrição salva que pede a chave OpenRouter
- Test: integration — nenhuma requisição; unit — `processing.rs` mapeia o código para a frase; `note-failure-banner.test.tsx`


#### Scenario: OpenRouter de áudio sem chave
- **WHEN** o motor de transcrição é OpenRouter e não há chave disponível
- **THEN** transcrição e prévia não enviam áudio e mostram a configuração necessária da OpenRouter, preservando o áudio salvo para tentar novamente
- Test: integration — ausência de chave, erro do cofre, sentinelas sem acessos; unit frontend — mensagem traduzida.

## ADDED Requirements

### Requirement: Whisper por OpenRouter
O sistema SHALL oferecer Whisper large-v3-turbo, Whisper large-v3 e Whisper-1 da OpenRouter para transcrição, com Turbo como padrão desse provedor. SHALL usar a API dedicada de transcrição, conservar idioma e vocabulário autorizado e validar resposta e limites sem usar chat multimodal ou trocar de provedor. A chave existente MUST ser reutilizada sem ser exibida ou registrada.

#### Scenario: Requisição em português
- **WHEN** OpenRouter e Whisper Turbo estão selecionados e há uma chave disponível
- **THEN** o áudio é enviado a `/api/v1/audio/transcriptions` com o identificador do modelo, idioma pt quando escolhido e formato suportado; o texto válido volta ao ditado ou à nota
- Test: integration — servidor HTTP falso confere credencial, corpo, idioma, modelo, limite de upload, resposta vazia, rejeição de chave e timeout.

#### Scenario: Catálogo separado por modalidade
- **WHEN** o usuário abre a escolha de modelo de transcrição OpenRouter
- **THEN** aparecem os Whisper de transcrição e os modelos exclusivos de chat não aparecem
- Test: unit — catálogo STT e seletor.

### Requirement: Seleção de transcrição e gerenciamento sem duplicação
Configurações SHALL apresentar um único grupo coerente para selecionar provedor e modelo de áudio, com disponibilidade, tamanho e estado de instalação por modelo local. O usuário SHALL poder gerenciar e remover modelos inativos sem trocar o modelo em uso. Todos os estados e ações MUST ter rótulos acessíveis e traduzidos, com navegação por teclado e recuperação clara de falhas.

#### Scenario: Navegação pelo seletor
- **WHEN** o usuário usa teclado para escolher Local, um modelo ausente e sua instalação
- **THEN** a interface anuncia progresso e o modelo ativo, permite cancelar ou tentar novamente e só mostra a nova escolha como ativa após confirmação persistida
- Test: unit frontend — interação pelo papel acessível e respostas assíncronas; visual — capturas pt-BR/en na largura mínima suportada e ampla.

#### Scenario: Gerenciar modelos de teste
- **WHEN** dois modelos estão instalados e o usuário abre o gerenciamento
- **THEN** cada modelo mostra estado e tamanho real e pode ser removido individualmente, com aviso claro ao remover o ativo
- Test: unit frontend e prova nativa com diretórios isolados.
