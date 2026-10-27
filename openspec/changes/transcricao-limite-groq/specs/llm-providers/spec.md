## ADDED Requirements

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

#### Scenario: Ritmo dentro do limite por minuto
- **WHEN** as duas faixas têm muitos turnos para transcrever
- **THEN** nunca começam mais de 18 requisições em 60 s e nunca há mais de 2 ao mesmo tempo
- Test: unit — `processing.rs`, `RequestPacer` com relógio pausado e stream com contador de concorrência

#### Scenario: Pedaços abaixo do limite de upload
- **WHEN** um turno ou a faixa inteira vai à Groq
- **THEN** cada pedaço tem no máximo 120 s (3,84 MB em PCM16 mono a 16 kHz), abaixo dos 25 MB
- Test: unit — `audio/turns.rs`

#### Scenario: Mensagem traduzida
- **WHEN** a interface está em pt-BR e a nota guarda a frase de limite ou de pausa da Groq
- **THEN** o banner mostra a frase em pt-BR, com a hora na de pausa
- Test: unit — `src/test/note-failure-banner.test.tsx`
