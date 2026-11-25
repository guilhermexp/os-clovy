# Spec Delta

## MODIFIED Requirements

### Requirement: Disponibilidade do motor local por plataforma
O motor Local SHALL oferecer exatamente Whisper large-v3-turbo, Parakeet Ultra, Fluid Speech Pro e Phonon-2.5 em Macs Apple Silicon compatíveis. Cada modelo MUST informar sua exigência de macOS e ficar indisponível quando incompatível. Groq e OpenRouter SHALL continuar disponíveis nas plataformas suportadas pelo app; preferências locais legadas em plataforma incompatível preservam a migração existente para Groq.

#### Scenario: Mac com Apple Silicon
- **WHEN** o usuário abre Configurações > Modelos num Mac com Apple Silicon e macOS 14+
- **THEN** a linha de Transcrição oferece Groq, OpenRouter e os modelos locais compatíveis

#### Scenario: Windows
- **WHEN** o usuário abre Configurações > Modelos no Windows
- **THEN** a linha de Transcrição oferece Groq e OpenRouter e nenhum controle de instalação de modelo aparece

#### Scenario: Configuração salva com Local em plataforma sem suporte
- **WHEN** `provider-settings.json` tem `transcriptionProvider: "local"` e o app abre numa plataforma sem suporte
- **THEN** o motor efetivo é Groq e a interface mostra Groq selecionado

Test: unit (Rust) — sanitização por plataforma; unit (frontend) — linha de Transcrição com flag de plataforma.

#### Scenario: Requisito específico de sistema
- **WHEN** o usuário está num Mac compatível com Ultra mas incompatível com outro modelo do catálogo
- **THEN** Ultra continua utilizável e o outro modelo informa o requisito sem iniciar download
- Test: unit — elegibilidade por modelo e sistema.

### Requirement: Instalação do modelo com progresso
Configurações SHALL instalar individualmente o modelo local escolhido, por revisão fixa, com progresso em bytes, cancelamento e retomada de arquivos íntegros. Cada arquivo MUST ter SHA-256 verificado antes de ficar pronto. Selecionar um modelo ausente inicia seu download e só o ativa depois da instalação e da gravação bem-sucedidas. Falha ou cancelamento MUST preservar a seleção efetiva anterior e explicar como tentar novamente.

#### Scenario: Instalação completa
- **WHEN** o usuário clica "Instalar modelo" e o download termina com todos os arquivos íntegros
- **THEN** o estado passa a "pronto", o tamanho ocupado aparece ao lado e o motor Local fica selecionável

#### Scenario: Progresso visível
- **WHEN** o download está em andamento
- **THEN** a interface mostra os bytes recebidos sobre o total e um botão "Cancelar"; o botão "Instalar modelo" não aparece

#### Scenario: Cancelamento
- **WHEN** o usuário clica "Cancelar" durante o download
- **THEN** o download para, o estado volta a "ausente" e nenhuma transcrição é afetada

#### Scenario: Arquivo corrompido
- **WHEN** um arquivo baixado tem SHA-256 diferente do esperado
- **THEN** o estado passa a "falhou" com o nome do arquivo no motivo, o botão "Tentar de novo" aparece, e a nova tentativa baixa só o que falta ou divergiu

#### Scenario: Sem rede
- **WHEN** o download falha por rede
- **THEN** o estado passa a "falhou" com mensagem de rede na língua da interface e "Tentar de novo" retoma do que já foi baixado

Test: integration (Rust) — servidor HTTP falso com arquivos de tamanho conhecido: sucesso, hash divergente, cancelamento e retomada; unit (frontend) — estados ausente/baixando/pronto/falhou da linha.

#### Scenario: Instalar outro modelo mantendo o atual
- **WHEN** o modelo A está ativo e o usuário escolhe instalar B
- **THEN** A permanece efetivo até B estar íntegro e a nova seleção ser salva; erro de download ou gravação conserva A
- Test: integration — downloader e persistência isolados; unit frontend — fluxo completo.

