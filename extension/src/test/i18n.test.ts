import { describe, expect, it } from "vitest";
import { resolveExtensionLocale, tx } from "../i18n";

describe("extension popup copy", () => {
  it("follows a Portuguese browser and falls back to English otherwise", () => {
    expect(resolveExtensionLocale("pt-BR")).toBe("pt-BR");
    expect(resolveExtensionLocale("pt_PT")).toBe("pt-BR");
    expect(resolveExtensionLocale("en-US")).toBe("en");
    expect(resolveExtensionLocale("fr")).toBe("en");
    expect(resolveExtensionLocale(undefined)).toBe("en");
  });

  it("translates and interpolates", () => {
    expect(tx("pairedTitle", {}, "pt-BR")).toBe("Conectado ao Clovy");
    expect(tx("pairedTitle", {}, "en")).toBe("Connected to Clovy");
    expect(tx("shareCode", { code: "ab12" }, "pt-BR")).toBe(
      "Código de compartilhamento: ab12. Cole na sua conversa do Clovy.",
    );
    expect(tx("shareCode", { code: "ab12" }, "en")).toBe(
      "Share code: ab12. Paste it into your Clovy chat.",
    );
  });
});
