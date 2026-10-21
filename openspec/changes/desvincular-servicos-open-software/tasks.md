# Tasks

## 1. Identidade local

- [ ] 1.1 RED: testes do módulo de identidade local (token padrão e sobreposto, usuário local, sempre conectado) e do onboarding/portões do frontend chegando à tela principal sem conta; verificar que falham.
- [ ] 1.2 GREEN: D1 no Rust (remover OS Accounts, esquema `osjune` e escopos; chamadores usam a identidade local) e D2 no frontend; testes de 1.1 passam.

## 2. Remoções

- [ ] 2.1 Cobrança, indicações, financiamento, upgrade e checagens de plano (D2), apagando os testes desses fluxos.
- [ ] 2.2 RED/GREEN do Computer Use sem liberação remota nem plano (D3).
- [ ] 2.3 P3A, relatórios de erro, companheiro, `fetch_browser_transport_policy` e links da Open Software (D4), com dependências sem uso.
- [ ] 2.4 Atualizador da June (D6): config, plugin, `updates.rs` e UI.

## 3. Servidor e guarda

- [ ] 3.1 RED/GREEN de D5: padrão `http://127.0.0.1:8080`, `CLOVY_API_URL` sobrepõe, links de compartilhamento pela origem do servidor.
- [ ] 3.2 Guarda D7 no `make verify`, falhando antes das remoções e passando depois.

## 4. Documentação

- [ ] 4.1 Atualizar ou remover docs afetadas (`docs/os-accounts-login.md`, `docs/companion-*`, `docs/llm-providers.md`, README do app) e conferir com o código.

## 5. Verificação

- [ ] 5.1 `make verify` verde no Mac.
- [ ] 5.2 Prova no app de desenvolvimento com identificador próprio da execução e perfil novo vazio: onboarding sem passo de conta até a tela principal; Ajustes sem Cobrança; uma nota transcrita e um ditado pelo `clovy-api` local; Computer Use disponível sem plano; capturas e números em `.tmp/verify/desvincular-servicos-open-software/`.
