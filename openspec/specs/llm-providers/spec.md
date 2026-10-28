# llm-providers Specification

## Purpose
Permite que o usuário use seus próprios provedores de IA no Clovy (CLIs de agentes locais e endpoints OpenAI-compatíveis) para geração pontual e como motor do chat, sem depender do Clovy API.

## Requirements

### Requirement: Catálogo de provedores CLI detectados
O sistema SHALL reconhecer os CLIs `claude`, `codex`, `pi`, `agy`, `cursor-agent` e `copilot`, resolvendo-os pelo PATH do shell de login do usuário (não apenas pelo PATH herdado pelo app gráfico) e exibindo, para cada um, se está instalado, o caminho resolvido e a versão quando o CLI a informar. O ambiente do perfil do usuário (por exemplo `HOME` real e variáveis como `PI_CODING_AGENT_DIR`) MUST ser preservado nas execuções. No Windows, que não tem shell de login, o sistema MUST resolver os CLIs pelo `Path` do ambiente do app, lido sem diferenciar maiúsculas de minúsculas, mais os diretórios onde os instaladores oficiais desses CLIs gravam; MUST aceitar só arquivos com extensão listada em `PATHEXT`; e MUST entregar ao CLI resolvido como `.cmd` ou `.bat` o prompt exatamente como foi composto.

#### Scenario: CLI instalado fora do PATH do app gráfico
- **WHEN** o `claude` existe apenas em um diretório adicionado pelo shell de login (por exemplo `~/.local/bin`) e o usuário abre a lista de provedores
- **THEN** o Clovy mostra `claude` como instalado com o caminho resolvido e permite selecioná-lo
- Test: unit — resolução de PATH via shell de login com executável falso em diretório temporário

#### Scenario: CLI ausente
- **WHEN** nenhum executável `cursor-agent` é encontrado
- **THEN** o provedor aparece como não instalado, não pode ser selecionado e o motivo é exibido
- Test: unit — detecção com PATH sem o executável

#### Scenario: CLI instalado pelo npm no Windows
- **WHEN** no Windows o `codex` foi instalado pelo npm, que grava `codex.cmd` e um script `codex` sem extensão em `%APPDATA%\npm`, e o ambiente do app traz esse diretório na variável `Path`
- **THEN** o Clovy mostra `codex` como instalado com o caminho de `codex.cmd` e a versão informada pelo CLI
- Test: unit — resolução com chave `Path`, `PATHEXT` e diretório temporário contendo `codex.cmd` e `codex` sem extensão

#### Scenario: CLI fora do Path no Windows
- **WHEN** no Windows o `claude.exe` existe só em `%USERPROFILE%\.local\bin` e esse diretório não está no `Path` do app
- **THEN** o Clovy mostra `claude` como instalado com esse caminho
- Test: unit — diretórios de reserva do Windows derivados de `USERPROFILE` e `APPDATA`

#### Scenario: Prompt íntegro num CLI `.cmd`
- **WHEN** no Windows um CLI resolvido como `.cmd` recebe um prompt com várias linhas, aspas, `%`, `^` e `&`, por argumento ou pela entrada padrão
- **THEN** o CLI recebe o texto byte a byte igual ao composto pelo Clovy, ou a execução falha com erro explícito, nunca com o texto alterado
- Test: integration (Windows) — `.cmd` falso que devolve o argumento e a entrada padrão recebidos

### Requirement: Endpoints OpenAI-compatíveis nomeados
O sistema SHALL permitir cadastrar vários endpoints OpenAI-compatíveis, cada um com nome, URL base, modelo e chave opcional. A chave MUST ficar no Keychain e nunca ser devolvida à interface nem registrada em log. O endpoint local já existente nas configurações do Clovy MUST ser migrado como um endpoint nomeado, sem perda de configuração.

#### Scenario: Migração do endpoint local existente
- **WHEN** o Clovy inicia com `provider-settings.json` contendo `localGeneration` preenchido
- **THEN** a lista de provedores contém um endpoint com a mesma URL, modelo e chave, e as rotas que usavam o endpoint local continuam usando-o
- Test: unit — carga de settings legado e verificação do registro migrado

#### Scenario: Chave não volta à interface
- **WHEN** a interface lista os endpoints cadastrados
- **THEN** cada endpoint informa apenas se possui chave, sem o valor
- Test: unit — DTO serializado sem o campo de segredo

