import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  invoke: (command: string, args?: unknown) =>
    args !== undefined ? invokeMock(command, args) : invokeMock(command),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
  emit: vi.fn(async () => undefined),
}));

import { ActivitySettingsSection } from "../components/settings/ActivitySettingsSection";
import {
  appSettingsTabsForCompanionPairing,
  SETTINGS_TABS,
} from "../components/settings/AppSettings";
import { settingsTabsForCompanionPairing } from "../components/settings/settings-config";
import { applyInterfaceLocale } from "../i18n/locale";
import type { ActivitySettingsDto, ActivityStatusDto } from "../lib/activity-capture";

const defaultSettings: ActivitySettingsDto = {
  enabled: false,
  secondaryMonitors: false,
  inputEvents: true,
  pauseOnProtectedVideo: true,
  ignoredApps: ["Terminal"],
  ignoredDomains: ["google.com"],
  workHours: {
    enabled: false,
    days: [1, 2, 3, 4, 5],
    start: "09:00",
    end: "18:00",
  },
  retentionDays: 30,
};

const defaultStatus: ActivityStatusDto = {
  supported: true,
  settings: defaultSettings,
  state: { kind: "off" },
  permissions: {
    accessibility: "granted",
    screenRecording: "granted",
    inputMonitoring: "granted",
  },
  manualPause: false,
  lastFrameAt: null,
  debugExportAvailable: false,
};

