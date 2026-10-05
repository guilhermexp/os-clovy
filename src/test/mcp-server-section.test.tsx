import type * as TauriCore from "@tauri-apps/api/core";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  writeClipboardText: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof TauriCore>()),
  invoke: mocks.invoke,
}));

vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: mocks.writeClipboardText,
}));

import { McpServerSection } from "../components/settings/McpServerSection";
import { applyInterfaceLocale } from "../i18n/locale";
import type { McpServerStatusDto } from "../lib/mcp-server";

const binaryPath = "/Applications/Clovy.app/Contents/Resources/native/bin/clovy-mcp";
const channelDir = "/Users/me/Library/Application Support/co.opensoftware.june/mcp";
const claudeCodeConfig = JSON.stringify(
  { mcpServers: { clovy: { type: "stdio", command: binaryPath, args: ["--dir", channelDir] } } },
  null,
  2,
);

function status(overrides: Partial<McpServerStatusDto> = {}): McpServerStatusDto {
  return {
    supported: true,
    enabled: false,
    running: false,
    error: null,
    binaryPath,
    binaryFound: true,
    claudeCodeCommand: `claude mcp add --scope user clovy -- ${binaryPath} --dir '${channelDir}'`,
    claudeCodeConfig,
    cursorConfig: JSON.stringify(
      { mcpServers: { clovy: { command: binaryPath, args: ["--dir", channelDir] } } },
      null,
      2,
    ),
    ...overrides,
  };
}

describe("McpServerSection", () => {
  let current: McpServerStatusDto;

  beforeEach(() => {
    current = status();
    mocks.writeClipboardText.mockResolvedValue(undefined);
    mocks.invoke.mockImplementation(async (command: string, args?: unknown) => {
      if (command === "mcp_server_status") return current;
      if (command === "mcp_server_set_enabled") {
        const { request } = args as { request: { enabled: boolean } };
        current = { ...current, enabled: request.enabled, running: request.enabled };
        return current;
      }
      throw new Error(`Unhandled command: ${command}`);
    });
  });

  afterEach(() => {
    cleanup();
    applyInterfaceLocale("en");
    vi.clearAllMocks();
  });

  it("starts off, turns on, and copies a Claude Code configuration that runs the bundled relay", async () => {
    const user = userEvent.setup();
    render(<McpServerSection />);

    const toggle = await screen.findByRole("switch", { name: "Clovy MCP server" });
    expect(toggle).not.toBeChecked();
    expect(screen.queryByRole("button", { name: "Copy Claude Code configuration" })).toBeNull();

    await user.click(toggle);
    expect(mocks.invoke).toHaveBeenCalledWith("mcp_server_set_enabled", {
      request: { enabled: true },
    });
    await waitFor(() => expect(toggle).toBeChecked());
    expect(screen.getByText("Running")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Copy Claude Code configuration" }));
    expect(mocks.writeClipboardText).toHaveBeenCalledTimes(1);
    const copied = JSON.parse(mocks.writeClipboardText.mock.calls[0][0] as string);
    expect(copied.mcpServers.clovy.command).toBe(binaryPath);
    expect(copied.mcpServers.clovy.args).toEqual(["--dir", channelDir]);
    expect(screen.getByRole("button", { name: "Copied" })).toHaveAttribute("data-copied", "true");

    await user.click(screen.getByRole("button", { name: "Copy Cursor configuration" }));
    expect(mocks.writeClipboardText).toHaveBeenLastCalledWith(current.cursorConfig);
    await user.click(screen.getByRole("button", { name: "Copy Claude Code command" }));
    expect(mocks.writeClipboardText).toHaveBeenLastCalledWith(current.claudeCodeCommand);
  });

  it("turns the server off again", async () => {
    current = status({ enabled: true, running: true });
    const user = userEvent.setup();
    render(<McpServerSection />);
    const toggle = await screen.findByRole("switch", { name: "Clovy MCP server" });
    await user.click(toggle);
    expect(mocks.invoke).toHaveBeenCalledWith("mcp_server_set_enabled", {
      request: { enabled: false },
    });
    await waitFor(() => expect(toggle).not.toBeChecked());
    expect(screen.queryByRole("button", { name: "Copy Cursor configuration" })).toBeNull();
  });

  it("shows why the server is not usable", async () => {
    current = status({
      enabled: true,
      binaryFound: false,
      error: "could not listen on /tmp/x/clovy.sock",
    });
    render(<McpServerSection />);
    const alerts = await screen.findAllByRole("alert");
    expect(alerts.map((alert) => alert.textContent)).toEqual([
      "The clovy-mcp program was not found next to Clovy. Install a full build to use the server.",
      "The server could not start: could not listen on /tmp/x/clovy.sock",
    ]);
  });

  it("shows a failed switch with the backend message", async () => {
    mocks.invoke.mockImplementation(async (command: string) => {
      if (command === "mcp_server_status") return current;
      throw { code: "activity_settings_save_failed", message: "disk full" };
    });
    const user = userEvent.setup();
    render(<McpServerSection />);
    await user.click(await screen.findByRole("switch", { name: "Clovy MCP server" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("disk full");
  });

  it("renders nothing where the server is unsupported", async () => {
    current = status({ supported: false });
    const { container } = render(<McpServerSection />);
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("mcp_server_status"));
    expect(container).toBeEmptyDOMElement();
  });

  it("speaks pt-BR", async () => {
    applyInterfaceLocale("pt-BR");
    current = status({ enabled: true, running: true });
    render(<McpServerSection />);
    expect(await screen.findByRole("switch", { name: "Servidor MCP do Clovy" })).toBeChecked();
    expect(
      screen.getByRole("button", { name: "Copiar configuração do Claude Code" }),
    ).toBeInTheDocument();
  });
});
