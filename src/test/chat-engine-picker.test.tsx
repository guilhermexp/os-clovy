import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mocks.invoke,
}));

import { ChatEngineNotice, ChatEnginePicker } from "../components/agent/composer/ChatEnginePicker";
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
      clovyTools: "available",
    },
    {
      id: "codex",
      name: "Codex",
      installed: true,
      reason: null,
      modelId: "__clovy_cli_engine__:codex",
      clovyTools: "server_off",
    },
    {
      id: "pi",
      name: "Pi",
      installed: false,
      reason: "Not found in PATH",
      modelId: "__clovy_cli_engine__:pi",
      clovyTools: "unsupported",
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

describe("ChatEnginePicker and ChatEngineNotice", () => {
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

    render(
      <ChatEnginePicker
        model="auto"
        setModel={setModel}
        catalog={sampleCatalog}
        showNotice={false}
      />,
    );

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

  it("disables a not-installed CLI with its reason", async () => {
    const user = userEvent.setup();
    const setModel = vi.fn();

    render(
      <ChatEnginePicker
        model="auto"
        setModel={setModel}
        catalog={sampleCatalog}
        showNotice={false}
      />,
    );

    await user.click(screen.getByRole("button", { name: /Chat engine/i }));

    const piOption = screen.getByRole("menuitemradio", { name: /Pi/ });
    expect(piOption).toBeDisabled();
    expect(piOption).toHaveTextContent("Not found in PATH");
  });

  it("choosing a CLI calls setModel with __clovy_cli_engine__:<id>", async () => {
    const user = userEvent.setup();
    const setModel = vi.fn();

    render(
      <ChatEnginePicker
        model="auto"
        setModel={setModel}
        catalog={sampleCatalog}
        showNotice={false}
      />,
    );

    await user.click(screen.getByRole("button", { name: /Chat engine/i }));
    await user.click(screen.getByRole("menuitemradio", { name: /^Claude Code/ }));

    expect(setModel).toHaveBeenCalledWith("__clovy_cli_engine__:claude");
  });

  it("choosing an endpoint sets its optionId", async () => {
    const user = userEvent.setup();
    const setModel = vi.fn();

    render(
      <ChatEnginePicker
        model="auto"
        setModel={setModel}
        catalog={sampleCatalog}
        showNotice={false}
      />,
    );

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
        showNotice={false}
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

  it("shows the no-tools notice for unsupported and server_off, and not for available", () => {
    const { rerender } = render(
      <ChatEngineNotice catalog={sampleCatalog} model="__clovy_cli_engine__:claude" />,
    );

    expect(
      screen.getByText("Claude Code runs on this Mac with Clovy's tools."),
    ).toBeInTheDocument();
    expect(screen.queryByText(/No Clovy tools/i)).not.toBeInTheDocument();

    rerender(<ChatEngineNotice catalog={sampleCatalog} model="__clovy_cli_engine__:codex" />);
    expect(
      screen.getByText(
        "No Clovy tools in this session. Turn on the Clovy MCP server in Settings, Agent to give Codex your notes and activity.",
      ),
    ).toBeInTheDocument();

    rerender(<ChatEngineNotice catalog={sampleCatalog} model="__clovy_cli_engine__:pi" />);
    expect(
      screen.getByText("No Clovy tools in this session: Pi can't receive Clovy's MCP server."),
    ).toBeInTheDocument();
  });

  it("renders labels and notices in pt-BR when locale is set to pt-BR", () => {
    applyInterfaceLocale("pt-BR");

    render(<ChatEngineNotice catalog={sampleCatalog} model="__clovy_cli_engine__:pi" />);
    expect(
      screen.getByText(
        "Nenhuma ferramenta do Clovy nesta sessão: Pi não pode receber o servidor MCP do Clovy.",
      ),
    ).toBeInTheDocument();
  });
});
