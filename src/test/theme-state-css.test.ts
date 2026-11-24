import { describe, expect, it } from "vitest";
import appCss from "../styles/app.css?raw";
import tokensCss from "../styles/tokens.css?raw";

function cssRuleFor(selector: string, css = appCss) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = new RegExp(`(?:^|\\n)${escaped}\\s*\\{`).exec(css);
  if (!match) throw new Error(`Missing CSS rule for ${selector}`);
  const openIndex = match.index + match[0].length - 1;
  let depth = 0;
  for (let index = openIndex; index < css.length; index += 1) {
    if (css[index] === "{") depth += 1;
    if (css[index] === "}") {
      depth -= 1;
      if (depth === 0) return css.slice(openIndex + 1, index);
    }
  }
  throw new Error(`Unclosed CSS rule for ${selector}`);
}

describe("Markdown result styles", () => {
  it("preserves whole words when markdown tables calculate column widths", () => {
    const cells = cssRuleFor(".agent-md-table th,\n.agent-md-table td");

    expect(cells).toContain("overflow-wrap: break-word;");
    expect(cells).toContain("vertical-align: top;");
  });
});

describe("Clovy theme-state parity", () => {
  it("lifts the semantic accent and flips its foreground in dark mode", () => {
    const darkTokens = cssRuleFor('[data-theme="dark"]', tokensCss);

    expect(darkTokens).toContain("--primary: var(--brand-bright);");
    expect(darkTokens).toContain("--primary-foreground: var(--clovy-ink);");
  });

  it("routes onboarding progress and checked switches through the semantic accent", () => {
    const progress = cssRuleFor('.onboarding-progress-seg[data-state="current"]');
    const checkedSwitch = cssRuleFor('.switch[data-state="checked"]');

    expect(progress).toContain("background: var(--primary);");
    expect(checkedSwitch).toContain("background: var(--primary);");
  });
});