describe("ActivitySettingsSection", () => {
  let currentStatus: ActivityStatusDto;

  beforeEach(() => {
    currentStatus = JSON.parse(JSON.stringify(defaultStatus));
    invokeMock.mockImplementation(async (command: string, args?: { request?: unknown }) => {
      if (command === "activity_status") {
        return currentStatus;
      }
      if (command === "activity_save_settings") {
        const req = args?.request as { settings: ActivitySettingsDto };
        currentStatus = { ...currentStatus, settings: req.settings };
        return currentStatus;
      }
      if (command === "activity_set_paused") {
        const req = args?.request as { paused: boolean };
        currentStatus = {
          ...currentStatus,
          manualPause: req.paused,
          state: req.paused ? { kind: "paused", reason: "manual" } : { kind: "active" },
        };
        return currentStatus;
      }
      if (command === "activity_request_permission") {
        return currentStatus;
      }
      if (command === "open_privacy_settings") {
        return undefined;
      }
      if (command === "activity_recreate_database") {
        currentStatus = { ...currentStatus, state: { kind: "off" } };
        return currentStatus;
      }
      if (command === "activity_debug_export") {
        return {
          path: "/Users/test/activity.sqlite3.export",
          frames: 42,
          secondaryFrames: 5,
          inputEvents: 120,
          pauses: 2,
        };
      }
      throw new Error(`Unhandled command: ${command}`);
    });
  });

  afterEach(() => {
    cleanup();
    applyInterfaceLocale("en");
    vi.clearAllMocks();
  });

  it("shows missing permissions with request and open-settings buttons and needs-permissions state", async () => {
    currentStatus = {
      ...currentStatus,
      settings: { ...defaultSettings, enabled: true },
      state: {
        kind: "needsPermissions",
        missing: ["accessibility", "screenRecording"],
      },
      permissions: {
        accessibility: "denied",
        screenRecording: "notDetermined",
        inputMonitoring: "notDetermined",
      },
    };

    render(<ActivitySettingsSection />);

    await waitFor(() => {
      expect(
        screen.getByText(/Needs permissions: Accessibility, Screen recording/),
      ).toBeInTheDocument();
    });

    const requestButtons = screen.getAllByRole("button", { name: /^Request/ });
    expect(requestButtons).toHaveLength(3);

    const openSettingsButtons = screen.getAllByRole("button", { name: /^Open System Settings/ });
    expect(openSettingsButtons).toHaveLength(3);

    // Request accessibility
    fireEvent.click(screen.getByRole("button", { name: "Request Accessibility permission" }));
    expect(invokeMock).toHaveBeenCalledWith("activity_request_permission", {
      request: { permission: "accessibility" },
    });

    // Open accessibility settings
    fireEvent.click(screen.getByRole("button", { name: "Open System Settings for Accessibility" }));
    expect(invokeMock).toHaveBeenCalledWith("open_privacy_settings", {
      request: { pane: "accessibility" },
    });

    // Request screen recording
    fireEvent.click(screen.getByRole("button", { name: "Request Screen recording permission" }));
    expect(invokeMock).toHaveBeenCalledWith("activity_request_permission", {
      request: { permission: "screenRecording" },
    });

    // Open screen recording settings
    fireEvent.click(
      screen.getByRole("button", { name: "Open System Settings for Screen recording" }),
    );
    expect(invokeMock).toHaveBeenCalledWith("open_privacy_settings", {
      request: { pane: "screenRecording" },
    });

    // Request input monitoring
    fireEvent.click(screen.getByRole("button", { name: "Request Input monitoring permission" }));
    expect(invokeMock).toHaveBeenCalledWith("activity_request_permission", {
      request: { permission: "inputMonitoring" },
    });

    // Open input monitoring settings
    fireEvent.click(
      screen.getByRole("button", { name: "Open System Settings for Input monitoring" }),
    );
    expect(invokeMock).toHaveBeenCalledWith("open_privacy_settings", {
      request: { pane: "inputMonitoring" },
    });
  });

  it("enabling capture sends enabled: true in saved settings", async () => {
    render(<ActivitySettingsSection />);

    await waitFor(() => {
      expect(screen.getByRole("switch", { name: "Capture activity" })).toBeInTheDocument();
    });

    const captureSwitch = screen.getByRole("switch", { name: "Capture activity" });
    expect(captureSwitch).toHaveAttribute("aria-checked", "false");

    fireEvent.click(captureSwitch);

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("activity_save_settings", {
        request: {
          settings: expect.objectContaining({ enabled: true }),
        },
      });
    });
  });

  it("pause/resume calls activity_set_paused", async () => {
    currentStatus = {
      ...currentStatus,
      settings: { ...defaultSettings, enabled: true },
      state: { kind: "active" },
      manualPause: false,
    };

    render(<ActivitySettingsSection />);

    const pauseButton = await screen.findByRole("button", { name: "Pause capture" });
    expect(pauseButton).toBeEnabled();

    fireEvent.click(pauseButton);

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("activity_set_paused", {
        request: { paused: true },
      });
    });

    // Now resume
    const resumeButton = await screen.findByRole("button", { name: "Resume capture" });
    fireEvent.click(resumeButton);

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("activity_set_paused", {
        request: { paused: false },
      });
    });
  });

  it("offers a manual pause, not a no-op resume, while paused automatically", async () => {
    currentStatus = {
      ...currentStatus,
      settings: { ...defaultSettings, enabled: true },
      state: { kind: "paused", reason: "workHours" },
      manualPause: false,
    };

    render(<ActivitySettingsSection />);

    const pauseButton = await screen.findByRole("button", { name: "Pause capture" });
    expect(screen.queryByRole("button", { name: "Resume capture" })).toBeNull();
    fireEvent.click(pauseButton);

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("activity_set_paused", {
        request: { paused: true },
      });
    });
    expect(invokeMock).not.toHaveBeenCalledWith("activity_set_paused", {
      request: { paused: false },
    });
  });

  it("requires confirmation before activity_recreate_database in key-missing flow", async () => {
    currentStatus = {
      ...currentStatus,
      state: { kind: "keyMissing" },
    };

    render(<ActivitySettingsSection />);

    const recreateButton = await screen.findByRole("button", { name: "Recreate database" });
    expect(recreateButton).toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalledWith("activity_recreate_database");

    fireEvent.click(recreateButton);

    // Confirmation dialog appears
    const confirmButton = await screen.findByRole("button", { name: "Recreate" });
    expect(confirmButton).toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalledWith("activity_recreate_database");

    fireEvent.click(confirmButton);

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("activity_recreate_database");
    });
  });

  it("normalizes ignored domain input https://www.YouTube.com/watch to youtube.com", async () => {
    const user = userEvent.setup();
    render(<ActivitySettingsSection />);

    const domainInput = await screen.findByRole("textbox", { name: "Ignored domains" });
    await user.type(domainInput, "https://www.YouTube.com/watch");

    const addDomainButton = screen.getByRole("button", { name: "Add domain" });
    await user.click(addDomainButton);

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("activity_save_settings", {
        request: {
          settings: expect.objectContaining({
            ignoredDomains: ["google.com", "youtube.com"],
          }),
        },
      });
    });
  });

  it("does not add duplicate app twice", async () => {
    const user = userEvent.setup();
    render(<ActivitySettingsSection />);

    const appInput = await screen.findByRole("textbox", { name: "Ignored applications" });
    await user.type(appInput, "terminal"); // "Terminal" already in defaultSettings

    const addAppButton = screen.getByRole("button", { name: "Add application" });
    await user.click(addAppButton);

    // Should NOT call activity_save_settings because terminal is already ignored
    expect(invokeMock).not.toHaveBeenCalledWith("activity_save_settings", expect.anything());
  });

  it("saves the right values for work-hours weekday toggle and retention", async () => {
    render(<ActivitySettingsSection />);

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Sat" })).toBeInTheDocument();
    });

    // Toggle Saturday (day 6)
    fireEvent.click(screen.getByRole("button", { name: "Sat" }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("activity_save_settings", {
        request: {
          settings: expect.objectContaining({
            workHours: expect.objectContaining({
              days: [1, 2, 3, 4, 5, 6],
            }),
          }),
        },
      });
    });

    // Change retention days
    const retentionInput = screen.getByRole("spinbutton", { name: "Retention days" });
    fireEvent.change(retentionInput, { target: { value: "60" } });

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("activity_save_settings", {
        request: {
          settings: expect.objectContaining({
            retentionDays: 60,
          }),
        },
      });
    });
  });

  it("hides debug export button unless debugExportAvailable is true", async () => {
    currentStatus = { ...currentStatus, debugExportAvailable: false };
    const { unmount } = render(<ActivitySettingsSection />);

    await waitFor(() => {
      expect(screen.getByRole("switch", { name: "Capture activity" })).toBeInTheDocument();
    });

    expect(
      screen.queryByRole("button", { name: "Export diagnostic data" }),
    ).not.toBeInTheDocument();
    unmount();

    // Now with debugExportAvailable: true
    currentStatus = { ...currentStatus, debugExportAvailable: true };
    render(<ActivitySettingsSection />);

    const exportButton = await screen.findByRole("button", { name: "Export diagnostic data" });
    expect(exportButton).toBeInTheDocument();

    fireEvent.click(exportButton);

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("activity_debug_export");
      expect(
        screen.getByText(/Exported to \/Users\/test\/activity.sqlite3.export/),
      ).toBeInTheDocument();
    });
  });

  it("renders the tab label Atividade in pt-BR", () => {
    applyInterfaceLocale("pt-BR");
    expect(SETTINGS_TABS.find((tab) => tab.id === "activity")?.label).toBe("Atividade");
  });

  it("hides activity tab on non-macOS platforms in settings-config and AppSettings helpers", () => {
    const originalPlatform = navigator.platform;
    const originalUserAgent = navigator.userAgent;

    try {
      Object.defineProperty(navigator, "platform", {
        configurable: true,
        get: () => "Linux x86_64",
      });
      Object.defineProperty(navigator, "userAgent", {
        configurable: true,
        get: () => "Mozilla/5.0 (X11; Linux x86_64)",
      });

      const companionTabs = settingsTabsForCompanionPairing(true);
      expect(companionTabs.some((t) => t.id === "activity")).toBe(false);

      const appTabs = appSettingsTabsForCompanionPairing(true);
      expect(appTabs.some((t) => t.id === "activity")).toBe(false);
    } finally {
      Object.defineProperty(navigator, "platform", {
        configurable: true,
        get: () => originalPlatform,
      });
      Object.defineProperty(navigator, "userAgent", {
        configurable: true,
        get: () => originalUserAgent,
      });
    }
  });
});
