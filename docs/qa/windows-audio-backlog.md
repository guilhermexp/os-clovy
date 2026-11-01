# Pendências de áudio de reunião no Windows

## Escopo e evidência

Registro solicitado pelo dono em 2026-11-01, antes de implementar as melhorias de Dock, ícones do histórico de ditado e contexto de ditado nos resumos. Relato: texto da fala e trechos da transcrição da reunião ficaram estranhos no Windows. A causa da gravação relatada ainda não foi demonstrada. Não atribuir o problema ao Groq nem à ausência do Parakeet sem evidência.

Esta lista registra trabalho pendente, não correções entregues. Não altera o escopo das changes já propostas e não autoriza implementação. As três melhorias anteriores continuam pendentes, sem mudança.

## Correções identificadas

- [ ] **F1: permitir controlar o áudio do sistema no Windows.** O estado inicia com `userWantsSystemAudio=true`; com System pronto e concedido, seleciona `microphonePlusSystem`. Porém, o painel e o botão de opções são condicionados a `isMacLikePlatform()`, escondendo o controle no Windows. Evidência por inspeção: `src/app/use-app-state.ts:173-197`, `src/components/note-editor/NoteEditor.tsx:347,761-779,872` e `src/lib/platform.ts:62-67`. Aceite: com captura System disponível no Windows, o usuário consegue alternar entre microfone sozinho e microfone + sistema; conferir modo solicitado e os arquivos das fontes em uma gravação sintética nativa. Ainda sem reprodução visual no Windows.

- [ ] **F2: reconciliar preview de áudio descartado no processamento definitivo.** Se o final elimina um turno Microphone por ser eco do System, o preview daquele turno não encontra transcrição definitiva da mesma fonte e permanece visível ao lado do System final. Reproduzido em 2026-11-01 com Node importando o módulo real `src/lib/live-transcript-preview.ts`, sem dependências de frontend ou rede, com exit 0; o controle de sobreposição na mesma fonte remove o preview. O editor concatena finais e previews em `src/components/note-editor/NoteEditor.tsx:293-311`. O problema é da reconciliação visual e não prova duplicação na nota gerada. Aceite: representar o descarte definitivo e remover somente os previews superados/descartados da sessão e intervalo correspondentes; preservar previews de sessões em andamento e trechos sem resultado por falha retryable. Criar regressão incluindo esses limites e conferir a tela ao finalizar uma gravação com eco.

- [ ] **F3: excluir a pausa do silêncio acrescentado ao encerrar System.** No loop Windows, a pausa mantém `last_written_at` inalterado; ao parar sem retomar, o padding final usa o tempo até `Instant::now()`, incluindo a pausa. Evidência por inspeção: `src-tauri/src/audio/system_windows.rs:244-288,783-793`; `stop` sinaliza encerramento sem retomar em `:123-125`. Ainda não reproduzido com WASAPI. Aceite: primeiro reproduzir em Windows com gravação sintética, pausa longa e stop; o WAV System deve excluir a pausa, assim como o microfone e o relógio da sessão. Cobrir também retomada e silêncio real depois de retomar antes de corrigir.

## Investigações pendentes, sem correção presumida

- [ ] **F4: verificar saída da reunião versus endpoint capturado.** O loopback usa `GetDefaultAudioEndpoint(eRender, eConsole)` no início, em `src-tauri/src/audio/system_windows.rs:527-537`. Reunião em outra saída ou troca de dispositivo pode explicar ausência de fala, mas não é causa confirmada nem decisão de ampliar suporte. Comparar saída da reunião, padrão Console e padrão Communications na máquina afetada; só propor mudança de seleção/acompanhamento após reproduzir.

- [ ] **I1: localizar a origem dos trechos estranhos da transcrição definitiva.** Comparar uma gravação curta sintética com preview desligado e ligado, primeiro microfone, depois System, depois as duas fontes. Inspecionar metadados dos WAVs, validação, descarte por silêncio/eco, spans dos jobs e texto definitivo por fonte. Distinguir erro já presente no áudio, processamento/corte/atribuição, resposta ASR e resíduo visual F2. Sem áudio ou sessão diagnóstica da instalação afetada, não há causa comprovada para o conteúdo estranho. Não exportar gravações/transcrições privadas nem chaves por padrão.

## Limites da investigação

- A seleção compartilhada de backend transforma uma escolha Local em Groq/Whisper na plataforma não suportada; não foi encontrado caminho normal que use Parakeet para uma fonte e Groq para outra. Isso não prova qualidade de reconhecimento nem captura nativa.
- Não foi executada nova reunião no Windows, suíte completa, chamada ao Groq ou teste de reconhecimento de fala real nesta investigação.
- Não apagar previews indiscriminadamente ao terminar: a lógica atual preserva texto útil quando o processamento final falha. A correção de F2 precisa distinguir descarte definitivo de ausência de resultado.
- Portar o modelo local ao Windows não está nesta lista e não decorre dos achados.
