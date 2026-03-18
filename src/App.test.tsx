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
} from "./lib/types";
import { useUiStore } from "./store/uiStore";

let currentBootstrap: BootstrapPayload;

function normalizedIdentityKey(identityKey: string | null | undefined) {
  const trimmed = identityKey?.trim();
  return trimmed ? trimmed : null;
}

function samePhysicalDevice(left: DeviceInfo, right: DeviceInfo) {
  const leftIdentity = normalizedIdentityKey(left.fingerprint?.identityKey);
  const rightIdentity = normalizedIdentityKey(right.fingerprint?.identityKey);
  if (leftIdentity != null && rightIdentity != null) {
    return leftIdentity === rightIdentity;
  }
  if (leftIdentity != null || rightIdentity != null) {
    return false;
  }
  return left.modelKey === right.modelKey;
}

const apiMocks = vi.hoisted(() => ({
  bootstrapLoad: vi.fn(),
  configSave: vi.fn(),
  appSettingsUpdate: vi.fn(),
  deviceDefaultsUpdate: vi.fn(),
  profilesCreate: vi.fn(),
  profilesUpdate: vi.fn(),
  profilesDelete: vi.fn(),
  devicesAdd: vi.fn(),
  devicesUpdateSettings: vi.fn(),
  devicesUpdateProfile: vi.fn(),
  devicesUpdateNickname: vi.fn(),
  devicesRemove: vi.fn(),
  devicesSelect: vi.fn(),
  devicesSelectMock: vi.fn(),
  importLegacyConfig: vi.fn(),
  debugClearLog: vi.fn(),
  deviceChangedListen: vi.fn(async () => () => undefined),
  profileChangedListen: vi.fn(async () => () => undefined),
  engineStatusChangedListen: vi.fn(async () => () => undefined),
  debugEventListen: vi.fn(async () => () => undefined),
}));

vi.mock("./lib/api", () => ({
  bootstrapLoad: apiMocks.bootstrapLoad,
  configSave: apiMocks.configSave,
  appSettingsUpdate: apiMocks.appSettingsUpdate,
  deviceDefaultsUpdate: apiMocks.deviceDefaultsUpdate,
  profilesCreate: apiMocks.profilesCreate,
  profilesUpdate: apiMocks.profilesUpdate,
  profilesDelete: apiMocks.profilesDelete,
  devicesAdd: apiMocks.devicesAdd,
  devicesUpdateSettings: apiMocks.devicesUpdateSettings,
  devicesUpdateProfile: apiMocks.devicesUpdateProfile,
  devicesUpdateNickname: apiMocks.devicesUpdateNickname,
  devicesRemove: apiMocks.devicesRemove,
  devicesSelect: apiMocks.devicesSelect,
  devicesSelectMock: apiMocks.devicesSelectMock,
  importLegacyConfig: apiMocks.importLegacyConfig,
  debugClearLog: apiMocks.debugClearLog,
  events: {
    deviceChangedEvent: { listen: apiMocks.deviceChangedListen },
    profileChangedEvent: { listen: apiMocks.profileChangedListen },
    engineStatusChangedEvent: { listen: apiMocks.engineStatusChangedListen },
    debugEventEnvelope: { listen: apiMocks.debugEventListen },
  },
}));

