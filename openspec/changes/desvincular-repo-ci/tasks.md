# Tasks

## 1. CI

- [ ] 1.1 Apagar os workflows de release e deploy da organização (D1).
- [ ] 1.2 Trocar runners e ações da Blacksmith nos workflows que ficam; validar com `actionlint` e disparar o workflow `desktop` no fork (`gh workflow run -R guilhermexp/os-clovy`), registrando que os jobs saem da fila e o resultado.

## 2. clovy-api local

- [ ] 2.1 RED: testes de config e inicialização que exigem subir sem OS Accounts, com `127.0.0.1` padrão e token local, e que `/verify` e o envio ao OS Platform não existem; verificar que falham.
- [ ] 2.2 GREEN: D2; testes de 2.1 passam; `cargo test` do workspace `clovy-api` verde (`pnpm test:clovy-api`).
- [ ] 2.3 Subir o `clovy-api` local do dono com o `.env` dele (sem imprimir valores) e fazer uma chamada real a `/healthz` e uma a `/v1/models` com o token local.

## 3. Repositório

- [ ] 3.1 Remover `CODEOWNERS`, `os-platform.json`, `.conductor/`, skills e entradas do lock ligadas à organização; ajustar `Makefile` (D3).
- [ ] 3.2 README, SECURITY, CONTRIBUTING, templates de issue e `AGENTS.md` sem links da organização.
- [ ] 3.3 Remover `git fetch upstream` de `scripts/qualify-windows-build.ps1`; o orquestrador remove o remote `upstream` do checkout principal ao integrar (D4).
- [ ] 3.4 Guarda D5 no `make verify`.

## 4. Verificação

- [ ] 4.1 `make verify` verde no Mac.
- [ ] 4.2 Evidência em `.tmp/verify/desvincular-repo-ci/` (link do run do CI no fork, saída do `clovy-api` local, varredura da guarda).
