# Tasks

## 1. Migração

- [x] 1.1 RED: testes do `identity_migration` (decisão com pasta nova ausente, vazia e preenchida; cópia completa com origem intacta; falha injetada deixa pasta nova vazia; credenciais copiadas incluindo a chave SQLCipher; credencial negada falha tudo); verificar que falham. Evidência: `ad5e2e34`, 12/12 falham (`.tmp/verify/identidade-clovy/1.1-red.txt`), testes sha256 `af57db47…`.
- [x] 1.2 GREEN: D3 e D2, chamados no início do `setup` antes de qualquer banco; testes de 1.1 passam. Evidência: `d3c8870c`, 12/12 passam com o mesmo `tests.rs` (`1.2-green.txt`). Desvio com evidência: roda no início de `run()`, antes do `Builder`, porque o Tauri cria as janelas e o WebView antes do `setup` (`tauri-2.11.3/src/app.rs:2524-2531`).

## 2. Renomeação

- [x] 2.1 D1: identificador, nomes, executável, auxiliares, inicialização automática, editor e `authors`, com constante única no Rust; scripts de verificação e testes que fixavam os nomes antigos. Evidência: `4633f9f9` (+ `e034cd9e` e `25b77337`, correções achadas no Windows).
- [x] 2.2 D4 e D5: extensão com chave e id novos, host de mensagens nativas novo, só `clovy://`. Evidência: `7ffeabca`, id `cmfpkfaibipkocbjcpgamaednndpopcp`; `osjune` já tinha saído no `main`.
- [x] 2.3 D6: remover camadas de compatibilidade June, mantendo só as origens da migração. Evidência: `3fc4ce0f`. Persona, skill e linha de configuração do Obsidian da era June são lidas uma vez e nunca escritas.

## 3. Documentação

- [x] 3.1 Nova ADR substituindo a 0055 (D7); `AGENTS.md`, `CONTEXT.md` e docs com os nomes e pastas novos. Evidência: `32c9a085` (ADR-0068).

## 4. Verificação

- [x] 4.1 `make verify` verde no Mac; guarda de domínios estendida para `co\.opensoftware|os-june|June\.app`. Evidência: `a49bc197`; `make verify` exit 0 em `make-verify.txt` e de novo após as correções em `make-verify-2.txt` (vitest 1786/1786).
- [x] 4.2 Prova no Mac com um perfil antigo sintético criado pela execução (notas, banco de atividade e chave SQLCipher de teste, sob origens e destinos de prova injetados por variável de ambiente só em build de dev), sem tocar o perfil nem o Keychain reais do dono: o app dev migra, abre com as mesmas notas e atividade, origem conferida intacta por hash; capturas e números em `.tmp/verify/identidade-clovy/`. O perfil real do dono só migra quando ele abrir o app depois do merge, com o orquestrador acompanhando. Evidência: `.tmp/verify/identidade-clovy/mac/report.md`: migrou 3 arquivos e 2 credenciais; notas e atividade lidas pelo app via MCP e na UI; origem igual por hash.
- [x] 4.3 Prova no Windows `cert` com perfil de teste antigo criado pela execução: migração e abertura; limpeza do `cert` ao final. Evidência: `.tmp/verify/identidade-clovy/windows/report.md`: migrou 3 arquivos e 2 credenciais; UI via CDP mostra notas e atividade; origem igual por hash; `cert` limpo (45,17 → 45,02 GB).