### Requirement: Teste de conexão e capacidade de saída estruturada
O sistema SHALL oferecer, para cada provedor, um teste de conexão que executa um prompt mínimo e mostra sucesso com latência ou o erro recebido, e SHALL registrar o nível de saída estruturada suportado (`none`, `prompt`, `json_object`, `json_schema`, `strict`). Recursos que exigem JSON estruturado MUST recusar provedores abaixo de `json_schema` com mensagem explicativa.

#### Scenario: Teste de conexão bem-sucedido
- **WHEN** o usuário aciona "Testar" em um provedor configurado e acessível
- **THEN** a interface mostra sucesso com a latência medida e o nível de saída estruturada detectado
- Test: integration — CLI falso e servidor HTTP falso respondendo ao prompt de teste

#### Scenario: Provedor sem saída estruturada suficiente
- **WHEN** o usuário escolhe para os recursos de atividade um endpoint cujo nível detectado é `prompt`
- **THEN** a escolha é recusada e a interface explica que é necessário suporte a JSON schema
- Test: unit — validação de seleção por nível de capacidade

### Requirement: Seleção de provedor por uso
O sistema SHALL permitir escolher o provedor separadamente para: chat/agente, geração de notas, limpeza de ditado e recursos de atividade (relatórios, resumo do dia, resumo de sessões de agentes). Clovy API MUST continuar disponível para chat, notas e ditado. Para recursos de atividade, o padrão MUST ser "nenhum", e sem provedor escolhido esses recursos ficam desligados, sem fallback para o Clovy API.

#### Scenario: Atividade sem provedor
- **WHEN** nenhum provedor de atividade foi escolhido e chega a hora de gerar o resumo do dia
- **THEN** nenhum texto de atividade sai da máquina e a interface mostra que o recurso precisa de um provedor configurado
- Test: integration — agendador com provedor "nenhum" não realiza chamada e registra estado desligado

#### Scenario: Nota gerada por CLI
- **WHEN** o provedor de notas é `claude` e uma gravação termina de ser transcrita
- **THEN** a nota estruturada é gerada pelo CLI e aparece no editor como nas notas geradas pelo Clovy API
- Test: integration — CLI falso devolvendo nota e verificação da nota persistida

### Requirement: Execução isolada de CLIs em geração pontual
Chamadas de geração pontual a CLIs SHALL rodar sem persistir sessão no CLI, sem ferramentas e sem carregar arquivos de contexto/skills, com tempo limite, e MUST marcar o prompt com o marcador de autoria do Clovy para que a ingestão de sessões de agentes as ignore. CLIs que não garantem todas as ferramentas desligadas (hoje `codex`, `agy`, `cursor-agent`, `copilot`) MUST ser recusados para notas, limpeza de ditado e atividade, sem redirecionamento silencioso; continuam disponíveis como motor do chat. Chamadas em segundo plano dos recursos de atividade MUST ser serializadas (uma por vez).

#### Scenario: CLI que mantém ferramentas
- **WHEN** o usuário tenta escolher `codex` para notas
- **THEN** a escolha é recusada com a explicação de que as ferramentas desse CLI não podem ser desligadas, e nenhum processo do `codex` é iniciado
- Test: unit — seleção por uso e invocação recusadas com CLI falso instalado

#### Scenario: Marcador de autoria presente
- **WHEN** o Clovy executa qualquer chamada de geração pontual por CLI
- **THEN** o prompt enviado contém o marcador de autoria do Clovy
- Test: unit — montagem do comando/prompt por provedor

#### Scenario: Tempo limite
- **WHEN** um CLI não responde dentro do tempo limite configurado
- **THEN** o processo é encerrado e a chamada falha com erro de tempo esgotado, sem processo órfão
- Test: integration — CLI falso que dorme além do limite

### Requirement: CLI como motor do chat
O sistema SHALL permitir escolher um CLI como motor de uma sessão de chat. Nessa sessão, cada mensagem do usuário MUST ser entregue ao CLI com continuidade da conversa anterior, e texto, raciocínio e atividade de ferramentas emitidos pelo CLI MUST aparecer na interface de sessão do Clovy e ser persistidos como itens da sessão. Cancelar MUST encerrar o processo do CLI. Quando o CLI aceitar servidores MCP, as ferramentas do Clovy MUST ser oferecidas pelo servidor MCP do Clovy; quando não aceitar, a interface MUST indicar que a sessão não tem ferramentas do Clovy. Mensagens enviadas durante uma execução MUST virar mensagem seguinte em vez de serem perdidas.

