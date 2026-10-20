# Tasks

## 1. Resolução no Windows

- [ ] 1.1 Reproduzir no `cert` antes de mudar: app de desenvolvimento da `main` atual mostra `claude` e `codex` como não instalados; registrar `where.exe claude codex`, `PATHEXT` e as chaves de PATH do ambiente do app em `.tmp/verify/cli-agentes-windows/`.
- [ ] 1.2 RED: testes unitários de `LoginEnv` para chave `Path`, `PATHEXT` (escolhe `codex.cmd`, ignora `codex` sem extensão), `PATHEXT` ausente e diretórios de reserva do Windows a partir de `USERPROFILE`/`APPDATA`, com a escrita no mesmo nome de chave; verificar que falham.
- [ ] 1.3 GREEN: D1–D4 em `shell_env.rs`, com a origem de cada diretório de reserva registrada em comentário; testes de 1.2 passam; testes Unix existentes de `llm` sem mudança.

## 2. Execução íntegra

- [ ] 2.1 Provar no `cert` com `.cmd` falso o que chega por argumento e pela entrada padrão para prompts com várias linhas, aspas, `%`, `^`, `&` e acima de 8191 caracteres, pelos caminhos reais `llm::process` e `chat_engine::turn`.
- [ ] 2.2 RED/GREEN do D5 se 2.1 mostrar texto alterado ou erro; senão, registrar a prova e manter um teste de integração Windows que trava esse comportamento.

## 3. Documentação

- [ ] 3.1 Atualizar `docs/llm-providers.md` com a resolução no Windows e conferir com o código.

## 4. Verificação

- [ ] 4.1 `make verify` verde no Mac; `cargo test --manifest-path src-tauri/Cargo.toml --locked llm` e `chat_engine` verdes no `cert`.
- [ ] 4.2 Prova no app de desenvolvimento no `cert`: Ajustes > Modelos lista `claude` e `codex` instalados com caminho e versão; "Testar" e uma conversa de chat com motor CLI respondem (só se o dono autorizar chamadas com as contas do `cert`); capturas e números em `.tmp/verify/cli-agentes-windows/`.
- [ ] 4.3 Limpar o `cert`: parar processos, remover tarefas agendadas, clone, target e toolchains instaladas para a prova; registrar o espaço livre antes e depois.
