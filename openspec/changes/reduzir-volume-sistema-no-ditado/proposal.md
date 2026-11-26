# Proposal

## Why

O som reproduzido pelo computador pode competir com a voz durante o ditado. A comparação com o Liv mostrou que ele silencia a saída global, enquanto o Clovy atualmente só silencia o canal de microfone de reuniões simultâneas.

## What Changes

- Adicionar em Ditado um controle macOS para reduzir temporariamente a saída do sistema a 20% do volume atual durante a captura, desligado por padrão.
- Restaurar o estado sob posse do Clovy ao parar/cancelar, sem aguardar transcrição ou aprimoramento; respeitar mudanças manuais e dispositivos sem controle de volume.
- Cobrir eventos duplicados/atrasados, falhas e encerramento normal sem interferir no microfone de reunião ou na transcrição.
- Registrar separadamente a comparação da transcrição crua; não trocar modelo, formato, idioma nem filtros de ASR nesta mudança.

## Capabilities

### New Capabilities

Nenhuma.

### Modified Capabilities

- `dictation`: redução opcional da saída do sistema durante a captura no macOS e restauração com posse por tomada/dispositivo.

## Impact

Configuração/DTO/comandos de ditado, bindings e interface de Configurações, ciclo de vida nativo macOS, testes e documentação do ditado. Implementação independente; nenhum código do Liv será copiado. Windows/Linux permanecem sem este controle; não adicionar dependências de mídia ou mudar áudio de reunião. Sem release, instalação, reinício ou teste com áudio real do usuário.