### Requirement: Remoção do modelo
Configurações SHALL permitir remover cada modelo instalado ou seus arquivos parciais sem apagar os demais modelos nem dados pessoais. Remover um modelo inativo MUST preservar o ativo. Remover o ativo exige ação explícita que informa a troca para Groq antes da confirmação. Remoção MUST liberar o processo e caches exclusivos daquele modelo, recusar conflito com transcrição em andamento e mostrar espaço realmente ocupado.

#### Scenario: Remover com Local escolhido
- **WHEN** o motor é Local e o usuário confirma remover o modelo ativo após ler que passará a Groq
- **THEN** os arquivos são apagados, o motor passa a Groq e a interface avisa que as transcrições voltam a usar a Groq

#### Scenario: Remover com Groq escolhido
- **WHEN** o motor é Groq e o usuário remove o modelo
- **THEN** os arquivos são apagados e o motor continua Groq

Test: integration (Rust) — remoção limpa o diretório e grava a configuração.

#### Scenario: Remover modelo de teste inativo
- **WHEN** A está ativo e o usuário remove B
- **THEN** somente B e seus caches exclusivos são removidos, A continua selecionado e funcional
- Test: integration — duas pastas temporárias e helper falso.

#### Scenario: Remoção durante uso
- **WHEN** o modelo está atendendo uma transcrição e o usuário solicita removê-lo
- **THEN** a remoção é recusada com motivo até a operação terminar e seus arquivos permanecem íntegros
- Test: integration — transcrição bloqueada em fixture e tentativa concorrente de remoção.

### Requirement: Estado do modelo persistente
O estado de cada modelo SHALL ser derivado de seus arquivos gerenciados ao abrir o app e consultável por identidade. Arquivos completos e íntegros significam pronto; instalação interrompida permite retomada. Downloads e eventos de um modelo MUST NOT alterar o estado de outro. A instalação existente do Parakeet Ultra MUST ser reconhecida sem novo download e sem apagar arquivos íntegros.

#### Scenario: Reabrir com modelo pronto
- **WHEN** o app abre e o diretório do modelo está completo
- **THEN** Configurações mostra "pronto" sem baixar nada

#### Scenario: Reabrir após download interrompido
- **WHEN** o app fecha no meio do download e reabre
- **THEN** Configurações mostra "ausente" com "Instalar modelo", e a instalação reaproveita os arquivos íntegros já baixados

Test: integration (Rust) — diretório parcial e completo em pasta temporária.


### Requirement: Escolha do motor Local exige modelo pronto
A seleção efetiva do motor Local SHALL exigir que o modelo específico escolhido esteja pronto e seja compatível. O seletor permite escolher modelos ausentes para instalar, mas MUST preservar o motor anterior até terminar. Uma gravação direta de configuração com modelo ausente MUST falhar sem alterar a configuração anterior. Troca posterior de escolha MUST impedir que o término de um download antigo ative o modelo errado.

#### Scenario: Seleção sem modelo
- **WHEN** o modelo está ausente e o usuário abre a escolha de motor
- **THEN** os modelos locais aparecem com ação de instalação e a seleção efetiva continua inalterada

#### Scenario: Gravação recusada
- **WHEN** algo tenta gravar o motor Local sem modelo pronto
- **THEN** a gravação falha com `local_model_not_installed` e a configuração anterior permanece

Test: unit (Rust) — `set_provider_model` com estado do modelo falso; unit (frontend) — seleção de modelo ausente inicia instalação sem mudar o motor efetivo antes da gravação bem-sucedida.

#### Scenario: Escolha muda durante download
- **WHEN** B está baixando e o usuário escolhe A ou um provedor remoto antes de B terminar
- **THEN** terminar o download de B não muda a escolha mais recente
- Test: unit frontend e integração de configuração — respostas fora de ordem.

### Requirement: Transcrição local sem rede
Com o motor Local escolhido, ditado, notas e prévia ao vivo SHALL usar o modelo local selecionado e sua identidade específica. Nenhuma requisição de transcrição MUST sair da máquina, nenhuma chave remota MUST ser exigida e limites exclusivos de provedores remotos MUST NOT se aplicar. Falha local MUST NOT trocar de provedor. Downloads e verificações de instalação são ações distintas da inferência local.