function makeBootstrap(): BootstrapPayload {
  const config: AppConfig = {
    version: 3,
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
          control:
            control as AppConfig["profiles"][number]["bindings"][number]["control"],
          actionId: index < 2 ? "alt_tab" : "none",
        })),
      },
    ],
    managedDevices: [
      {
        id: "mx_master_3s",
        modelKey: "mx_master_3s",
        displayName: "MX Master 3S",
        nickname: null,
        profileId: null,
        identityKey: "mock:mx_master_3s:1",
        settings: {
          dpi: 1200,
          invertHorizontalScroll: false,
          invertVerticalScroll: false,
          macosThumbWheelSimulateTrackpad: false,
          macosThumbWheelTrackpadHoldTimeoutMs: 500,
          gestureThreshold: 50,
          gestureDeadzone: 40,
          gestureTimeoutMs: 3000,
          gestureCooldownMs: 500,
          manualLayoutOverride: null,
        },
        createdAtMs: 1,
        lastSeenAtMs: 1,
        lastSeenTransport: "Bluetooth Low Energy",
      },
    ],
    settings: {
      startMinimized: true,
      startAtLogin: false,
      appearanceMode: "system",
      debugMode: false,
    },
    deviceDefaults: {
      dpi: 1200,
      invertHorizontalScroll: false,
      invertVerticalScroll: false,
      macosThumbWheelSimulateTrackpad: false,
      macosThumbWheelTrackpadHoldTimeoutMs: 500,
      gestureThreshold: 50,
      gestureDeadzone: 40,
      gestureTimeoutMs: 3000,
      gestureCooldownMs: 500,
      manualLayoutOverride: null,
    },
  };

  const devices: DeviceInfo[] = [
    {
      key: "mx_master_3s",
      modelKey: "mx_master_3s",
      displayName: "MX Master 3S",
      nickname: null,
      productId: 45108,
      productName: "MX Master 3S",
      transport: "Bluetooth Low Energy",
      source: "mock-catalog",
      uiLayout: "mx_master",
      imageAsset: "/assets/mouse.png",
      supportedControls: config.profiles[0].bindings.map(
        (binding) => binding.control,
      ),
      gestureCids: [195, 215],
      dpiMin: 200,
      dpiMax: 8000,
      connected: true,
      batteryLevel: 84,
      currentDpi: 1200,
      fingerprint: {
        identityKey: "mock:mx_master_3s:1",
        serialNumber: null,
        hidPath: null,
        interfaceNumber: null,
        usagePage: null,
        usage: null,
        locationId: null,
      },
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
    knownApps: [
      {
        executable: "Code.exe",
        label: "VS Code",
        iconAsset: "/assets/apps/vscode.png",
      },
      {
        executable: "msedge.exe",
        label: "Microsoft Edge",
        iconAsset: null,
      },
    ],
    supportedDevices: [
      {
        key: "mx_master_3s",
        displayName: "MX Master 3S",
        productIds: [45108],
        aliases: ["Logitech MX Master 3S"],
        gestureCids: [195, 215],
        uiLayout: "mx_master",
        imageAsset: "/assets/mouse.png",
        supportedControls: config.profiles[0].bindings.map(
          (binding) => binding.control,
        ),
        dpiMin: 200,
        dpiMax: 8000,
      },
      {
        key: "mx_anywhere_3s",
        displayName: "MX Anywhere 3S",
        productIds: [45111],
        aliases: ["Logitech MX Anywhere 3S"],
        gestureCids: [195],
        uiLayout: "generic_mouse",
        imageAsset: "/assets/icons/mouse-simple.svg",
        supportedControls: config.profiles[0].bindings.map(
          (binding) => binding.control,
        ),
        dpiMin: 200,
        dpiMax: 8000,
      },
    ],
    layouts,
    engineSnapshot: {
      devices,
      detectedDevices: devices,
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
      mappingEngineReady: false,
      gestureDiversionAvailable: false,
      activeHidBackend: "macos-hidapi",
      activeHookBackend: "macos-eventtap-stub",
      activeFocusBackend: "macos-nsworkspace",
      hidapiAvailable: true,
      iokitAvailable: false,
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
        bindings: next.config.profiles[0].bindings.map((binding) => ({
          ...binding,
          actionId: "copy",
        })),
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

function makeWindowsBootstrap(): BootstrapPayload {
  const next = makeBootstrap();
  next.platformCapabilities = {
    ...next.platformCapabilities,
    platform: "windows",
  };
  return next;
}

function makeGenericMouseBootstrap(): BootstrapPayload {
  const next = makeBootstrap();
  next.config = {
    ...next.config,
    managedDevices: next.config.managedDevices?.map((device) =>
      device.id === "mx_master_3s"
        ? {
            ...device,
            modelKey: "generic_mouse",
            displayName: "Generic Mouse",
          }
        : device,
    ),
  };
  next.engineSnapshot = {
    ...next.engineSnapshot,
    devices: next.engineSnapshot.devices.map((device) =>
      device.key === "mx_master_3s"
        ? {
            ...device,
            modelKey: "generic_mouse",
            displayName: "Generic Mouse",
            uiLayout: "generic_mouse",
            imageAsset: "/assets/icons/mouse-simple.svg",
          }
        : device,
    ),
    detectedDevices: next.engineSnapshot.detectedDevices.map((device) =>
      device.key === "mx_master_3s"
        ? {
            ...device,
            modelKey: "generic_mouse",
            displayName: "Generic Mouse",
            uiLayout: "generic_mouse",
            imageAsset: "/assets/icons/mouse-simple.svg",
          }
        : device,
    ),
    activeDevice: next.engineSnapshot.activeDevice
      ? {
          ...next.engineSnapshot.activeDevice,
          modelKey: "generic_mouse",
          displayName: "Generic Mouse",
          uiLayout: "generic_mouse",
          imageAsset: "/assets/icons/mouse-simple.svg",
        }
      : null,
  };
  return next;
}

function renderApp() {
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  const user = userEvent.setup();

  return {
    user,
    ...render(
      <QueryClientProvider client={client}>
        <App />
      </QueryClientProvider>,
    ),
  };
}

describe("App", () => {
  beforeEach(() => {
    currentBootstrap = makeBootstrap();

    apiMocks.bootstrapLoad.mockReset();
    apiMocks.configSave.mockReset();
    apiMocks.appSettingsUpdate.mockReset();
    apiMocks.deviceDefaultsUpdate.mockReset();
    apiMocks.profilesCreate.mockReset();
    apiMocks.profilesUpdate.mockReset();
    apiMocks.profilesDelete.mockReset();
    apiMocks.devicesAdd.mockReset();
    apiMocks.devicesUpdateSettings.mockReset();
    apiMocks.devicesUpdateProfile.mockReset();
    apiMocks.devicesUpdateNickname.mockReset();
    apiMocks.devicesRemove.mockReset();
    apiMocks.devicesSelect.mockReset();
    apiMocks.devicesSelectMock.mockReset();
    apiMocks.importLegacyConfig.mockReset();
    apiMocks.debugClearLog.mockReset();
    apiMocks.deviceChangedListen.mockClear();
    apiMocks.profileChangedListen.mockClear();
    apiMocks.engineStatusChangedListen.mockClear();
    apiMocks.debugEventListen.mockClear();

    apiMocks.bootstrapLoad.mockImplementation(async () => currentBootstrap);
    apiMocks.configSave.mockImplementation(async (config: AppConfig) => {
      currentBootstrap = {
        ...currentBootstrap,
        config,
      };
      return currentBootstrap;
    });
    apiMocks.appSettingsUpdate.mockImplementation(
      async (settings: AppConfig["settings"]) => {
        currentBootstrap = {
          ...currentBootstrap,
          config: {
            ...currentBootstrap.config,
            settings,
          },
          engineSnapshot: {
            ...currentBootstrap.engineSnapshot,
            engineStatus: {
              ...currentBootstrap.engineSnapshot.engineStatus,
              debugMode: settings.debugMode,
            },
          },
        };
        return currentBootstrap;
      },
    );
    apiMocks.deviceDefaultsUpdate.mockImplementation(
      async (deviceDefaults: NonNullable<AppConfig["deviceDefaults"]>) => {
        currentBootstrap = {
          ...currentBootstrap,
          config: {
            ...currentBootstrap.config,
            deviceDefaults,
          },
        };
        return currentBootstrap;
      },
    );
    apiMocks.profilesUpdate.mockImplementation(
      async (updatedProfile: AppConfig["profiles"][number]) => {
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
      },
    );
    apiMocks.profilesCreate.mockImplementation(
      async (nextProfile: AppConfig["profiles"][number]) => {
        currentBootstrap = {
          ...currentBootstrap,
          config: {
            ...currentBootstrap.config,
            profiles: [...currentBootstrap.config.profiles, nextProfile],
          },
        };
        return currentBootstrap;
      },
    );
    apiMocks.profilesDelete.mockImplementation(async (profileId: string) => {
      currentBootstrap = {
        ...currentBootstrap,
        config: {
          ...currentBootstrap.config,
          activeProfileId: "default",
          managedDevices: (currentBootstrap.config.managedDevices ?? []).map(
            (device) =>
              device.profileId === profileId
                ? { ...device, profileId: null }
                : device,
          ),
          profiles: currentBootstrap.config.profiles.filter(
            (profile) => profile.id !== profileId,
          ),
        },
      };
      return currentBootstrap;
    });
    apiMocks.devicesAdd.mockImplementation(async (modelKey: string) => {
      const supportedDevice = currentBootstrap.supportedDevices.find(
        (device) => device.key === modelKey,
      );
      if (!supportedDevice) {
        return currentBootstrap;
      }

      const nextDevice: DeviceInfo = {
        key: `${modelKey}-${(currentBootstrap.config.managedDevices?.length ?? 0) + 1}`,
        modelKey,
        displayName: supportedDevice.displayName,
        nickname: null,
        productId: supportedDevice.productIds[0] ?? null,
        productName: supportedDevice.displayName,
        transport: null,
        source: "managed",
        uiLayout: supportedDevice.uiLayout,
        imageAsset: supportedDevice.imageAsset,
        supportedControls: supportedDevice.supportedControls,
        gestureCids: supportedDevice.gestureCids,
        dpiMin: supportedDevice.dpiMin,
        dpiMax: supportedDevice.dpiMax,
        connected: false,
        batteryLevel: null,
        currentDpi: currentBootstrap.config.deviceDefaults?.dpi ?? 1200,
        fingerprint: {
          identityKey: `mock:${modelKey}:${(currentBootstrap.config.managedDevices?.length ?? 0) + 1}`,
          serialNumber: null,
          hidPath: null,
          interfaceNumber: null,
          usagePage: null,
          usage: null,
          locationId: null,
        },
      };

      currentBootstrap = {
        ...currentBootstrap,
        config: {
          ...currentBootstrap.config,
          managedDevices: [
            ...(currentBootstrap.config.managedDevices ?? []),
            {
              id: nextDevice.key,
              modelKey,
              displayName: supportedDevice.displayName,
              nickname: null,
              profileId: null,
              identityKey: nextDevice.fingerprint?.identityKey ?? null,
              settings: currentBootstrap.config.deviceDefaults,
              createdAtMs: 1,
              lastSeenAtMs: null,
              lastSeenTransport: null,
            },
          ],
        },
        engineSnapshot: {
          ...currentBootstrap.engineSnapshot,
          devices: [...currentBootstrap.engineSnapshot.devices, nextDevice],
        },
      };
      return currentBootstrap;
    });
    apiMocks.devicesUpdateSettings.mockImplementation(
      async (
        deviceKey: string,
        settings: NonNullable<AppConfig["deviceDefaults"]>,
      ) => {
        currentBootstrap = {
          ...currentBootstrap,
          config: {
            ...currentBootstrap.config,
            managedDevices: (currentBootstrap.config.managedDevices ?? []).map(
              (device) =>
                device.id === deviceKey ? { ...device, settings } : device,
            ),
          },
          engineSnapshot: {
            ...currentBootstrap.engineSnapshot,
            devices: currentBootstrap.engineSnapshot.devices.map((device) =>
              device.key === deviceKey
                ? { ...device, currentDpi: settings.dpi }
                : device,
            ),
            activeDevice:
              currentBootstrap.engineSnapshot.activeDevice?.key === deviceKey
                ? {
                    ...currentBootstrap.engineSnapshot.activeDevice,
                    currentDpi: settings.dpi,
                  }
                : currentBootstrap.engineSnapshot.activeDevice,
          },
        };
        return currentBootstrap;
      },
    );
    apiMocks.devicesUpdateProfile.mockImplementation(
      async (deviceKey: string, profileId: string | null) => {
        const nextActiveProfileId =
          currentBootstrap.engineSnapshot.engineStatus.selectedDeviceKey ===
          deviceKey
            ? (profileId ?? "default")
            : currentBootstrap.config.activeProfileId;
        currentBootstrap = {
          ...currentBootstrap,
          config: {
            ...currentBootstrap.config,
            activeProfileId: nextActiveProfileId,
            managedDevices: (currentBootstrap.config.managedDevices ?? []).map(
              (device) =>
                device.id === deviceKey ? { ...device, profileId } : device,
            ),
          },
          engineSnapshot: {
            ...currentBootstrap.engineSnapshot,
            engineStatus: {
              ...currentBootstrap.engineSnapshot.engineStatus,
              activeProfileId: nextActiveProfileId,
            },
          },
        };
        return currentBootstrap;
      },
    );
    apiMocks.devicesUpdateNickname.mockImplementation(
      async (deviceKey: string, nickname: string | null) => {
        currentBootstrap = {
          ...currentBootstrap,
          config: {
            ...currentBootstrap.config,
            managedDevices: (currentBootstrap.config.managedDevices ?? []).map(
              (device) =>
                device.id === deviceKey ? { ...device, nickname } : device,
            ),
          },
          engineSnapshot: {
            ...currentBootstrap.engineSnapshot,
            devices: currentBootstrap.engineSnapshot.devices.map((device) =>
              device.key === deviceKey ? { ...device, nickname } : device,
            ),
            activeDevice:
              currentBootstrap.engineSnapshot.activeDevice?.key === deviceKey
                ? {
                    ...currentBootstrap.engineSnapshot.activeDevice,
                    nickname,
                  }
                : currentBootstrap.engineSnapshot.activeDevice,
          },
        };
        return currentBootstrap;
      },
    );
    apiMocks.devicesRemove.mockImplementation(async (deviceKey: string) => {
      currentBootstrap = {
        ...currentBootstrap,
        config: {
          ...currentBootstrap.config,
          managedDevices: (currentBootstrap.config.managedDevices ?? []).filter(
            (device) => device.id !== deviceKey,
          ),
        },
        engineSnapshot: {
          ...currentBootstrap.engineSnapshot,
          devices: currentBootstrap.engineSnapshot.devices.filter(
            (device) => device.key !== deviceKey,
          ),
          activeDeviceKey:
            currentBootstrap.engineSnapshot.activeDeviceKey === deviceKey
              ? null
              : currentBootstrap.engineSnapshot.activeDeviceKey,
          activeDevice:
            currentBootstrap.engineSnapshot.activeDevice?.key === deviceKey
              ? null
              : currentBootstrap.engineSnapshot.activeDevice,
        },
      };
      return currentBootstrap;
    });
    apiMocks.devicesSelect.mockImplementation(async (deviceKey: string) => {
      const devices = currentBootstrap.engineSnapshot.devices.map((device) => ({
        ...device,
        connected:
          device.key === deviceKey &&
          currentBootstrap.engineSnapshot.detectedDevices.some((detected) =>
            samePhysicalDevice(detected, device),
          ),
      }));
      const activeDevice =
        devices.find((device) => device.key === deviceKey) ?? null;
      const selectedManagedDevice =
        currentBootstrap.config.managedDevices?.find(
          (device) => device.id === deviceKey,
        );
      const nextActiveProfileId = selectedManagedDevice?.profileId ?? "default";
      currentBootstrap = {
        ...currentBootstrap,
        config: {
          ...currentBootstrap.config,
          activeProfileId: nextActiveProfileId,
        },
        engineSnapshot: {
          ...currentBootstrap.engineSnapshot,
          devices,
          activeDeviceKey: deviceKey,
          activeDevice,
          engineStatus: {
            ...currentBootstrap.engineSnapshot.engineStatus,
            activeProfileId: nextActiveProfileId,
            selectedDeviceKey: deviceKey,
            connected: Boolean(activeDevice),
          },
        },
      };
      return currentBootstrap.engineSnapshot;
    });
    apiMocks.devicesSelectMock.mockImplementation(apiMocks.devicesSelect);
    apiMocks.importLegacyConfig.mockImplementation(async () => {
      currentBootstrap = makeImportedBootstrap();
      return {
        config: currentBootstrap.config,
        warnings: [],
        sourcePath: null,
        importedProfiles: currentBootstrap.config.profiles.length,
      };
    });
    apiMocks.debugClearLog.mockImplementation(async () => {
      currentBootstrap = {
        ...currentBootstrap,
        engineSnapshot: {
          ...currentBootstrap.engineSnapshot,
          engineStatus: {
            ...currentBootstrap.engineSnapshot.engineStatus,
            debugLog: [],
          },
        },
      };
      return currentBootstrap.engineSnapshot;
    });

    useUiStore.setState({
      shellMode: "dashboard",
      activeSection: "devices",
      selectedProfileId: null,
      importDraft: "",
      eventLog: [],
    });
  });

  it("renders the MX Master layout from bootstrap data", async () => {
    renderApp();
    expect(await screen.findByTestId("device-layout-card")).toBeInTheDocument();
    expect(screen.getByTestId("device-layout-image")).toHaveAttribute(
      "src",
      "/assets/mouse.png",
    );
  });

  it("opens the buttons sheet and updates a control mapping", async () => {
    const { user } = renderApp();

    await user.click(await screen.findByRole("button", { name: "Buttons" }));
    expect(
      screen.queryByTestId("buttons-editor-sheet"),
    ).not.toBeInTheDocument();

    await user.click(await screen.findByTestId("hotspot-card-middle"));
    expect(
      await screen.findByTestId("buttons-editor-sheet"),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("combobox", { name: "Middle button" }));
    await user.click(await screen.findByRole("option", { name: "Copy" }));

    await waitFor(() => {
      expect(apiMocks.profilesUpdate).toHaveBeenCalled();
      const calls = apiMocks.profilesUpdate.mock.calls;
      const lastCall = calls[calls.length - 1]?.[0];
      expect(lastCall?.bindings).toEqual(
        expect.arrayContaining([
          expect.objectContaining({ control: "middle", actionId: "copy" }),
        ]),
      );
    });
  });

  it("saves device tuning changes through the debounced DPI controls", async () => {
    const { user } = renderApp();
    await user.click(await screen.findByRole("button", { name: "Tune" }));
    await user.click(await screen.findByTestId("dpi-preset-1600"));

    await waitFor(() => {
      expect(apiMocks.devicesUpdateSettings).toHaveBeenCalled();
      const calls = apiMocks.devicesUpdateSettings.mock.calls;
      const lastCall = calls[calls.length - 1];
      expect(lastCall?.[0]).toBe("mx_master_3s");
      expect(lastCall?.[1]).toEqual(
        expect.objectContaining({
          dpi: 1600,
        }),
      );
    });
  });

  it("shows and saves the macOS thumb wheel trackpad beta toggle for MX Master devices", async () => {
    const { user } = renderApp();
    await user.click(await screen.findByRole("button", { name: "Tune" }));

    const betaSwitch = await screen.findByRole("switch", {
      name: /Simulate trackpad swipe from thumb wheel/i,
    });
    expect(betaSwitch).toBeInTheDocument();

    await user.click(betaSwitch);

    await waitFor(() => {
      expect(apiMocks.devicesUpdateSettings).toHaveBeenCalled();
      const calls = apiMocks.devicesUpdateSettings.mock.calls;
      const lastCall = calls[calls.length - 1];
      expect(lastCall?.[0]).toBe("mx_master_3s");
      expect(lastCall?.[1]).toEqual(
        expect.objectContaining({
          macosThumbWheelSimulateTrackpad: true,
        }),
      );
    });
  });

  it("shows and saves the thumb wheel swipe hold timeout when the beta toggle is enabled", async () => {
    const { user } = renderApp();
    await user.click(await screen.findByRole("button", { name: "Tune" }));

    await user.click(
      await screen.findByRole("switch", {
        name: /Simulate trackpad swipe from thumb wheel/i,
      }),
    );

    const timeoutInput = await screen.findByLabelText(
      /Thumb wheel swipe hold \(ms\)/i,
    );
    await user.clear(timeoutInput);
    await user.type(timeoutInput, "900");

    await waitFor(() => {
      expect(apiMocks.devicesUpdateSettings).toHaveBeenCalled();
      const calls = apiMocks.devicesUpdateSettings.mock.calls;
      const lastCall = calls[calls.length - 1];
      expect(lastCall?.[0]).toBe("mx_master_3s");
      expect(lastCall?.[1]).toEqual(
        expect.objectContaining({
          macosThumbWheelTrackpadHoldTimeoutMs: 900,
        }),
      );
    });
  });

  it("hides the beta toggle when the active device is not an MX Master family mouse", async () => {
    currentBootstrap = makeGenericMouseBootstrap();

    const { user } = renderApp();
    await user.click(await screen.findByRole("button", { name: "Tune" }));

    expect(
      screen.queryByRole("switch", {
        name: /Simulate trackpad swipe from thumb wheel/i,
      }),
    ).not.toBeInTheDocument();
  });

  it("hides the beta toggle on non-macOS platforms", async () => {
    currentBootstrap = makeWindowsBootstrap();

    const { user } = renderApp();
    await user.click(await screen.findByRole("button", { name: "Tune" }));

    expect(
      screen.queryByRole("switch", {
        name: /Simulate trackpad swipe from thumb wheel/i,
      }),
    ).not.toBeInTheDocument();
  });

  it("hydrates the UI from the legacy importer flow", async () => {
    const { user } = renderApp();
    await user.click(await screen.findByTestId("device-layout-card"));
    await user.click(await screen.findByRole("button", { name: "Debug" }));
    await user.click(await screen.findByTestId("legacy-import-button"));
    await user.click(await screen.findByRole("button", { name: "Profiles" }));

    await waitFor(() =>
      expect(screen.getByTestId("profile-label-display")).toHaveTextContent(
        "VS Code",
      ),
    );
  });

  it("prefers source-path imports over raw JSON when a path is provided", async () => {
    const { user } = renderApp();
    await user.click(await screen.findByTestId("device-layout-card"));
    await user.click(await screen.findByRole("button", { name: "Debug" }));

    await user.type(
      await screen.findByPlaceholderText(
        "~/Library/Application Support/Mouser/config.json",
      ),
      "/tmp/legacy-config.json",
    );
    await user.click(await screen.findByTestId("legacy-import-button"));

    await waitFor(() => {
      expect(apiMocks.importLegacyConfig).toHaveBeenCalled();
      const calls = apiMocks.importLegacyConfig.mock.calls;
      const lastCall = calls[calls.length - 1];
      expect(lastCall?.[0]).toEqual({
        sourcePath: "/tmp/legacy-config.json",
        rawJson: null,
      });
    });
  });

  it("opens app settings in a global dialog", async () => {
    const { user } = renderApp();
    await user.click(
      await screen.findByRole("button", { name: "App settings" }),
    );
    expect(await screen.findByText("App Settings")).toBeInTheDocument();
    await user.click(screen.getByText("Start at login"));

    await waitFor(() => {
      expect(apiMocks.appSettingsUpdate).toHaveBeenCalled();
      const calls = apiMocks.appSettingsUpdate.mock.calls;
      const lastCall = calls[calls.length - 1];
      expect(lastCall?.[0]).toEqual(
        expect.objectContaining({ startAtLogin: true }),
      );
    });
  });

  it("keeps the dashboard usable when there is no active device", async () => {
    currentBootstrap = {
      ...makeBootstrap(),
      engineSnapshot: {
        ...makeBootstrap().engineSnapshot,
        activeDeviceKey: null,
        activeDevice: null,
        engineStatus: {
          ...makeBootstrap().engineSnapshot.engineStatus,
          connected: false,
          selectedDeviceKey: null,
        },
      },
    };

    renderApp();

    expect(await screen.findByText("MX Master 3S")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "App settings" }),
    ).toBeInTheDocument();
  });
});
