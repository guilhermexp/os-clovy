# Design

## Context

- A lavagem é só CSS: `.agent-workspace::before` (`src/styles/app.css` ~271-307, hero), `[data-home="true"]::before` (~347-380, claro e escuro), camadas de marca do véu do composer do Início (~5532-5552) e o cartão da etapa de personalidade do onboarding (~23676-23691). Tokens `--hero-wash`, `--hero-wash-soft`, `--hero-wash-max-w`, `--hero-wash-side-fade` em `src/styles/tokens.css`. O `[data-home]::after` é véu de rolagem, não brilho, e fica.
- `.agent-workspace` já cria contexto de empilhamento (`isolation: isolate`), então uma camada de fundo dentro dela fica local.
- Tema: `src/lib/theme.ts` (padrão "system"), espelhado antes do paint em `index.html`, `styleguide.html` e `onboarding-preview.html`. Preferências de interface ficam em localStorage, um módulo por preferência em `src/lib/` (modelo: `src/lib/date-format.ts`).
- pnpm 11 com `minimumReleaseAge: 10080` (7 dias) e `allowBuilds` negado por padrão; instalação pela regra `spec/package-install-security.md` (`sfw pnpm add`).
- Em 2026-10-28, a versão mais nova com mais de 7 dias é `asciify-engine@4.29.0` (2026-10-20). O skill do asciify foi testado na 4.32.8; `background`, `drift` e `mark` existem desde 4.14/4.19.

## Goals / Non-Goals

**Goals:**
- Uma camada de fundo única para Nova sessão e Início, controlada por uma preferência.
- Reproduzir a seleção Drift do dono sem trocar padrão nem estilo.

**Non-Goals:**
- Outros presets além de Drift (ficam para depois).
- Fundo nas janelas HUD, nas notas ou nas configurações.
- Mudar a cor de destaque (`--brand`) usada em botões e ícones.

## Decisions

1. **Integração pelo kit, não pelos arquivos baixáveis.** `background(host, { preset: drift, clear, mark: 'dot' })` de `asciify-engine/kit`, como na integração de referência. Alternativa descartada: template baixado (`asciiBackground` de `./drift.js`), que o próprio skill marca como legado.
2. **Versão fixa `asciify-engine@4.29.0`** via `sfw pnpm add asciify-engine@4.29.0`. Antes de usar, conferir nos tipos instalados (`node_modules/asciify-engine/dist/kit*.d.ts`) que `background`, `drift`, `mark`, `clear` e `destroy` existem e quais dos controles da seleção o tipo aceita. Se o pacote tiver script de instalação, adicionar entrada versionada em `allowBuilds`. Atualizar para 4.32.x é tarefa separada, depois da janela de 7 dias.
3. **Mapeamento da seleção.** A seleção do site é `{renderStyle: original, algorithm: bayer4, ditherStrength: 0, scheme: dark, ink: #8f8d86, speed: 1, size: 11, opacity: 1, characters: original, scene: flow, hover: trail, mark: dot}`. Os campos que o tipo do kit aceita vão como opção; a tinta vai como `--ascii-ink: #8f8d86` no host (o kit pinta pelos tokens `--ascii-ink*`); `scheme` segue o tema do app (o kit acompanha claro e escuro). Campo da seleção que o kit 4.29.0 não expõe e não é o padrão do preset fica registrado nesta seção na implementação; se algum controle essencial (preset, marca, tinta) não existir, parar e perguntar ao dono em vez de inventar export.
4. **Componente único** (nome final do implementador, por exemplo `AgentBackground`) montado dentro de `.agent-workspace` quando `heroMode` ou `homeMode`, com `position: absolute; inset: 0; z-index: -1; pointer-events: none`. Import dinâmico de `asciify-engine/kit` dentro de `useEffect`, com guarda: se desmontar antes do import resolver, destruir o handle assim que chegar. Um handle por montagem; `destroy()` no cleanup. `clear` aponta para a coluna do título e do composer (hero) ou para a coluna do conteúdo (Início).
5. **Pausa e movimento reduzido ficam com o kit** (o skill garante pausa fora da tela e quadro parado com movimento reduzido). O componente não duplica esse controle; a verificação visual confirma.
6. **Preferência** `src/lib/app-background.ts`: chave `os-clovy:background`, valores `drift | none`, padrão `drift`, normalizador, evento de mudança na janela (padrão de `date-format.ts`). Sem evento Tauri, porque as HUDs não usam o fundo.
7. **Tema escuro padrão**: sem chave salva, `getStoredTheme` devolve `"dark"` e os três scripts antes do paint resolvem escuro. Mantém o literal `localStorage.getItem("os-clovy:theme")` que `technical-identity.test.mjs` exige; o texto que o teste espera em `styleguide.html` muda junto.
8. **Remoção da lavagem**: apagar as regras de marca e os tokens `--hero-wash*` sem uso; no véu do composer, manter só a camada de fade do card. O teste `home-polish-css.test.ts` "contains the new-session tint..." deixa de valer e é removido ou trocado por uma checagem de ausência de `--brand` atrás do conteúdo.

## Risks / Trade-offs

- [WKWebView sem WebGL2 em algum Mac] → o kit devolve handle inerte; cenário de falha coberto por teste e verificação com WebGL desligado.
- [Custo de GPU com o app aberto o dia todo] → pausa fora da tela e janela oculta vem do kit; medir no `pnpm tauri:dev` e citar dispositivo se houver número de FPS.
- [Versão 4.29.0 atrás da testada pelo skill] → conferir os tipos instalados antes de usar cada opção.
- [Quem tinha o tema em "sistema" sem salvar passa a ver escuro] → é o pedido; quem salvou "sistema" continua em "sistema".
