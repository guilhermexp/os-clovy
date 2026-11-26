# Design

## Context

Base Clovy `031b7436`; Liv consultado em `4aad305e` com WIP preservado. As auditorias somente leitura encontraram diferenças de modelo, idioma, codec, filtros e retries, registradas no relatório da comparação. Esta entrega implementa apenas a redução de saída pedida, sem inferir ganhos de precisão nem copiar código sem licença localizada.

O fluxo `emit_dictation_event_value` já coordena eventos do helper e o silêncio do canal de microfone de reuniões (`sync_capture_mic_duck`). Esse recurso é independente e permanece intacto. `listening_started` chega após o recorder iniciar; parada gera eventos de finalização antes da chamada de ASR. O host Rust já usa FFI CoreAudio em `meeting_detection.rs` e permanece vivo para tratar morte do helper.

## Goals / Non-Goals

**Goals:** controle opt-in macOS, redução relativa a 20%, restauração por tomada/dispositivo e isolamento de erros; UI coerente com as preferências confirmadas do Ditado.

**Non-Goals:** alterar ASR, baixar modelos, pausar players, mute total, volume por aplicativo, suporte Windows/Linux, histórico em disco de volumes, recuperação após SIGKILL do host, inferência paga, reprodução real na máquina do dono ou release.

## Decisions

### Controle independente no host

Criar módulo `src-tauri/src/dictation_output_duck.rs` com estado portável, adapter CoreAudio sob cfg macOS e executor serial. Expor API interna de iniciar/liberar/reconciliar e manter a posse por take ID. Integrar em `dictation.rs` no funil de eventos, `supervise_helper_exit` e `stop_helper`; não mudar o protocolo Swift nem duplicar posse entre processos. Enfileirar efeitos fora do caminho crítico da captura; nenhuma invocação de shell/AppleScript.

### Estado e propriedade de dispositivos

Capturar a identidade estável e os volumes originais do dispositivo padrão, suporte/gravação e mute observado. Preferir volume master quando gravável; recorrer a canais somente com snapshot completo dos canais que serão alterados. Aplicar `original * 0.2`, sem elevar volume nem desmutar; saída já muda pode permanecer sem alteração.

Serializar início, fim, desabilitação e mudanças de device; início duplicado não recaptura o estado reduzido. Fim stale não libera tomada nova. Preservar canais alterados parcialmente para rollback em falhas. Ao restaurar, comparar estado com o último valor imposto confirmado, tolerando somente ruído numérico; se houve intervenção externa em volume/mute, não escrever o snapshot por cima. Eventos de propriedades CoreAudio permitem reconciliar estado, mas não identificam autoria de uma escrita; manter lógica testável com adapter fake.

Revisão da candidata884addf: o SET pode ser assíncrono e quantizar o alvo, sem token de autoria. Não há contrato público de normalização antecipada nos conversores scalar/dB. Portanto, nunca adotar qualquer leitura menor como nossa só porque ocorreu imediatamente após SET, nem declarar restaurado ao abandonar uma escrita ambígua. SET cria pendência; somente readback/listener compatível com o alvo confirma a imposição. Divergência desconhecida entre valor solicitado e observado é falha de confirmação: manter resultado explícito não confirmado/aviso, sem elevar volume para resolver a ambiguidade. Aplicar a mesma regra na restauração e no timeout do teardown. Essa regra implementa o tratamento de falha já previsto; não é aceite da candidata nem garantia universal em dispositivos quantizados.

Na troca de saída, liberar a saída anterior quando ainda sob posse; não levar seu volume para a saída nova. Reconciliar a captura ativa no novo device apenas após obter snapshot próprio, sem alterar dispositivo indisponível. Se o device antigo desconectar, não escrever em outra saída e registrar limite da restauração. Restore falho não é sucesso silencioso nem motivo para parar ditado.

### Preferência e UI

Adicionar `reduceSystemVolume` booleano ao snapshot de Ditado (serde default false), DTO e setter Tauri dedicado. Persistir sob o lock existente antes de publicar; erro mantém estado confirmado e não muda outros campos. Reutilizar o guard de sessão e merge de campo de `AppSettings.tsx`. Em Ditado > Microfone, usar o Toggle e linha de configurações existentes com texto pt/en: "Reduzir volume do sistema durante o ditado" e descrição de redução para 20% do volume atual/restauração ao parar. Exibir apenas no macOS. Desligar libera a redução atual; ligar durante uma captura vale para a próxima tomada, evitando capturar um snapshot tardio inesperado.

O padrão 20% foi informado como suposição baseada no pedido de abaixar, após formulário opcional sem resposta; não é uma escolha atribuída ao usuário. Se ele escolher silenciamento, revisar os artefatos antes de aplicar essa variante.

## Risks / Trade-offs

- CoreAudio e saída sem controle: falha best-effort, sem mudar device nem bloquear ditado; testes de HAL com fake não provam hardware real.
- Mudança manual no exato valor imposto e quantização assíncrona podem ser indistinguíveis de uma escrita nossa; callbacks não carregam autoria. Observar propriedades sem inventar autoria; caso ambíguo retorna restauração não confirmada, nunca sucesso falso ou aumento arbitrário do volume.
- Crash/SIGKILL do host não executa cleanup: não garantir restauração nesse caso e não criar daemon/persistência automática fora do escopo.
- Build Linux não cobre HAL: suíte portável na box, compilação/Clippy/testes integrais macOS via workflow do projeto em SHA candidato, sem app do dono.
- Licença Liv não localizada: implementação independente baseada no comportamento solicitado.

## Migration Plan

Ausência da preferência conserva o comportamento antigo. Não migrar perfis reais manualmente. Rollback desliga a preferência; preserva campos desconhecidos e não altera banco. Destino autorizado é entrega de código revisada/integrada, sem nova tag ou release.
