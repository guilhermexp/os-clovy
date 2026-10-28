# Spec Delta

## Purpose

Define o tema com que o Clovy abre e o fundo desenhado atrás das telas de conversa (Nova sessão e Início), incluindo a escolha do fundo pelo usuário e o comportamento do fundo animado.

## ADDED Requirements

### Requirement: Tema inicial escuro
Sem preferência de tema salva, o app SHALL abrir em modo escuro já no primeiro quadro, sem passar pelo claro. Uma preferência salva (claro, escuro ou sistema) MUST continuar valendo como antes.

#### Scenario: Primeira abertura
- **WHEN** o app abre sem a chave `os-clovy:theme` no armazenamento local, num sistema em modo claro
- **THEN** a janela principal abre com `data-theme="dark"` antes do primeiro paint e o seletor de tema em Aparência mostra "Escuro"
- Test: unit — `getStoredTheme`/`resolveTheme` sem chave salva; unit — script de `index.html` executado em jsdom sem chave

#### Scenario: Preferência salva
- **WHEN** o usuário salvou "Claro" e reabre o app
- **THEN** o app abre em modo claro
- Test: unit — leitura da preferência salva

### Requirement: Telas de conversa sem lavagem colorida
A Nova sessão, o Início e a etapa de personalidade do onboarding MUST NOT pintar gradiente na cor da marca atrás do conteúdo. Com o fundo "Nenhum", essas telas mostram só a superfície neutra do tema.

#### Scenario: Nova sessão com fundo Nenhum
- **WHEN** o usuário abre a Nova sessão em modo escuro com o fundo "Nenhum"
- **THEN** a área atrás de "No que vamos trabalhar?" e do composer tem a cor da superfície do tema, sem faixa azul ou verde-água em nenhuma altura
- Test: unit — regras CSS de `.agent-workspace` sem `--hero-wash` nem `--brand`; visual — capturas claro e escuro

### Requirement: Escolha do fundo
Configurações → Aparência SHALL oferecer a linha "Fundo" com as opções "Drift" e "Nenhum", com "Drift" como padrão. A escolha MUST ficar salva entre reinícios e MUST valer na hora, sem reiniciar o app.

#### Scenario: Trocar para Nenhum
- **WHEN** o usuário escolhe "Nenhum" com a Nova sessão aberta em outra aba
- **THEN** o desenho some da Nova sessão sem recarregar a janela, e continua sem desenho após reiniciar o app
- Test: unit — módulo de preferência (padrão, valor inválido, evento de mudança); component — fundo desmontado ao receber o evento

### Requirement: Fundo Drift
O fundo "Drift" SHALL desenhar o preset `drift` do asciify.org/backgrounds com os controles do dono: marca `dot`, tinta `#8f8d86`, velocidade 1, célula 11, opacidade 1, cena `flow`, rastro ao passar o ponteiro. O desenho MUST ficar atrás do conteúdo da Nova sessão e do Início, com área livre atrás do título e do composer, e MUST NOT receber clique, foco nem leitor de tela.

#### Scenario: Conteúdo por cima do desenho
- **WHEN** a Nova sessão abre com "Drift"
- **THEN** o título, o composer e as sugestões ficam legíveis sobre a área livre, clique e teclado chegam a eles, e o desenho está marcado `aria-hidden`
- Test: component — kit simulado recebe `preset: drift`, `mark: 'dot'` e o elemento `clear`; visual — capturas claro e escuro, janela larga e janela estreita (390 px)

### Requirement: Ciclo de vida do fundo
Cada tela SHALL ter no máximo um desenho do fundo ativo, destruído ao sair da tela ou ao trocar para "Nenhum". O desenho MUST pausar quando a janela está oculta ou o elemento fora da tela, MUST mostrar um quadro parado com movimento reduzido, e falha de carregamento ou falta de WebGL2 MUST deixar o fundo liso sem erro visível.

#### Scenario: Movimento reduzido
- **WHEN** o sistema pede movimento reduzido e a Nova sessão abre com "Drift"
- **THEN** o fundo mostra um quadro parado e não anima
- Test: visual — emulação de `prefers-reduced-motion: reduce`

#### Scenario: Falha ao carregar
- **WHEN** o módulo do fundo falha ao carregar ou o WebGL2 não está disponível
- **THEN** a Nova sessão funciona com fundo liso e nenhum erro aparece para o usuário
- Test: component — import do kit rejeitado; visual — WebGL desligado

#### Scenario: Limpeza
- **WHEN** o usuário sai da Nova sessão para uma nota e volta duas vezes
- **THEN** existe um único desenho ativo e os anteriores foram destruídos
- Test: component — `destroy` chamado uma vez por montagem; desmontagem antes do import terminar destrói o handle ao chegar
