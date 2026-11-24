import { existsSync, readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { catalogs } from "../i18n/catalog";

describe("retired desktop chat", () => {
  it.each([
    "src/components/agent/AgentWorkspace.tsx",
    "src/components/agent/AgentSessionsList.tsx",
    "src/components/note-chat",
    "src/agent-hud.ts",
    "agent-hud.html",
    "src/styleguide/sections/ChatPattern.tsx",
  ])("removes %s from the frontend", (path) => {
    expect(existsSync(path)).toBe(false);
  });

  it.each(["en", "pt-BR"] as const)("omits retired chat copy from %s", (locale) => {
    const messages = catalogs[locale];
    expect(Object.keys(messages).filter((key) => key.startsWith("notes.chat."))).toEqual([]);
    expect(messages["agent.hero.greeting1"]).toBeUndefined();
    expect(messages["notes.header.copyReference"]).toBeDefined();
    expect(messages["agent.computerUse.label"]).toBeDefined();
  });

  it("drops the retired composer styles and keeps the routine Markdown renderer styled", () => {
    const css = readFileSync("src/styles/app.css", "utf8");
    expect(css).not.toMatch(/\.(agent-composer-box|note-chat-panel|agent-workspace)\b/);
    expect(css).toMatch(/\.agent-markdown\s*\{/);
    expect(css).toMatch(/\.agent-approval-actions\s*\{/);
    expect(css).toMatch(/\.agent-composer-model-popover\s*\{/);
  });
});
