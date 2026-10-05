import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { LlmProvidersSection } from "../components/settings/LlmProvidersSection";
import { applyInterfaceLocale } from "../i18n/locale";
import type { LlmCliStatusDto, LlmProvidersDto } from "../lib/llm-providers";
import { dispatchProviderModelSettingsChanged } from "../lib/model-privacy";

const mocks = vi.hoisted(() => ({
  llmProviders: vi.fn(),
  llmDetectClis: vi.fn(),
  llmSaveEndpoint: vi.fn(),
  llmDeleteEndpoint: vi.fn(),
  llmTestProvider: vi.fn(),
  llmSetUsage: vi.fn(),
  probeLocalGenerationEndpoint: vi.fn(),
}));

vi.mock("../lib/llm-providers", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/llm-providers")>()),
  llmProviders: mocks.llmProviders,
  llmDetectClis: mocks.llmDetectClis,
  llmSaveEndpoint: mocks.llmSaveEndpoint,
  llmDeleteEndpoint: mocks.llmDeleteEndpoint,
  llmTestProvider: mocks.llmTestProvider,
  llmSetUsage: mocks.llmSetUsage,
}));

vi.mock("../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/tauri")>()),
  probeLocalGenerationEndpoint: mocks.probeLocalGenerationEndpoint,
}));

const sampleClis: LlmCliStatusDto[] = [
  {
    id: "claude",
    name: "Claude Code",
    installed: true,
    path: "/usr/local/bin/claude",
    version: "1.0.0",
    structuredOutput: "strict",
    toolsDisabled: true,
  },
  {
    id: "codex",
    name: "Codex",
    installed: false,
    reason: "Not found in your login shell PATH.",
    structuredOutput: "none",
    toolsDisabled: false,
  },
  {
    id: "pi",
    name: "Pi",
    installed: false,
    structuredOutput: "prompt",
    toolsDisabled: true,
  },
  {
    id: "agy",
    name: "Antigravity",
    installed: false,
    structuredOutput: "none",
    toolsDisabled: false,
  },
  {
    id: "cursor-agent",
    name: "Cursor Agent",
    installed: false,
    structuredOutput: "none",
    toolsDisabled: false,
  },
  {
    id: "copilot",
    name: "GitHub Copilot",
    installed: false,
    structuredOutput: "none",
    toolsDisabled: false,
  },
];

const sampleProviders: LlmProvidersDto = {
  endpoints: [
    {
      id: "ep-1",
      name: "Local Ollama",
      baseUrl: "http://localhost:11434/v1",
      modelId: "llama3.2:3b",
      hasApiKey: true,
      structuredOutput: "json_schema",
    },
  ],
  usage: {
    chat: { kind: "clovy" },
    notes: { kind: "clovy" },
    dictationCleanup: { kind: "clovy" },
    activity: { kind: "none" },
  },
  cliLevels: {},
};

