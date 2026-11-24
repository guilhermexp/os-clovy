import { describe, expect, it } from "vitest";
import shimmerCss from "../styles/shimmer.css?raw";

function cssRuleFor(css: string, selector: string) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = new RegExp(`${escaped}\\s*\\{`).exec(css);
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

describe("shimmer system", () => {
  it("uses the shared theme contrast and cadence", () => {
    expect(shimmerCss).toContain(
      "--_highlight: var(--shimmer-color, oklch(from currentColor l c h / calc(alpha * 0.2)));",
    );
    expect(cssRuleFor(shimmerCss, '[data-theme="dark"] .shimmer')).toContain(
      "oklch(from currentColor max(0.8, calc(l + 0.4)) c h / calc(alpha + 0.4))",
    );
    expect(shimmerCss).toContain(
      "animation: tw-shimmer var(--shimmer-duration, 2s) linear infinite;",
    );
  });
});
