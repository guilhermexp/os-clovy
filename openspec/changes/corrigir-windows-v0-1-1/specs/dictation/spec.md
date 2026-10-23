## ADDED Requirements

### Requirement: Atalhos do ditado no Windows
No Windows, o usuário SHALL poder salvar como atalho uma combinação de Ctrl/Alt/Shift/Win com uma tecla, ou uma tecla sozinha que o Windows enxerga: Ctrl direito, Alt direito (AltGr), Shift direito, F13–F24, Pause, Scroll Lock, Insert e afins. Uma tecla modificadora sozinha MUST disparar só quando apertada e solta sem outra tecla no meio, para não atrapalhar o uso normal dela. O atalho padrão do ditado no Windows SHALL ser Ctrl direito. Teclas que o Windows não enxerga (como Fn na maioria dos teclados) não podem ser atalho: quando a captura termina sem tecla detectada, a tela MUST dizer que a tecla apertada não chega ao Windows. Todo erro de validação ou de registro do atalho MUST aparecer na tela e continuar visível até a próxima ação do usuário.

#### Scenario: Combinação personalizada
- **WHEN** o usuário grava Ctrl+Shift+D para o ditado no Windows
- **THEN** o atalho é salvo, registrado e passa a iniciar o ditado
- Test: unit — validação e persistência; manual — Windows de prova

#### Scenario: Ctrl direito sozinho
- **WHEN** o atalho é Ctrl direito e o usuário aperta e solta Ctrl direito sem outra tecla
- **THEN** o ditado começa; Ctrl direito+C continua copiando sem iniciar o ditado
- Test: unit — máquina de estados do gancho de teclado com sequências de teclas

#### Scenario: Tecla invisível ao Windows
- **WHEN** o usuário aperta só Fn durante a captura e nenhuma tecla chega ao Windows
- **THEN** ao fim da captura a tela explica que essa tecla é tratada pelo teclado e o Windows não a vê, e sugere Ctrl direito
- Test: unit — fim de captura sem tecla mostra a mensagem

#### Scenario: Erro de registro visível
- **WHEN** o Windows recusa o atalho porque outro app já o usa
- **THEN** a mensagem aparece na linha do atalho e não some com o fim da captura
- Test: unit — eventos de cancelamento não apagam o erro
