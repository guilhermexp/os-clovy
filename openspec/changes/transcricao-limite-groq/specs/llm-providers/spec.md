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
