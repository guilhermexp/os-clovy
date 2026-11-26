# Spec Delta

## ADDED Requirements

### Requirement: Redução opcional do volume do sistema no ditado macOS
No macOS, Configurações de Ditado SHALL oferecer redução da saída do sistema a 20% do volume atual enquanto a captura de ditado estiver ativa. A opção SHALL começar desligada e persistir sem alterar as demais preferências. Teste de microfone, transcrição de nota e outras plataformas MUST NOT acionar a redução.

#### Scenario: Opção habilitada
- **WHEN** a opção está ligada e a captura do ditado começa com volume de saída 50%
- **THEN** a saída controlável é reduzida a 10%, sem aguardar ou atrasar o início da captura

#### Scenario: Padrão e teste de microfone
- **WHEN** a opção não foi habilitada ou somente um teste de microfone é iniciado
- **THEN** o volume do sistema permanece inalterado

#### Scenario: Erro de persistência
- **WHEN** salvar a preferência falha
- **THEN** a interface mantém a preferência confirmada anterior e apresenta o erro, sem sobrescrever idioma, modelo, estilo ou microfone

### Requirement: Restauração do volume sob posse da tomada
O Clovy SHALL restaurar o volume anterior da saída que alterou ao encerrar a captura, cancelar, falhar, desabilitar a opção, perder o helper ou encerrar normalmente. A restauração MUST respeitar intervenção manual, saída previamente muda, identidade de dispositivo e eventos atrasados/duplicados. Somente a tomada que detém a alteração SHALL liberá-la.

#### Scenario: Parada antes da transcrição
- **WHEN** a captura acaba e a transcrição ou o aprimoramento ainda estão em andamento
- **THEN** a redução é encerrada imediatamente, sem aguardar o provedor

#### Scenario: Mudança manual
- **WHEN** o usuário muda volume ou mute durante a redução
- **THEN** sua escolha prevalece e o Clovy não a sobrescreve ao restaurar

#### Scenario: Troca de saída
- **WHEN** o dispositivo padrão muda durante a captura
- **THEN** o Clovy não restaura o volume de um dispositivo sobre outro nem altera uma saída sem ter capturado seu estado anterior

#### Scenario: Eventos antigos e encerramento
- **WHEN** chegam eventos repetidos ou de outra tomada, ou o helper termina inesperadamente
- **THEN** eventos antigos não desfazem a redução de uma tomada nova e a perda real do helper libera as alterações que ainda pertencem ao Clovy

### Requirement: Falha do controle de saída não impede o ditado
Falhas ou ausência de controle de volume do dispositivo SHALL preservar a captura, a transcrição, a colagem e o áudio de reunião. O Clovy MUST NOT mudar de dispositivo, aumentar volume, desmutar uma saída ou iniciar rede para contornar esse limite. Encerramento forçado do processo principal sem execução de cleanup não SHALL ser apresentado como restauração garantida.

#### Scenario: Saída sem volume ajustável
- **WHEN** a saída é HDMI ou outro dispositivo sem controle gravável de volume
- **THEN** o ditado continua e nenhuma outra saída é alterada

#### Scenario: Falha ao reduzir ou restaurar
- **WHEN** o sistema recusa a operação de volume
- **THEN** o erro permanece limitado ao controle da saída, sem perder o texto nem interromper o microfone da reunião