#### Scenario: Conversa com continuidade
- **WHEN** o usuário envia duas mensagens seguidas numa sessão com motor `claude`
- **THEN** a segunda execução retoma a mesma conversa do CLI e ambas as respostas aparecem em ordem na sessão, persistidas após reiniciar o app
- Test: integration — CLI falso que exige o identificador de retomada na segunda chamada

#### Scenario: Cancelamento
- **WHEN** o usuário cancela uma execução em andamento com motor CLI
- **THEN** o processo do CLI termina e a execução fica marcada como cancelada
- Test: integration — CLI falso de longa duração e verificação de término do processo

### Requirement: Endpoint OpenAI-compatível como modelo do agente
O sistema SHALL permitir usar qualquer endpoint cadastrado como modelo do agente do Clovy, mantendo as ferramentas do Clovy, streaming e direcionamento durante a execução.

#### Scenario: Agente com endpoint próprio
- **WHEN** o usuário escolhe um endpoint cadastrado como modelo do chat e pede uma tarefa que usa `search_june`
- **THEN** a resposta chega em streaming pelo endpoint e a chamada de ferramenta aparece na sessão
- Test: integration — servidor OpenAI-compatível falso com resposta de tool call

### Requirement: Notas com chave própria vão direto ao provedor
Com o motor de Notas = Clovy e a chave do provedor salva, o desktop SHALL mandar a transcrição de nota à Groq e a geração (modelo explícito, não Auto) e a limpeza da nota à OpenRouter, direto. A requisição direta MUST levar só a chave do provedor chamado e o mesmo contrato que o Clovy API manda. Sem a chave, a chamada MUST seguir pelo Clovy API como antes.

#### Scenario: Transcrição final com chave Groq
- **WHEN** a chave Groq está salva e um turno da reunião é transcrito
- **THEN** a requisição vai a `/audio/transcriptions` da Groq com modelo, `response_format=json`, idioma, prompt do dicionário e nome de arquivo canônico, e nenhuma requisição vai ao Clovy API
- Test: integration — `clovy_api_notes_tests.rs`, Groq e Clovy API falsos contam requisições e conferem o corpo multipart e os cabeçalhos

#### Scenario: Prévia ao vivo com chave Groq
- **WHEN** a chave Groq está salva e a prévia ao vivo envia um trecho
- **THEN** o trecho vai direto à Groq e nenhum campo de prévia do Clovy API (`preview`, `previewOptedIn`) vai junto
- Test: integration — `clovy_api_notes_tests.rs`, pedido com `preview: true` contra Groq falsa

#### Scenario: Falha transitória da Groq
- **WHEN** a Groq responde 503 na primeira tentativa e 200 na segunda
- **THEN** a transcrição devolve o texto da segunda resposta depois de exatamente duas requisições; um 400 não é repetido
- Test: integration — `clovy_api_notes_tests.rs`, Groq falsa com contador de chamadas

#### Scenario: Áudio sem fala
- **WHEN** a Groq devolve texto vazio
- **THEN** a transcrição falha com `no_speech`, que o pipeline trata como fonte sem fala
- Test: integration — `clovy_api_notes_tests.rs`

#### Scenario: Chave Groq rejeitada
- **WHEN** a Groq responde 401 ou 403
- **THEN** a transcrição falha com `groq_api_key_rejected` e a frase que manda atualizar a chave
- Test: integration — `clovy_api_notes_tests.rs`

#### Scenario: Áudio acima de 25 MB
- **WHEN** o arquivo do trecho tem mais de 26 214 400 bytes
- **THEN** a transcrição falha com `audio_too_large` sem nenhuma requisição
- Test: integration — `clovy_api_notes_tests.rs`, Groq falsa sem requisições

#### Scenario: Sem chave Groq
- **WHEN** não há chave Groq salva
- **THEN** a transcrição vai a `/v1/notes/transcribe` do Clovy API com o token local, como antes
- Test: integration — `clovy_api_notes_tests.rs`

#### Scenario: Geração com chave OpenRouter
- **WHEN** a chave OpenRouter está salva e o modelo de notas é explícito
- **THEN** a nota vem de uma chamada `/chat/completions` à OpenRouter com o prompt `note_generate.md`, a mensagem do usuário do servidor e os rótulos de fonte limpos, e nenhuma requisição vai ao Clovy API
- Test: integration — `clovy_api_notes_tests.rs`

