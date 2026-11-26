import { act, cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { applyInterfaceLocale } from "../i18n/locale";
import type { LocalTranscriptionModelStatus } from "../lib/tauri";

const api = vi.hoisted(() => ({
  state: vi.fn(),
  install: vi.fn(),
  cancel: vi.fn(),
  remove: vi.fn(),
  listen: vi.fn(),
}));
vi.mock("../lib/tauri", () => ({
  localTranscriptionModelState: api.state,
  installLocalTranscriptionModel: api.install,
  cancelLocalTranscriptionModelInstall: api.cancel,
  removeLocalTranscriptionModel: api.remove,
  onLocalTranscriptionModelState: api.listen,
}));
import { LocalTranscriptionSettings } from "../components/settings/LocalTranscriptionSettings";

const ultra = {
  modelId: "parakeet-ultra",
  name: "Parakeet Ultra",
  minimumMacos: 14,
  compatible: true,
  downloadBytes: 632162964,
  installedBytes: 632162964,
  status: { state: "ready", bytes: 632162964 },
} satisfies LocalTranscriptionModelStatus;
const phonon = {
  modelId: "phonon-2.5",
  name: "Phonon-2.5",
  minimumMacos: 15,
  compatible: true,
  downloadBytes: 289793778,
  installedBytes: 0,
  status: { state: "absent" },
} satisfies LocalTranscriptionModelStatus;

function Harness({ supported = true, initial = "parakeet-ultra" } = {}) {
  const [model, setModel] = useState(initial);
  const provider =
    model === "parakeet-ultra" || model === "phonon-2.5" || model === "fluid-speech-pro"
      ? "local"
      : model.includes("/")
        ? "openrouter"
        : "groq";
  return (
    <LocalTranscriptionSettings
      supported={supported}
      provider={provider}
      model={model}
      openrouterKeyConfigured={false}
      onSelect={async (id) => setModel(id)}
      onActivated={(selection) => setModel(selection.transcriptionModel)}
      onRemoved={async (_modelId, result) => {
        if (result?.switched_to_groq) setModel("whisper-large-v3");
      }}
    />
  );
}

let emit: (model: LocalTranscriptionModelStatus) => void;
beforeEach(() => {
  vi.resetAllMocks();
  applyInterfaceLocale("pt-BR");
  api.state.mockResolvedValue([ultra, phonon]);
  api.cancel.mockResolvedValue(undefined);
  api.listen.mockImplementation(async (listener) => {
    emit = listener;
    return () => {};
  });
});
afterEach(() => {
  cleanup();
  applyInterfaceLocale("en");
});

it("offers OpenRouter transcription even when Local is unavailable", async () => {
  const user = userEvent.setup();
  render(<Harness supported={false} initial="whisper-large-v3-turbo" />);
  await user.selectOptions(await screen.findByRole("combobox", { name: "Provedor" }), "openrouter");
  expect(screen.getByRole("combobox", { name: "Modelo" })).toHaveValue(
    "openai/whisper-large-v3-turbo",
  );
  expect(screen.getByText(/adicione sua chave da OpenRouter/i)).toBeVisible();
  expect(screen.queryByRole("option", { name: "Local" })).toBeNull();
});

it("chooses Whisper large v3 when Groq has no prior selection", async () => {
  const user = userEvent.setup();
  render(<Harness supported={false} initial="openai/whisper-large-v3-turbo" />);
  await user.selectOptions(await screen.findByRole("combobox", { name: "Provedor" }), "groq");
  expect(screen.getByRole("combobox", { name: "Modelo" })).toHaveValue("whisper-large-v3");
});

it("preserves the saved Groq Turbo selection when returning to Groq", async () => {
  const user = userEvent.setup();
  render(<Harness supported={false} initial="whisper-large-v3-turbo" />);
  const provider = await screen.findByRole("combobox", { name: "Provedor" });
  expect(screen.getByRole("combobox", { name: "Modelo" })).toHaveValue("whisper-large-v3-turbo");
  await user.selectOptions(provider, "openrouter");
  await user.selectOptions(provider, "groq");
  expect(screen.getByRole("combobox", { name: "Modelo" })).toHaveValue("whisper-large-v3-turbo");
});

it("focus moves from provider to model and management in keyboard order", async () => {
  const user = userEvent.setup();
  render(<Harness />);
  await screen.findByRole("option", { name: /Phonon/ });
  await user.tab();
  expect(screen.getByRole("combobox", { name: "Provedor" })).toHaveFocus();
  await user.tab();
  expect(screen.getByRole("combobox", { name: "Modelo" })).toHaveFocus();
  await user.tab();
  expect(screen.getByText("Gerenciar modelos locais")).toHaveFocus();
});

it("shows the active model during installation and updates it after activation", async () => {
  const user = userEvent.setup();
  let finish!: (value: unknown) => void;
  api.install.mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  render(<Harness />);
  await screen.findByRole("option", { name: /Phonon/ });
  await user.selectOptions(screen.getByRole("combobox", { name: "Modelo" }), "phonon-2.5");
  act(() =>
    emit({ ...phonon, status: { state: "downloading", received: 1230000, total: 289793778 } }),
  );
  expect(screen.getByRole("progressbar")).toHaveAttribute("value", "1230000");
  expect(screen.getByText(/Parakeet Ultra continua ativo/)).toBeVisible();
  expect(screen.getByRole("combobox", { name: "Modelo" })).toHaveValue("parakeet-ultra");
  await act(async () => {
    emit({ ...phonon, installedBytes: 289793778, status: { state: "ready", bytes: 289793778 } });
    finish({ settings: { transcriptionProvider: "local", transcriptionModel: "phonon-2.5" } });
  });
  expect(screen.getByRole("combobox", { name: "Modelo" })).toHaveValue("phonon-2.5");
});

it("keeps the new pending selection when cancellation of an older download completes", async () => {
  const user = userEvent.setup();
  const pro = { ...phonon, modelId: "fluid-speech-pro", name: "Fluid Speech Pro" };
  api.state.mockResolvedValue([ultra, phonon, pro]);
  let completeCancellation!: () => void;
  api.cancel.mockReturnValue(
    new Promise<void>((resolve) => {
      completeCancellation = resolve;
    }),
  );
  let completePro!: (response: unknown) => void;
  api.install.mockReturnValueOnce(new Promise(() => {})).mockReturnValueOnce(
    new Promise((resolve) => {
      completePro = resolve;
    }),
  );
  render(<Harness />);
  await screen.findByRole("option", { name: /Phonon/ });
  const model = screen.getByRole("combobox", { name: "Modelo" });
  await user.selectOptions(model, "phonon-2.5");
  act(() => emit({ ...phonon, status: { state: "downloading", received: 1, total: 10 } }));
  await user.click(screen.getByRole("button", { name: "Cancelar download de Phonon-2.5" }));
  await user.selectOptions(model, "fluid-speech-pro");
  await act(async () => completeCancellation());
  expect(screen.getByText(/Fluid Speech Pro.*Parakeet Ultra continua ativo/)).toBeVisible();
  await act(async () => {
    emit({ ...pro, status: { state: "ready", bytes: pro.downloadBytes } });
    completePro({ settings: { transcriptionProvider: "local", transcriptionModel: pro.modelId } });
  });
  expect(model).toHaveValue("fluid-speech-pro");
});

it("keeps the newer provider when a response from an older installation arrives", async () => {
  const user = userEvent.setup();
  let finish!: (value: unknown) => void;
  api.install.mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  render(<Harness />);
  await screen.findByRole("option", { name: /Phonon/ });
  await user.selectOptions(screen.getByRole("combobox", { name: "Modelo" }), "phonon-2.5");
  await user.selectOptions(screen.getByRole("combobox", { name: "Provedor" }), "groq");
  await act(async () => {
    finish({ settings: { transcriptionProvider: "local", transcriptionModel: "phonon-2.5" } });
  });
  expect(screen.getByRole("combobox", { name: "Provedor" })).toHaveValue("groq");
});

it("keeps the active model when asked to finish another installation in the background", async () => {
  const user = userEvent.setup();
  let finish!: (value: unknown) => void;
  api.install.mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  render(<Harness />);
  await screen.findByRole("option", { name: /Phonon/ });
  await user.selectOptions(screen.getByRole("combobox", { name: "Modelo" }), "phonon-2.5");
  await user.click(screen.getByRole("button", { name: "Manter Parakeet Ultra" }));
  await act(async () => {
    finish({ settings: { transcriptionProvider: "local", transcriptionModel: "phonon-2.5" } });
  });
  expect(screen.getByRole("combobox", { name: "Modelo" })).toHaveValue("parakeet-ultra");
});

it("preserves the active model through a failed download and a canceled retry", async () => {
  const user = userEvent.setup();
  let finish!: (value: unknown) => void;
  let fail!: (cause: unknown) => void;
  api.install.mockImplementation(
    async () =>
      new Promise((resolve, reject) => {
        finish = resolve;
        fail = reject;
      }),
  );
  api.cancel.mockImplementation(async () => {
    emit(phonon);
    finish({});
  });
  render(<Harness />);
  await screen.findByRole("option", { name: /Phonon/ });
  await user.selectOptions(screen.getByRole("combobox", { name: "Modelo" }), "phonon-2.5");
  await act(async () => {
    emit({ ...phonon, status: { state: "failed", reason: "network: Encoder/weight.bin" } });
    fail({ code: "local_model_install_failed", message: "Download failed" });
  });
  expect(screen.getByRole("alert")).toHaveTextContent(/Encoder\/weight.bin/);
  await user.click(screen.getByRole("button", { name: "Tentar baixar Phonon-2.5 de novo" }));
  act(() => emit({ ...phonon, status: { state: "downloading", received: 1, total: 289793778 } }));
  await user.click(screen.getByRole("button", { name: "Cancelar download de Phonon-2.5" }));
  expect(screen.queryByRole("progressbar")).toBeNull();
  expect(screen.getByRole("combobox", { name: "Modelo" })).toHaveValue("parakeet-ultra");
});

it("shows operating system requirements next to unavailable models", async () => {
  api.state.mockResolvedValue([ultra, { ...phonon, compatible: false }]);
  render(<Harness />);
  expect(await screen.findByRole("option", { name: /Phonon.*macOS 15/ })).toBeDisabled();
  expect(screen.getByRole("option", { name: /Parakeet Ultra/ })).toBeEnabled();
});

it("removes an inactive model independently and confirms the active model's switch to Groq", async () => {
  const user = userEvent.setup();
  api.state.mockResolvedValue([
    ultra,
    { ...phonon, installedBytes: 289793778, status: { state: "ready", bytes: 289793778 } },
  ]);
  api.remove.mockImplementation(async (id) => {
    const entry = id === ultra.modelId ? ultra : phonon;
    emit({ ...entry, installedBytes: 0, status: { state: "absent" } });
    return { switched_to_groq: id === ultra.modelId };
  });
  render(<Harness />);
  await user.click(await screen.findByText("Gerenciar modelos locais"));
  await user.click(screen.getByRole("button", { name: "Remover Phonon-2.5" }));
  expect(screen.getByRole("combobox", { name: "Modelo" })).toHaveValue("parakeet-ultra");
  await user.click(screen.getByRole("button", { name: "Remover Parakeet Ultra" }));
  const dialog = await screen.findByRole("dialog");
  expect(dialog).toHaveTextContent(/Groq/);
  await user.click(within(dialog).getByRole("button", { name: "Remover e usar Groq" }));
  expect(screen.getByRole("combobox", { name: "Provedor" })).toHaveValue("groq");
});
