# Tasks

## 1. Migração

- [ ] 1.1 RED: testes do `identity_migration` (decisão com pasta nova ausente, vazia e preenchida; cópia completa com origem intacta; falha injetada deixa pasta nova vazia; credenciais copiadas incluindo a chave SQLCipher; credencial negada falha tudo); verificar que falham.
- [ ] 1.2 GREEN: D3 e D2, chamados no início do `setup` antes de qualquer banco; testes de 1.1 passam.

## 2. Renomeação

- [ ] 2.1 D1: identificador, nomes, executável, auxiliares, inicialização automática, editor e `authors`, com constante única no Rust; scripts de verificação e testes que fixavam os nomes antigos.
- [ ] 2.2 D4 e D5: extensão com chave e id novos, host de mensagens nativas novo, só `clovy://`.
- [ ] 2.3 D6: remover camadas de compatibilidade June, mantendo só as origens da migração.

## 3. Documentação

- [ ] 3.1 Nova ADR substituindo a 0055 (D7); `AGENTS.md`, `CONTEXT.md` e docs com os nomes e pastas novos.

## 4. Verificação

- [ ] 4.1 `make verify` verde no Mac; guarda de domínios estendida para `co\.opensoftware|os-june|June\.app`.
- [ ] 4.2 Prova no Mac com um perfil antigo sintético criado pela execução (notas, banco de atividade e chave SQLCipher de teste, sob origens e destinos de prova injetados por variável de ambiente só em build de dev), sem tocar o perfil nem o Keychain reais do dono: o app dev migra, abre com as mesmas notas e atividade, origem conferida intacta por hash; capturas e números em `.tmp/verify/identidade-clovy/`. O perfil real do dono só migra quando ele abrir o app depois do merge, com o orquestrador acompanhando.
- [ ] 4.3 Prova no Windows `cert` com perfil de teste antigo criado pela execução: migração e abertura; limpeza do `cert` ao final.