#### Scenario: Ditado sem chave Groq
- **WHEN** o motor é Local, não há chave Groq salva e o usuário dita
- **THEN** o texto transcrito localmente é colado e nenhuma requisição de transcrição sai da máquina

#### Scenario: Nota longa
- **WHEN** o motor é Local e uma gravação de 40 minutos termina
- **THEN** todos os trechos são transcritos localmente, sem pausa por limite de taxa, e a nota é gerada

#### Scenario: Turnos locais sem mistura entre turnos
- **WHEN** o motor é Local e a transcrição final processa turnos de uma nota
- **THEN** cada turno é enviado numa requisição local individual e recebe somente o texto dessa requisição
- **AND** o caminho Local não agrega turnos em pacotes, evitando que tempos imprecisos e a agregação de tokens por pausa de 400 ms atribuam texto a outro turno
- **AND** o caminho Groq mantém seus pacotes com intervalo fixo de 500 ms entre turnos e divisão pelos tempos dos segmentos; essa regra não é alterada pelo motor Local

#### Scenario: Prévia ao vivo
- **WHEN** o motor é Local, a prévia ao vivo está ligada e um trecho fica pronto
- **THEN** o trecho é transcrito localmente e aparece na prévia, sem orçamento de segundos por hora

#### Scenario: Idioma do ditado
- **WHEN** o idioma do ditado está configurado como português
- **THEN** o modelo local recebe a dica de idioma e devolve o texto em português

Test: integration (Rust) — transcritor local falso nos caminhos de ditado, nota, turnos individuais e prévia, com servidor sentinela de rede recebendo zero requisições; regressão com três turnos e tempos imprecisos confere atribuição individual e preserva o intervalo de 500 ms dos pacotes Groq; unit — pacer não é consultado com motor Local.

#### Scenario: Quatro motores com português
- **WHEN** cada um dos quatro modelos é instalado e selecionado numa plataforma compatível
- **THEN** uma fixture de fala em português produz texto por aquele modelo, sem chamada de transcrição remota
- Test: native — quatro inferências com áudio sintético público em diretório isolado, sem microfone do dono.

### Requirement: Falha do motor local é erro claro
Falha ao iniciar o helper, carregar o modelo escolhido ou transcrever SHALL produzir erro local claro e traduzido com identificação do modelo e recuperação por reinstalação ou troca explícita. A gravação de nota permanece disponível para tentar novamente. O sistema MUST NOT enviar o áudio para outro modelo ou provedor nem colar resultado falso.

#### Scenario: Helper não inicia
- **WHEN** o motor é Local e o helper não responde ao carregar o modelo
- **THEN** a nota mostra a mensagem do motor local, o áudio fica salvo e nenhuma requisição à Groq é feita

#### Scenario: Ditado com falha local
- **WHEN** o motor é Local e a transcrição de uma tomada falha
- **THEN** o HUD mostra que o modelo local falhou e nada é colado

Test: integration (Rust) — transcritor local falso que falha; unit (frontend) — `note-failure-banner` traduz a frase.


### Requirement: Modelo carregado uma vez por sessão
O helper local SHALL manter carregado o modelo ativo para reutilizar entre tomadas. Trocar de modelo ou sair de Local MUST liberar o modelo anterior, respeitando operações em andamento. A identidade do modelo e do provedor SHALL ser capturada pela operação para que uma troca nas Configurações não redirecione uma transcrição já iniciada.

#### Scenario: Segunda tomada
- **WHEN** o usuário dita duas vezes seguidas com o motor Local
- **THEN** a segunda tomada não recarrega o modelo e o texto é colado em menos tempo que a primeira

#### Scenario: Troca para Groq
- **WHEN** o usuário troca o motor de Local para Groq
- **THEN** o helper local é encerrado

Test: integration (Rust) — cliente do helper com processo falso contando cargas; manual — `dictation-events.log` com duas tomadas.

#### Scenario: Tomada mantém sua identidade
- **WHEN** uma transcrição foi iniciada com A e a configuração muda para B
- **THEN** a operação de A termina com A, sem liberar arquivos ainda em uso nem reutilizar seu resultado como sendo de B
- Test: integration — helper correlacionado e dois modelos.
