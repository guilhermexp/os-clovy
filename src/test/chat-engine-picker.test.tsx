import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mocks.invoke,
}));

import { ChatEnginePicker } from "../components/agent/composer/ChatEnginePicker";
import appCss from "../styles/app.css?raw";
import { applyInterfaceLocale, resetInterfaceLocaleForTests } from "../i18n/locale";
import type { ChatEngineCatalog } from "../lib/chat-engine";

const sampleCatalog: ChatEngineCatalog = {
  clis: [
    {
      id: "claude",
      name: "Claude Code",
      installed: true,
      reason: null,
      modelId: "__clovy_cli_engine__:claude",
    },
    {
      id: "codex",
      name: "Codex",
      installed: true,
      reason: null,
      modelId: "__clovy_cli_engine__:codex",
    },
    {
      id: "pi",
      name: "Pi",
      installed: false,
      reason: "Not found in PATH",
      modelId: "__clovy_cli_engine__:pi",
    },
  ],
  endpoints: [
    {
      id: "local",
      name: "Local Ollama",
      modelId: "llama3",
      optionId: "__june_local_generation__:llama3",
    },
  ],
};

describe("ChatEnginePicker", () => {
  beforeEach(() => {
    resetInterfaceLocaleForTests();
    applyInterfaceLocale("en");
    mocks.invoke.mockReset();
    mocks.invoke.mockImplementation((cmd: string) => {
      if (cmd === "chat_engine_catalog") {
        return Promise.resolve(sampleCatalog);
      }
      return Promise.resolve(null);
    });
  });

  afterEach(() => {
    cleanup();
    resetInterfaceLocaleForTests();
    applyInterfaceLocale("en");
  });

  it("lists Clovy, endpoints, and CLIs in the picker menu", async () => {
    const user = userEvent.setup();
    const setModel = vi.fn();

    render(<ChatEnginePicker model="auto" setModel={setModel} catalog={sampleCatalog} />);

    const trigger = screen.getByRole("button", { name: /Chat engine/i });
    expect(trigger).toHaveTextContent("Clovy");

    await user.click(trigger);

    expect(screen.getByRole("menu", { name: "Chat engine" })).toBeInTheDocument();
    expect(screen.getByRole("menuitemradio", { name: /^Clovy/ })).toBeInTheDocument();
    expect(screen.getByRole("menuitemradio", { name: /^Local Ollama/ })).toBeInTheDocument();
    expect(screen.getByRole("menuitemradio", { name: /^Claude Code/ })).toBeInTheDocument();
    expect(screen.getByRole("menuitemradio", { name: /^Codex/ })).toBeInTheDocument();
    expect(screen.getByRole("menuitemradio", { name: /Pi/ })).toBeInTheDocument();
  });

  it("lists the CLIs first with Claude Code as the default and Clovy last as legacy", async () => {
    const user = userEvent.setup();
    render(
      <ChatEnginePicker
        model="__clovy_cli_engine__:claude"
        setModel={vi.fn()}
        catalog={sampleCatalog}
      />,
    );

    await user.click(screen.getByRole("button", { name: /Chat engine/i }));
    const rows = screen.getAllByRole("menuitemradio");
    expect(
      rows.map((row) => row.querySelector(".agent-composer-model-choice-copy")?.textContent),
    ).toEqual([
      "Claude CodeDefault",
      "Codex",
      "PiNot found in your login shell PATH.",
      "Local Ollama",
      "ClovyBuilt-in agent, kept while its tools move to the CLIs",
    ]);
    expect(screen.getByText("Legacy")).toBeInTheDocument();
  });

  it("marks Codex as the default when Claude Code is not installed", async () => {
    const user = userEvent.setup();
    const catalog: ChatEngineCatalog = {
      ...sampleCatalog,
      clis: sampleCatalog.clis.map((cli) =>
        cli.id === "claude" ? { ...cli, installed: false, reason: "Not found in PATH" } : cli,
      ),
    };
    render(<ChatEnginePicker model="auto" setModel={vi.fn()} catalog={catalog} />);

    await user.click(screen.getByRole("button", { name: /Chat engine/i }));
    expect(screen.getByRole("menuitemradio", { name: /^Codex/ })).toHaveTextContent("Default");
    expect(screen.getByRole("menuitemradio", { name: /^Claude Code/ })).not.toHaveTextContent(
      "Default",
    );
  });

  it("disables a not-installed CLI with its reason", async () => {
    const user = userEvent.setup();
    const setModel = vi.fn();

    render(<ChatEnginePicker model="auto" setModel={setModel} catalog={sampleCatalog} />);

    await user.click(screen.getByRole("button", { name: /Chat engine/i }));

    const piOption = screen.getByRole("menuitemradio", { name: /Pi/ });
    expect(piOption).toBeDisabled();
    expect(piOption).toHaveTextContent("Not found in your login shell PATH.");
    expect(piOption).not.toHaveTextContent("Not found in PATH");
  });

  it("choosing a CLI calls setModel with __clovy_cli_engine__:<id>", async () => {
    const user = userEvent.setup();
    const setModel = vi.fn();

    render(<ChatEnginePicker model="auto" setModel={setModel} catalog={sampleCatalog} />);

    await user.click(screen.getByRole("button", { name: /Chat engine/i }));
    await user.click(screen.getByRole("menuitemradio", { name: /^Claude Code/ }));

    expect(setModel).toHaveBeenCalledWith("__clovy_cli_engine__:claude");
  });

  it("choosing an endpoint sets its optionId", async () => {
    const user = userEvent.setup();
    const setModel = vi.fn();

    render(<ChatEnginePicker model="auto" setModel={setModel} catalog={sampleCatalog} />);

    await user.click(screen.getByRole("button", { name: /Chat engine/i }));
    await user.click(screen.getByRole("menuitemradio", { name: /^Local Ollama/ }));

    expect(setModel).toHaveBeenCalledWith("__june_local_generation__:llama3");
  });

  it("keeps two endpoints that serve the same model apart", async () => {
    const user = userEvent.setup();
    const setModel = vi.fn();
    const catalog: ChatEngineCatalog = {
      clis: sampleCatalog.clis,
      endpoints: [
        {
          id: "home",
          name: "Home server",
          modelId: "qwen3",
          optionId: "__june_local_generation__:qwen3@home",
        },
        {
          id: "lab",
          name: "Lab server",
          modelId: "qwen3",
          optionId: "__june_local_generation__:qwen3@lab",
        },
      ],
    };

    render(
      <ChatEnginePicker
        model="__june_local_generation__:qwen3@lab"
        setModel={setModel}
        catalog={catalog}
      />,
    );

    expect(screen.getByRole("button", { name: /Chat engine: Lab server/i })).toBeVisible();
    await user.click(screen.getByRole("button", { name: /Chat engine/i }));
    expect(screen.getByRole("menuitemradio", { name: /^Lab server/ })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(screen.getByRole("menuitemradio", { name: /^Home server/ })).toHaveAttribute(
      "aria-checked",
      "false",
    );
    await user.click(screen.getByRole("menuitemradio", { name: /^Home server/ }));
    expect(setModel).toHaveBeenCalledWith("__june_local_generation__:qwen3@home");
  });

  it("keeps an unavailable CLI's logo, name and reason on its own row", async () => {
    const user = userEvent.setup();
    const reason = (bin: string) => `\`${bin}\` was not found in your login shell PATH.`;
    const catalog: ChatEngineCatalog = {
      clis: [
        ...sampleCatalog.clis,
        {
          id: "cursor-agent",
          name: "Cursor Agent",
          installed: false,
          reason: reason("cursor-agent"),
          modelId: "__clovy_cli_engine__:cursor-agent",
        },
        {
          id: "copilot",
          name: "GitHub Copilot",
          installed: false,
          reason: reason("copilot"),
          modelId: "__clovy_cli_engine__:copilot",
        },
      ],
      endpoints: [],
    };
    render(<ChatEnginePicker model="auto" setModel={vi.fn()} catalog={catalog} />);
    await user.click(screen.getByRole("button", { name: /Chat engine/i }));

    const cursor = screen.getByRole("menuitemradio", { name: /^Cursor Agent/ });
    const copilot = screen.getByRole("menuitemradio", { name: /^GitHub Copilot/ });
    expect(cursor).not.toContainElement(copilot);
    expect(cursor).not.toHaveTextContent("GitHub Copilot");
    for (const [row, name] of [
      [cursor, "Cursor Agent"],
      [copilot, "GitHub Copilot"],
    ] as const) {
      expect(row.querySelector(".agent-composer-model-row-logo svg")).not.toBeNull();
      expect(row.querySelector(".agent-composer-model-row-name")).toHaveTextContent(name);
      expect(row.querySelector(".agent-composer-model-choice-desc")).toHaveTextContent(
        "Not found in your login shell PATH.",
      );
    }

    // jsdom has no layout: the stylesheet carries the contract. Engine rows
    // keep their natural height (a capped popover scrolls instead of
    // squeezing two-line rows into one), the copy takes the free width and
    // the reason wraps inside it.
    const rule = (selector: string) => {
      const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
      const match = new RegExp(`(?:^|\\n)${escaped}\\s*\\{([^}]*)\\}`).exec(appCss);
      return match?.[1] ?? "";
    };
    expect(rule(".agent-engine-popover")).toContain("overflow-y: auto;");
    expect(rule(".agent-engine-popover .agent-composer-model-menu")).toContain("flex: none;");
    expect(rule(".agent-engine-popover .agent-composer-model-row")).toContain("flex: none;");
    expect(rule(".agent-engine-popover .agent-composer-model-choice-copy")).toContain(
      "flex: 1 1 auto;",
    );
    expect(rule(".agent-engine-popover .agent-composer-model-choice-desc")).toContain(
      "overflow-wrap: anywhere;",
    );
  });

  it("shows the translated not-found reason instead of the backend text", async () => {
    applyInterfaceLocale("pt-BR");
    const user = userEvent.setup();
    const catalog: ChatEngineCatalog = {
      clis: [
        {
          id: "copilot",
          name: "GitHub Copilot",
          installed: false,
          reason: "`copilot` was not found in your login shell PATH.",
          modelId: "__clovy_cli_engine__:copilot",
        },
      ],
      endpoints: [],
    };
    render(<ChatEnginePicker model="auto" setModel={vi.fn()} catalog={catalog} />);
    await user.click(screen.getByRole("button"));

    const copilot = screen.getByRole("menuitemradio", { name: /^GitHub Copilot/ });
    expect(copilot).toHaveTextContent("Não encontrado no PATH da sua sessão.");
    expect(copilot).not.toHaveTextContent("was not found");
  });

  it("gives the legacy Clovy row the Clovy mark like the other rows", async () => {
    const user = userEvent.setup();
    render(<ChatEnginePicker model="auto" setModel={vi.fn()} catalog={sampleCatalog} />);
    await user.click(screen.getByRole("button", { name: /Chat engine/i }));

    const clovy = screen.getByRole("menuitemradio", { name: /^Clovy/ });
    const logo = clovy.querySelector(".agent-composer-model-row-logo");
    expect(logo).toHaveAttribute("data-brand", "clovy");
    expect(logo?.querySelector("svg")).not.toBeNull();
  });
});
