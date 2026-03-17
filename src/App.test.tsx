import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import type {
  AppConfig,
  BootstrapPayload,
  DebugEventRecord,
  DeviceInfo,
  DeviceLayout,
  EngineSnapshot,
} from "./lib/types";
import { useUiStore } from "./store/uiStore";

let currentBootstrap: BootstrapPayload;

const listenMock = vi.fn(async () => () => undefined);
const invokeMock = vi.fn(async (command: string, args?: Record<string, unknown>) => {
  switch (command) {
    case "bootstrap_load":
      return currentBootstrap;
    case "config_save":
      currentBootstrap = {
        ...currentBootstrap,
        config: args?.config as AppConfig,
      };
      return currentBootstrap;
    case "profiles_update": {
      const updatedProfile = args?.profile as AppConfig["profiles"][number];
      currentBootstrap = {
        ...currentBootstrap,
        config: {
          ...currentBootstrap.config,
          profiles: currentBootstrap.config.profiles.map((profile) =>
            profile.id === updatedProfile.id ? updatedProfile : profile,
          ),
        },
      };
      return currentBootstrap;
    }
    case "profiles_create": {
      const nextProfile = args?.profile as AppConfig["profiles"][number];
      currentBootstrap = {
        ...currentBootstrap,
        config: {
          ...currentBootstrap.config,
          profiles: [...currentBootstrap.config.profiles, nextProfile],
        },
      };
      return currentBootstrap;
    }
    case "profiles_delete": {
      const profileId = args?.profile_id as string;
      currentBootstrap = {
        ...currentBootstrap,
        config: {
          ...currentBootstrap.config,
          activeProfileId: "default",
          profiles: currentBootstrap.config.profiles.filter((profile) => profile.id !== profileId),
        },
      };
      return currentBootstrap;
    }
    case "devices_select_mock": {
      const deviceKey = args?.device_key as string;
      const devices = currentBootstrap.engineSnapshot.devices.map((device) => ({
        ...device,
        connected: device.key === deviceKey,
      }));
      const activeDevice = devices.find((device) => device.key === deviceKey) ?? null;
      currentBootstrap = {
        ...currentBootstrap,
        engineSnapshot: {
          ...currentBootstrap.engineSnapshot,
          devices,
          activeDeviceKey: deviceKey,
          activeDevice,
          engineStatus: {
            ...currentBootstrap.engineSnapshot.engineStatus,
            selectedDeviceKey: deviceKey,
            connected: Boolean(activeDevice),
          },
        },
      };
      return currentBootstrap.engineSnapshot as EngineSnapshot;
    }
    case "import_legacy_config": {
      currentBootstrap = makeImportedBootstrap();
      return {
        config: currentBootstrap.config,
        warnings: [],
        sourcePath: null,
        importedProfiles: currentBootstrap.config.profiles.length,
      };
    }
    default:
      throw new Error(`Unhandled command ${command}`);
  }
});

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: Parameters<typeof invokeMock>) => invokeMock(...args),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: Parameters<typeof listenMock>) => listenMock(...args),
}));

