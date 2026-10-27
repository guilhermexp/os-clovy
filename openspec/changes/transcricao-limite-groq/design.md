# Design

## Context

A transcrição final de uma nota é feita por turno: a detecção por energia (`audio/turns.rs`) acha os trechos de fala de cada faixa, `coalesce_turns_for_transcription_avoiding_echo` junta turnos da mesma faixa separados por até 2,5 s, e cada turno vira um job durável (`note_transcription_jobs`) e uma requisição à Groq. Silêncio já é pulado: a faixa de sistema em silêncio sai antes (`partition_silent_system_sources`), e cada pedaço passa por `source_is_effectively_silent` antes de ir à Groq. O stream (`transcribe_prepared_turn_stream`) roda até 2 turnos ao mesmo tempo, um por faixa.

Limites da Groq (docs `console.groq.com/docs/rate-limits` e `speech-to-text`, consultadas em 2026-10-27), iguais para `whisper-large-v3` e `whisper-large-v3-turbo` na tabela base: 20 RPM, 2.000 RPD, 7.200 ASH (segundos de áudio por hora), 28.800 ASD. Mínimo cobrado de 10 s por requisição. Upload de até 25 MB no gratuito. Todo 429 traz `retry-after` em segundos; `x-ratelimit-reset-requests` vem em formato como `2m59.56s`. Não há cabeçalho de RPM para áudio.

## Goals / Non-Goals

**Goals:**

- Um 429 vira espera, não falha da nota.
- Limite longo: pausa com progresso salvo, hora de retomada visível e retomada automática.
- Ritmo dentro de 20 RPM somando as duas faixas.
- Menos requisições e menos segundos cobrados por reunião.
- Mensagem traduzida.

**Non-Goals:**

- Mudar o Clovy API ou o caminho sem chave.
- Mudar a prévia ao vivo e o ditado (o ritmo vale só para a transcrição final da nota).
- Persistir progresso dentro de um pedaço do fallback da faixa inteira.
- Ler o plano do usuário para acelerar além de 18 RPM.
- Mostrar a espera curta (até 90 s) na barra de progresso.

## Decisions

### D1. A espera vem do cabeçalho

`send_direct` lê, num 429, `retry-after` (segundos, inteiro ou decimal). Sem ele, usa `x-ratelimit-reset-requests` quando `x-ratelimit-remaining-requests` é `0`. O valor vai no erro como `details.retryAfterMs`, o mesmo campo que o caminho pelo Clovy API já usa. Se a espera é de até 2 s, `send_direct` espera esse tempo e faz a nova tentativa de sempre; senão devolve o erro na hora, porque repetir antes do `retry-after` só gasta outra requisição. Sem cabeçalho nenhum, mantém a nova tentativa de 300 ms. Vale para todo `send_direct` (geração e limpeza na OpenRouter também), o que só melhora esses caminhos.

### D2. Um ritmo para o processo inteiro

`RequestPacer` em `processing.rs`: janela deslizante de 60 s com no máximo 18 inícios de requisição (2 de folga para a prévia e o ditado, que usam a mesma conta Groq) e um `paused_until` comum. Um único pacer estático serve todas as notas e as duas faixas. Um 429 com espera de até 90 s põe `paused_until` para todos e a mesma requisição é repetida depois, até 5 vezes. Os testes injetam o próprio pacer e usam o relógio pausado do tokio.

Alternativa: ritmo adaptativo, rápido até o primeiro 429. Rejeitada por ora: no plano gratuito o primeiro estouro custa uma espera de até 60 s e uma requisição perdida; 18 RPM com turnos de até 120 s já transcrevem até 36 min de áudio por minuto.

### D3. Espera longa pausa a nota

Espera acima de 90 s (ou mais de 5 esperas seguidas) vira o erro `transcription_rate_limited` com `details.resumeAtMs`. O turno que o recebeu é gravado como falha (o job volta a pendente na próxima rodada), o stream para de lançar turnos, e o fallback da faixa não roda. A nota fica com a frase "Groq usage limit reached. Clovy continues transcribing at HH:MM. Parts already transcribed are kept." (hora local). `commands.rs` agenda, no mesmo processo, a nova rodada para `resumeAtMs` + 5 s, se a nota ainda estiver com essa falha. A nova rodada é a de "Tentar de novo": jobs prontos são reaproveitados e só os pendentes vão à Groq. Se o app fechar, a nota fica com a frase e o botão "Tentar de novo".

### D4. Turnos de até 120 s

`FINAL_TRANSCRIPTION_SPAN_MS = 120_000` passa a ser o teto do agrupamento e do corte. Escolha:

- 120 s de PCM16 mono a 16 kHz têm 3,84 MB, 15% dos 25 MB.
- Fala contínua de uma faixa vira 1 requisição em vez de 4; turnos curtos se juntam mais vezes, e o mínimo de 10 s cobrado pesa menos.
- O agrupamento continua só na mesma faixa, sem atravessar turno da outra faixa nem trecho de eco cortado, com intervalo de até 2,5 s: não entra silêncio novo e a atribuição por faixa não muda.
- Uma falha reenvia no máximo 120 s.
- Mais que isso quase não ajuda: os turnos já terminam na troca de faixa ou numa pausa.

O fallback da faixa inteira passa a cortar em 120 s: uma faixa de 1 h vai de 120 para 30 requisições.

Compatibilidade: a id de cada job vem do intervalo do turno. Uma sessão que já tem turnos prontos no layout de 30 s calcula os dois layouts e fica com o que reaproveita mais turnos prontos; no empate, 120 s.

FLAC: a Groq aceita, mas o projeto não tem codificador e a Groq cobra por segundo de áudio, não por byte. O tamanho não é o limite que estoura, então fica WAV.

### D5. Mensagens

`notes.failure.groqUsageLimit` com o texto exato que o Rust grava para a Groq, e `notes.failure.rateLimitPaused` com `{time}`, reconhecida por padrão no banner. As duas em en e pt-BR.

## Risks / Trade-offs

- Usuário de plano pago fica em 18 RPM: uma reunião de 1 h com ~60 turnos por faixa leva cerca de 7 min. Melhorar exige ler o plano, fora deste escopo.
- O fallback da faixa inteira não guarda pedaços: um limite longo no meio dele perde os pedaços daquela rodada. Com pedaços de 120 s e o ritmo, ele faz menos requisições.
- A retomada automática não sobrevive ao fechamento do app.
