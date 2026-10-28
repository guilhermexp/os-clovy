# Tasks

## 1. Dependência e skill

- [ ] 1.1 Ler https://asciify.org/skill/SKILL.md e as referências `applying-asciify.md`, `components.md` e `kit.md`; instalar o skill do agente (`skills add ayangabryl/asciify-engine --skill asciify-engine`) pelo caminho que `spec/package-install-security.md` permite, ou registrar no design por que foi lido direto. Verificar: skill presente ou nota no design.
- [ ] 1.2 `sfw pnpm add asciify-engine@4.29.0` (entrada em `allowBuilds` se pedir script de instalação) e conferir nos tipos instalados `background`, `drift`, `mark`, `clear`, `destroy` e os controles da seleção. Verificar: `pnpm install --frozen-lockfile` passa e o mapeamento dos controles fica registrado na decisão 3 do design.

## 2. Tema escuro padrão

- [ ] 2.1 Sem chave salva, `src/lib/theme.ts`, `index.html`, `styleguide.html` e `onboarding-preview.html` resolvem escuro; teste unitário do padrão e da preferência salva; ajustar `technical-identity.test.mjs`. Verificar: testes passam e a primeira abertura em perfil novo mostra escuro sem flash claro.

## 3. Remover a lavagem colorida

- [ ] 3.1 Apagar as regras de marca em `.agent-workspace::before`, `[data-home]::before` (claro e escuro), as camadas de marca do véu do composer e o gradiente do cartão do onboarding; remover tokens `--hero-wash*` sem uso e a entrada em `Color.tsx`; trocar o teste de `home-polish-css.test.ts` por checagem de ausência. Verificar: `pnpm test` do arquivo passa; capturas da Nova sessão e do Início em claro e escuro sem faixa colorida.

## 4. Preferência e configuração

- [ ] 4.1 Criar `src/lib/app-background.ts` (chave `os-clovy:background`, `drift | none`, padrão `drift`, evento de mudança) com teste unitário de padrão, valor inválido e evento. Verificar: teste passa.
- [ ] 4.2 Linha "Fundo" em Configurações → Aparência com "Drift" e "Nenhum", chaves i18n em en e pt-BR, blurb da aba atualizado. Verificar: `i18n-catalog.test.ts` passa e a troca aparece na Nova sessão sem recarregar.

## 5. Componente do fundo

- [ ] 5.1 Componente de fundo montado em `AgentWorkspace.tsx` para hero e Início: import dinâmico do kit, um handle por montagem, `destroy` no cleanup e na troca para "Nenhum", guarda de desmontagem antes do import, `aria-hidden`, `pointer-events: none`, `--ascii-ink: #8f8d86`, `clear` na coluna do conteúdo. Testes de componente com o kit simulado: opções recebidas, destroy único, import rejeitado sem erro. Verificar: testes passam.
- [ ] 5.2 Registrar o fundo e a preferência nos docs de design que descrevem a lavagem do hero (`docs/design/*`) e no styleguide, se houver seção. Verificar: nenhuma menção restante a `--hero-wash` fora do histórico.

## 6. Verificação integrada

- [ ] 6.1 Rodar `pnpm check`, `pnpm typecheck` e `NODE_OPTIONS=--no-experimental-webstorage pnpm test`. Verificar: saída sem falhas novas.
- [ ] 6.2 No app (`pnpm tauri:dev` com identificador de dev próprio) e no `pnpm dev`: capturas da Nova sessão e do Início em claro e escuro, janela larga e estreita (390 px); movimento reduzido mostra quadro parado; WebGL desligado deixa fundo liso; ir e voltar da Nova sessão mantém um único canvas; trocar para "Nenhum" remove o canvas. Evidência em `.tmp/verify/fundo-ascii-tema-escuro/`.
- [ ] 6.3 Rodar `/Users/guilhermevarela/orchestrator.v5/scripts/review-pair.sh --gate` antes do commit final e tratar os achados. Verificar: exit 0 ou achados refutados registrados aqui com o caminho do `summary.md`.

## Workflow follow-up

- Arquivar a change depois da revisão e atualizar o `AGENTS.md` do projeto.