function makeBootstrap(): BootstrapPayload {
  const config: AppConfig = {
    version: 1,
    activeProfileId: "default",
    profiles: [
      {
        id: "default",
        label: "Default (All Apps)",
        appMatchers: [],
        bindings: [
          "middle",
          "gesture_press",
          "gesture_left",
          "gesture_right",
          "gesture_up",
          "gesture_down",
          "back",
          "forward",
          "hscroll_left",
          "hscroll_right",
        ].map((control, index) => ({
          control: control as AppConfig["profiles"][number]["bindings"][number]["control"],
          actionId: index < 2 ? "alt_tab" : "none",
        })),
      },
    ],
    settings: {
      startMinimized: true,
      startAtLogin: false,
      invertHorizontalScroll: false,
      invertVerticalScroll: false,
      dpi: 1200,
      gestureThreshold: 50,
      gestureDeadzone: 40,
      gestureTimeoutMs: 3000,
      gestureCooldownMs: 500,
      appearanceMode: "system",
      debugMode: false,
      deviceLayoutOverrides: {},
    },
  };

  const devices: DeviceInfo[] = [
    {
      key: "mx_master_3s",
      displayName: "MX Master 3S",
      productId: 45108,
      productName: "MX Master 3S",
      transport: "Bluetooth Low Energy",
      source: "mock-catalog",
      uiLayout: "mx_master",
      imageAsset: "/assets/mouse.png",
      supportedControls: config.profiles[0].bindings.map((binding) => binding.control),
      gestureCids: [195, 215],
      dpiMin: 200,
      dpiMax: 8000,
      connected: true,
      batteryLevel: 84,
      currentDpi: 1200,
    },
  ];

  const layouts: DeviceLayout[] = [
    {
      key: "mx_master",
      label: "MX Master family",
      imageAsset: "/assets/mouse.png",
      imageWidth: 460,
      imageHeight: 360,
      interactive: true,
      manualSelectable: true,
      note: "",
      hotspots: [
        {
          control: "middle",
          label: "Middle button",
          summaryType: "mapping",
          normX: 0.35,
          normY: 0.4,
          labelSide: "right",
          labelOffX: 100,
          labelOffY: -160,
          isHscroll: false,
        },
      ],
    },
    {
      key: "generic_mouse",
      label: "Generic mouse",
      imageAsset: "/assets/icons/mouse-simple.svg",
      imageWidth: 220,
      imageHeight: 220,
      interactive: false,
      manualSelectable: false,
      note: "Fallback",
      hotspots: [],
    },
  ];

  const debugLog: DebugEventRecord[] = [
    {
      kind: "info",
      message: "Mock runtime ready",
      timestampMs: 1,
    },
  ];

  return {
    config,
    availableActions: [
      { id: "alt_tab", label: "Alt + Tab", category: "Navigation" },
      { id: "copy", label: "Copy", category: "Editing" },
      { id: "none", label: "Do Nothing", category: "Other" },
    ],
    layouts,
    engineSnapshot: {
      devices,
      activeDeviceKey: "mx_master_3s",
      activeDevice: devices[0],
      engineStatus: {
        enabled: true,
        connected: true,
        activeProfileId: "default",
        frontmostApp: "Finder",
        selectedDeviceKey: "mx_master_3s",
        debugMode: false,
        debugLog,
      },
    },
    platformCapabilities: {
      platform: "macos",
      windowsSupported: true,
      macosSupported: true,
      liveHooksAvailable: false,
      liveHidAvailable: false,
      trayReady: true,
    },
    manualLayoutChoices: [
      { key: "", label: "Auto-detect" },
      { key: "mx_master", label: "MX Master family" },
    ],
  };
}

function makeImportedBootstrap(): BootstrapPayload {
  const next = makeBootstrap();
  next.config = {
    ...next.config,
    activeProfileId: "vscode",
    profiles: [
      ...next.config.profiles,
      {
        id: "vscode",
        label: "VS Code",
        appMatchers: [{ kind: "executable", value: "Code.exe" }],
        bindings: next.config.profiles[0].bindings.map((binding) => ({ ...binding, actionId: "copy" })),
      },
    ],
    settings: {
      ...next.config.settings,
      debugMode: true,
    },
  };
  next.engineSnapshot.engineStatus.activeProfileId = "vscode";
  next.engineSnapshot.engineStatus.debugMode = true;
  return next;
}

function renderApp() {
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={client}>
      <App />
    </QueryClientProvider>,
  );
}

describe("App", () => {
  beforeEach(() => {
    currentBootstrap = makeBootstrap();
    invokeMock.mockClear();
    listenMock.mockClear();
    useUiStore.setState({
      activeSection: "devices",
      selectedProfileId: null,
      importDraft: "",
      eventLog: [],
    });
  });

  it("renders the MX Master layout from bootstrap data", async () => {
    renderApp();
    expect(await screen.findByTestId("device-layout-card")).toBeInTheDocument();
    expect(screen.getByTestId("device-layout-image")).toHaveAttribute("src", "/assets/mouse.png");
  });

  it("saves settings changes through config_save", async () => {
    renderApp();
    const settingsButton = await screen.findByRole("button", { name: "Settings" });
    await userEvent.click(settingsButton);
    const dpiInput = await screen.findByTestId("dpi-input");
    await userEvent.clear(dpiInput);
    await userEvent.type(dpiInput, "900");
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith(
        "config_save",
        expect.objectContaining({
          config: expect.objectContaining({
            settings: expect.objectContaining({ dpi: 900 }),
          }),
        }),
      ),
    );
  });

  it("hydrates the UI from the legacy importer flow", async () => {
    renderApp();
    const debugButton = await screen.findByRole("button", { name: "Debug" });
    await userEvent.click(debugButton);
    await userEvent.click(await screen.findByTestId("legacy-import-button"));
    await userEvent.click(await screen.findByRole("button", { name: "Profiles" }));
    await waitFor(() =>
      expect(screen.getByTestId("profile-label-display")).toHaveTextContent("VS Code"),
    );
  });
});