#### Scenario: Geração com Auto ou sem chave
- **WHEN** o modelo é `open-software/auto` ou não há chave OpenRouter
- **THEN** a geração vai a `/v1/notes/generate` do Clovy API sem cabeçalho de chave
- Test: integration — `clovy_api_notes_tests.rs`

#### Scenario: Limpeza da transcrição da nota com chave OpenRouter
- **WHEN** a chave OpenRouter está salva e o pipeline limpa a transcrição com o estilo `note_transcript_cleanup`
- **THEN** a chamada vai à OpenRouter com o prompt `note_transcript_cleanup.md`, a mensagem do servidor para esse estilo e `temperature: 0`
- Test: integration — `clovy_api_notes_tests.rs`

### Requirement: Falha clara sem chave nem servidor
Quando não há a chave do provedor e o Clovy API não responde, a nota SHALL dizer, na língua da interface (pt-BR ou en), qual chave falta e onde salvá-la (Configurações > Modelos > Mais opções > Chaves de API de provedores), e MUST NOT mostrar o erro cru da biblioteca HTTP. O áudio fica salvo e "Tentar de novo" continua disponível.

#### Scenario: Transcrição sem chave Groq e sem servidor
- **WHEN** não há chave Groq e a conexão com o Clovy API é recusada
- **THEN** a transcrição falha com `groq_api_key_missing` e a frase "Add your Groq API key in Settings > Models > More options > Provider API keys to transcribe recordings.", que a nota mostra traduzida
- Test: integration — `clovy_api_notes_tests.rs` com porta fechada; unit — `note-failure-banner.test.tsx` traduz a frase em pt-BR

#### Scenario: Geração sem chave OpenRouter e sem servidor
- **WHEN** não há chave OpenRouter e a conexão com o Clovy API é recusada
- **THEN** a geração falha com `openrouter_api_key_missing` e a nota mostra a frase de transcrição salva que pede a chave OpenRouter
- Test: integration — `clovy_api_notes_tests.rs`; unit — `processing.rs` mapeia o código para a frase; `note-failure-banner.test.tsx`

### Requirement: Limite de taxa da Groq na transcrição de nota
Na transcrição direta à Groq, um 429 SHALL virar espera pelo tempo que a Groq informa, e não falha da nota. As requisições de transcrição final MUST respeitar um ritmo comum às duas faixas. Uma espera longa SHALL pausar a nota com o progresso salvo, mostrar quando ela continua e retomar sozinha; os turnos prontos MUST NOT ser reenviados.

#### Scenario: 429 com retry-after vira espera
- **WHEN** a Groq responde 429 com `retry-after` de alguns segundos e depois 200
- **THEN** o erro do 429 leva `retryAfterMs`, a transcrição do turno espera esse tempo, repete a mesma requisição e o turno sai com o texto, sem falha na nota
- Test: integration — `clovy_api_notes_tests.rs` (Groq falsa com cabeçalhos) e `processing.rs` (transcritor falso, relógio pausado)

#### Scenario: Limite longo pausa e diz quando continua
- **WHEN** a Groq responde 429 com `retry-after` acima de 90 s
- **THEN** a rodada para com `transcription_rate_limited`, a nota mostra "O Clovy continua a transcrição às HH:MM", os turnos não iniciados ficam pendentes e o fallback da faixa não roda
- Test: integration — `processing.rs`, stream com transcritor falso

#### Scenario: Nova tentativa só envia o que falta
- **WHEN** uma rodada pausada é retomada
- **THEN** os turnos já transcritos são reaproveitados e só os pendentes vão à Groq
- Test: integration — `processing.rs`, duas rodadas no mesmo banco contando requisições

#### Scenario: Job longo da faixa inteira termina
- **WHEN** uma gravação audível vira um job da faixa inteira que passa do áudio da hora e pausa antes do último pedaço
- **THEN** cada pedaço pronto fica salvo, a retomada só envia os que faltam, e a nota termina
- Test: integration — `processing.rs`, oito pedaços de 30 s contra 100 s por hora, banco em memória

#### Scenario: A pausa termina pelo relógio de parede
- **WHEN** a nota pausou e o Mac dormiu além do prazo, ou o relógio voltou
- **THEN** a pausa termina na hora de parede marcada e nunca se estende por um relógio que voltou
- Test: unit — `processing.rs`, `RequestPacer` com relógio injetável