describe("LlmProvidersSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.llmProviders.mockResolvedValue(sampleProviders);
    mocks.llmDetectClis.mockResolvedValue(sampleClis);
    mocks.llmSaveEndpoint.mockImplementation(async (req) => ({
      ...sampleProviders,
      endpoints: [
        ...sampleProviders.endpoints,
        {
          id: req.id ?? "ep-new",
          name: req.name,
          baseUrl: req.baseUrl,
          modelId: req.modelId,
          hasApiKey: Boolean(req.apiKey),
        },
      ],
    }));
    mocks.llmDeleteEndpoint.mockResolvedValue(sampleProviders);
    mocks.llmTestProvider.mockResolvedValue({
      latencyMs: 142,
      structuredOutput: "strict",
    });
    mocks.llmSetUsage.mockImplementation(async (usage, provider) => ({
      ...sampleProviders,
      usage: {
        ...sampleProviders.usage,
        [usage]: provider,
      },
    }));
  });

  afterEach(() => {
    cleanup();
    applyInterfaceLocale("en");
  });

  it("renders installed vs not-installed CLIs with path/version vs reason, and disables not-installed in selects", async () => {
    render(<LlmProvidersSection />);

    expect(await screen.findByRole("heading", { name: "Claude Code" })).toBeInTheDocument();
    expect(screen.getByText(/\/usr\/local\/bin\/claude · 1.0.0/)).toBeInTheDocument();

    expect(screen.getByRole("heading", { name: "Codex" })).toBeInTheDocument();
    // Non-installed CLI reasons
    expect(screen.getAllByText("Not found in your login shell PATH.").length).toBeGreaterThan(0);
    // Usage select checks
    const chatSelect = screen.getByRole("combobox", { name: "Chat" });
    const installedOption = chatSelect.querySelector(
      'option[value="cli:claude"]',
    ) as HTMLOptionElement;
    const notInstalledOption = chatSelect.querySelector(
      'option[value="cli:codex"]',
    ) as HTMLOptionElement;

    expect(installedOption).toBeInTheDocument();
    expect(installedOption.disabled).toBe(false);
    expect(installedOption.textContent).toBe("Claude Code");

    expect(notInstalledOption).toBeInTheDocument();
    expect(notInstalledOption.disabled).toBe(true);
    expect(notInstalledOption.textContent).toContain("(not installed)");
  });

  it("offers a CLI whose tools stay on for chat only, never for content uses, and does not test it", async () => {
    mocks.llmDetectClis.mockResolvedValue(
      sampleClis.map((cli) =>
        cli.id === "codex" ? { ...cli, installed: true, path: "/usr/local/bin/codex" } : cli,
      ),
    );
    render(<LlmProvidersSection />);

    expect(await screen.findByText(/\/usr\/local\/bin\/codex/)).toBeInTheDocument();
    const option = (name: string) =>
      screen
        .getByRole("combobox", { name })
        .querySelector('option[value="cli:codex"]') as HTMLOptionElement;
    expect(option("Chat").disabled).toBe(false);
    for (const name of ["Notes", "Dictation cleanup", "Activity"]) {
      expect(option(name).disabled, name).toBe(true);
      expect(option(name).textContent).toBe("Codex (tools can't be turned off)");
    }
    expect(screen.getAllByText(/Its tools can't be turned off/).length).toBe(4);
    // Only Claude Code (installed, tools off) has a test button.
    expect(screen.getAllByRole("button", { name: "Test" })).toHaveLength(1);
  });

  it("test button shows latency and level", async () => {
    const user = userEvent.setup();
    render(<LlmProvidersSection />);

    expect(await screen.findByRole("heading", { name: "Claude Code" })).toBeInTheDocument();
    const testButton = screen.getByRole("button", { name: "Test" });
    await user.click(testButton);

    expect(mocks.llmTestProvider).toHaveBeenCalledWith({ kind: "cli", id: "claude" });
    expect(
      await screen.findByText(/Connected in 142 ms · Structured output: Strict/),
    ).toBeInTheDocument();
  });

  it("add endpoint sends request without echoing key back and shows 'API key saved' from hasApiKey", async () => {
    const user = userEvent.setup();
    render(<LlmProvidersSection />);

    expect(await screen.findByRole("heading", { name: "Local Ollama" })).toBeInTheDocument();
    expect(screen.getByText(/API key saved/)).toBeInTheDocument();

    // Verify the secret key is never rendered in HTML
    expect(screen.queryByText("supersecretkey")).not.toBeInTheDocument();

    const addBtn = screen.getByRole("button", { name: "Add endpoint" });
    await user.click(addBtn);

    expect(screen.getByRole("dialog")).toBeInTheDocument();

    const nameInput = screen.getByLabelText("Name");
    const urlInput = screen.getByLabelText("Base URL");
    const modelInput = screen.getByLabelText("Model ID");
    const keyInput = screen.getByLabelText("API key");

    await user.type(nameInput, "Custom Gateway");
    await user.type(urlInput, "http://localhost:8000/v1");
    await user.type(modelInput, "custom-model");
    await user.type(keyInput, "my-secret-token");

    const saveBtn = screen.getByRole("button", { name: "Save endpoint" });
    await user.click(saveBtn);

    expect(mocks.llmSaveEndpoint).toHaveBeenCalledWith({
      id: undefined,
      name: "Custom Gateway",
      baseUrl: "http://localhost:8000/v1",
      modelId: "custom-model",
      apiKey: "my-secret-token",
      clearApiKey: undefined,
    });
  });

  it("choosing activity provider rejected with llm_structured_output_insufficient shows message and keeps previous value", async () => {
    const user = userEvent.setup();
    mocks.llmSetUsage.mockRejectedValue({
      code: "llm_structured_output_insufficient",
      message: "JSON schema support is required for background activity. Run Test first.",
    });

    render(<LlmProvidersSection />);

    expect(await screen.findByRole("heading", { name: "Local Ollama" })).toBeInTheDocument();

    const activitySelect = screen.getByRole("combobox", { name: "Activity" }) as HTMLSelectElement;
    expect(activitySelect.value).toBe("none");

    await user.selectOptions(activitySelect, "endpoint:ep-1");

    expect(
      await screen.findByText(
        "JSON schema support is required for background activity. Run Test first.",
      ),
    ).toBeInTheDocument();
    // Select keeps the previous value
    expect(activitySelect.value).toBe("none");
  });

  it("default activity selection renders as None", async () => {
    render(<LlmProvidersSection />);
    expect(await screen.findByRole("heading", { name: "Local Ollama" })).toBeInTheDocument();

    const activitySelect = screen.getByRole("combobox", { name: "Activity" }) as HTMLSelectElement;
    expect(activitySelect.value).toBe("none");
    expect(
      screen.getByText("Activity stays off when set to None. Nothing is sent anywhere."),
    ).toBeInTheDocument();
  });

  it("chat set to a CLI shows the later-update note", async () => {
    const user = userEvent.setup();
    render(<LlmProvidersSection />);
    expect(await screen.findByRole("heading", { name: "Local Ollama" })).toBeInTheDocument();

    const chatSelect = screen.getByRole("combobox", { name: "Chat" });
    await user.selectOptions(chatSelect, "cli:claude");

    expect(
      await screen.findByText(
        "Chat with agent CLIs arrives in a later update. Chat keeps using Clovy until then.",
      ),
    ).toBeInTheDocument();
  });

  it("re-reads the registry when the text model changes elsewhere", async () => {
    mocks.llmProviders.mockResolvedValueOnce({
      ...sampleProviders,
      usage: { ...sampleProviders.usage, chat: { kind: "endpoint", id: "ep-1" } },
    });
    render(<LlmProvidersSection />);
    const chatSelect = (await screen.findByRole("combobox", {
      name: "Chat",
    })) as HTMLSelectElement;
    await waitFor(() => expect(chatSelect.value).toBe("endpoint:ep-1"));

    // Picking a Clovy model in the text-model picker moved chat back to Clovy.
    dispatchProviderModelSettingsChanged({ mode: "generation", modelId: "zai-org-glm-5-2" });

    await waitFor(() => expect(chatSelect.value).toBe("clovy"));
  });

  it("loads models of a saved endpoint with its stored key when no key is typed", async () => {
    const user = userEvent.setup();
    mocks.probeLocalGenerationEndpoint.mockResolvedValue({ models: ["llama3.2:3b"] });
    render(<LlmProvidersSection />);

    await user.click(await screen.findByRole("button", { name: "Configure Local Ollama" }));
    await user.click(screen.getByRole("button", { name: "Load models" }));

    expect(mocks.probeLocalGenerationEndpoint).toHaveBeenCalledWith({
      baseUrl: "http://localhost:11434/v1",
      apiKey: "",
      endpointId: "ep-1",
    });

    await user.type(screen.getByLabelText("API key"), "sk-new-key");
    await user.click(screen.getByRole("button", { name: "Load models" }));
    expect(mocks.probeLocalGenerationEndpoint).toHaveBeenLastCalledWith({
      baseUrl: "http://localhost:11434/v1",
      apiKey: "sk-new-key",
      endpointId: "ep-1",
    });
  });

  it("renders properly in pt-BR", async () => {
    applyInterfaceLocale("pt-BR");
    render(<LlmProvidersSection />);

    expect(await screen.findByText("Provedores de LLM")).toBeInTheDocument();
    expect(screen.getByText("Provedores para cada uso")).toBeInTheDocument();
    expect(screen.getByText("CLIs de agentes")).toBeInTheDocument();
    expect(screen.getByText("Endpoints")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Adicionar endpoint" })).toBeInTheDocument();
  });
});