#### Scenario: Pausa de uma chave não segura outra rota
- **WHEN** a conta da chave A está pausada
- **THEN** uma chave B e a rota do Clovy API sem chave seguem mandando, e a impressão digital da chave nunca contém a chave
- Test: unit — `processing.rs`

#### Scenario: Retomada e "Tentar de novo" ao mesmo tempo
- **WHEN** a retomada automática e um "Tentar de novo" disputam a mesma nota pausada
- **THEN** só um reivindica a pausa e a nota é gerada uma vez só
- Test: integration — `processing.rs` (reivindicação no banco), unit — `processing_queue.rs` (revalidação ao pegar o lock)

#### Scenario: Troca de modelo entre a pausa e a retomada
- **WHEN** o modelo de transcrição muda entre a pausa e a retomada
- **THEN** nenhum turno nem pedaço do outro modelo é reaproveitado, e uma sessão do modelo padrão de antes mantém a identidade dos seus turnos
- Test: unit/integration — `processing.rs`

#### Scenario: Chave trocada no meio do run
- **WHEN** a chave Groq salva muda entre duas requisições
- **THEN** cada requisição lê a rota uma vez, e o pacer e a chamada usam a mesma chave
- Test: unit — `processing.rs`

#### Scenario: Chave salva que o cofre não devolve
- **WHEN** há chave Groq salva nas configurações, mas o cofre não a devolve
- **THEN** a transcrição falha com `provider_api_key_unavailable` e a nota diz, em pt-BR ou en, para salvar a chave de novo; nada vai ao Clovy API e a prévia não envia nada
- Test: unit — `processing.rs`, `src/test/note-failure-banner.test.tsx`

#### Scenario: Retry suspenso enquanto a retomada termina
- **WHEN** o retry manual é suspenso antes de mostrar o estado e a retomada termina a nota
- **THEN** o retry não grava `transcribing` por cima da nota `ready` e, pulado, deixa a nota como a retomada a deixou
- Test: integration — `processing.rs` (banco em memória e tickets reais da fila)

#### Scenario: Ritmo dentro do limite por minuto
- **WHEN** as duas faixas têm muitos turnos para transcrever
- **THEN** nunca começam mais de 18 requisições em 60 s, somando as faixas e a prévia
- Test: unit — `processing.rs`, `RequestPacer` com relógio pausado

#### Scenario: Áudio da hora dentro do limite
- **WHEN** o áudio cobrado na última hora já chegou a 7.200 s
- **THEN** a próxima requisição espera a janela liberar; se a espera passa de 90 s, a nota pausa com a hora de retomada
- Test: unit — `processing.rs`, `RequestPacer` com orçamento de áudio

#### Scenario: A prévia não gasta o áudio da nota
- **WHEN** a prévia ao vivo já gastou 3.600 s de áudio na última hora, ou não há requisição livre no minuto, ou um 429 pausou a conta
- **THEN** o trecho da prévia é descartado sem esperar, e a transcrição final mantém o resto do orçamento
- Test: unit — `processing.rs`, `try_acquire`

#### Scenario: Fala da faixa em pacotes, sem silêncio
- **WHEN** a chave Groq está salva e uma faixa tem vários turnos de fala
- **THEN** os turnos seguidos vão juntos numa requisição de até 600 s com tempos de segmento, sem o silêncio entre eles, e cada turno volta com seu texto e seu job
- Test: integration — `processing.rs`, stream com transcritor de pacote falso; `clovy_api_notes_tests.rs`, `verbose_json` com segmentos contra Groq falsa

#### Scenario: Pedaços abaixo do limite de upload
- **WHEN** um pacote ou um WAV longo vai à Groq
- **THEN** cada requisição tem no máximo 600 s (19,2 MB em PCM16 mono a 16 kHz), abaixo dos 25 MB, e o corte fixo repete 10 s no começo do pedaço seguinte, sem duplicar as palavras no texto
- Test: unit — `audio/turns.rs` e `processing.rs`

#### Scenario: Mensagem traduzida
- **WHEN** a interface está em pt-BR e a nota guarda a frase de limite ou de pausa da Groq
- **THEN** o banner mostra a frase em pt-BR, com a hora na de pausa
- Test: unit — `src/test/note-failure-banner.test.tsx`
