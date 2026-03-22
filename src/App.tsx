import {
  useEffect,
  useId,
  useRef,
  useState,
  type ComponentProps,
  type ReactNode,
  type RefObject,
  type SVGProps,
} from "react";
import { AnimatePresence, motion } from "framer-motion";
import {
  ArrowCounterClockwise,
  CaretLeft,
  BugBeetle,
  GearSix,
  MouseLeftClick,
  MouseScroll,
  Plus,
  Stack,
  WarningCircle,
  X,
} from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "./components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "./components/ui/dialog";
import { Input } from "./components/ui/input";
import { Label } from "./components/ui/label";
import { ScrollArea } from "./components/ui/scroll-area";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "./components/ui/select";
import { Slider } from "./components/ui/slider";
import { Switch } from "./components/ui/switch";
import { Textarea } from "./components/ui/textarea";
import {
  appIconLoad,
  appDiscoveryRefresh,
  bootstrapLoad,
  configSave,
  devicesAdd,
  devicesResetToFactory,
  devicesRemove,
  devicesSelect,
  importLegacyConfig,
} from "./lib/api";
import { sampleLegacyConfig } from "./lib/sampleLegacyConfig";
import type {
  ActionDefinition,
  AppConfig,
  AppMatcher,
  AppMatcherKind,
  BackendHealthState,
  Binding,
  BootstrapPayload,
  DeviceControlSpec,
  DeviceSupportLevel,
  DeviceInfo,
  DeviceLayout,
  DiscoveredApp,
  ImportLegacyRequest,
  KnownDeviceSpec,
  LogicalControl,
  Profile,
  RuntimeHealth,
} from "./lib/types";
import { useRuntimeEvents } from "./hooks/useRuntimeEvents";
import { cn } from "./lib/utils";
import { type SectionName, useUiStore } from "./store/uiStore";

const SECTION_ORDER: SectionName[] = [
  "buttons",
  "devices",
  "profiles",
  "debug",
];

const SECTION_META: Record<
  SectionName,
  {
    label: string;
    icon: (props: SVGProps<SVGSVGElement>) => ReactNode;
  }
> = {
  buttons: {
    label: "Buttons",
    icon: MouseLeftClick,
  },
  devices: {
    label: "Point & Scroll",
    icon: MouseScroll,
  },
  profiles: {
    label: "Profiles",
    icon: Stack,
  },
  debug: {
    label: "Debug",
    icon: BugBeetle,
  },
};

const CONTROL_LABELS: Record<LogicalControl, string> = {
  middle: "Middle button",
  gesture_press: "Gesture button",
  gesture_left: "Gesture left",
  gesture_right: "Gesture right",
  gesture_up: "Gesture up",
  gesture_down: "Gesture down",
  back: "Back button",
  forward: "Forward button",
  hscroll_left: "Thumb wheel left",
  hscroll_right: "Thumb wheel right",
  smartshift_toggle: "SmartShift toggle",
  mission_control_button: "Mission Control button",
  smart_zoom_button: "Smart Zoom button",
  precision_mode_button: "Precision mode button",
  dpi_button: "DPI button",
  emoji_button: "Emoji button",
};

const GESTURE_CONTROLS: LogicalControl[] = [
  "gesture_press",
  "gesture_left",
  "gesture_right",
  "gesture_up",
  "gesture_down",
];

const HSCROLL_CONTROLS: LogicalControl[] = ["hscroll_left", "hscroll_right"];

const PRIMARY_CONTROL_ORDER: LogicalControl[] = [
  "middle",
  "back",
  "forward",
  "gesture_press",
  "hscroll_left",
  "smartshift_toggle",
  "mission_control_button",
  "smart_zoom_button",
  "precision_mode_button",
  "dpi_button",
  "emoji_button",
];

type AppSelectOption = {
  label: string;
  value: string;
  group?: string;
  disabled?: boolean;
  badge?: string;
};

const EMPTY_SELECT_VALUE = "__empty__";
const DPI_PRESETS = [400, 800, 1000, 1600, 2400, 4000, 6000, 8000];
const DEFAULT_DEVICE_SETTINGS: NonNullable<AppConfig["deviceDefaults"]> = {
  dpi: 1000,
  invertHorizontalScroll: false,
  invertVerticalScroll: false,
  macosThumbWheelSimulateTrackpad: false,
  macosThumbWheelTrackpadHoldTimeoutMs: 500,
  gestureThreshold: 50,
  gestureDeadzone: 40,
  gestureTimeoutMs: 3000,
  gestureCooldownMs: 500,
  manualLayoutOverride: null,
};
const DEFAULT_DEBUG_LOG_GROUPS: NonNullable<
  AppConfig["settings"]["debugLogGroups"]
> = {
  runtime: true,
  hookRouting: false,
  gestures: false,
  thumbWheel: false,
  hid: false,
};

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

function isMxMasterFamilyDevice(device: DeviceInfo | null | undefined) {
  return device?.modelKey.startsWith("mx_master") ?? false;
}

function supportLevelLabel(level: DeviceSupportLevel) {
  switch (level) {
    case "full":
      return "Verified";
    case "partial":
      return "Partial";
    case "experimental":
      return "Experimental";
  }

  return "Experimental";
}

function supportLevelTone(level: DeviceSupportLevel): "success" | "warning" | "neutral" {
  switch (level) {
    case "full":
      return "success";
    case "partial":
      return "warning";
    case "experimental":
      return "neutral";
  }

  return "neutral";
}

function runtimeHealthIssues(runtimeHealth: RuntimeHealth | null | undefined) {
  const entries = [
    ["persistence", "Config save", runtimeHealth?.persistence],
    ["hid", "Device detection", runtimeHealth?.hid],
    ["hook", "Input hook", runtimeHealth?.hook],
    ["focus", "App focus", runtimeHealth?.focus],
    ["discovery", "App discovery", runtimeHealth?.discovery],
  ] as const;

  return entries.flatMap(([key, label, health]) => {
    const state = health?.state;
    if (state == null || state === "ready") {
      return [];
    }

    return [
      {
        key,
        label,
        state,
        message:
          health?.message ??
          (state === "error"
            ? `${label} is unavailable.`
            : `${label} is using stale data.`),
      },
    ];
  });
}

function runtimeHealthTone(state: BackendHealthState): "warning" | "danger" {
  return state === "error" ? "danger" : "warning";
}

function batteryStatusLabel(device: DeviceInfo | null | undefined) {
  if (device?.battery?.label) {
    return device.battery.label;
  }
  if (device?.batteryLevel != null) {
    return `${device.batteryLevel}%`;
  }
  return null;
}

function batteryStatusPillValue(device: DeviceInfo | null | undefined) {
  const label = batteryStatusLabel(device);
  if (label == null) {
    return device?.connected ? "Connected" : device ? "Added" : "No device";
  }
  return device?.battery?.kind === "percentage" || device?.batteryLevel != null
    ? `${label} battery`
    : label;
}

function formatHexByteList(bytes: number[] | null | undefined) {
  if (!bytes || bytes.length === 0) {
    return "None";
  }
  return bytes
    .map((value) => `0x${value.toString(16).toUpperCase().padStart(2, "0")}`)
    .join(" ");
}

function supportsControl(
  supportedControls: LogicalControl[],
  control: LogicalControl,
) {
  return supportedControls.includes(control);
}

function supportsAnyControl(
  supportedControls: LogicalControl[],
  controls: LogicalControl[],
) {
  return controls.some((control) => supportsControl(supportedControls, control));
}

function primaryControlsForDevice(supportedControls: LogicalControl[]) {
  const visible = new Set<LogicalControl>();
  const ordered: LogicalControl[] = [];

  for (const control of PRIMARY_CONTROL_ORDER) {
    if (control === "gesture_press") {
      if (supportsAnyControl(supportedControls, GESTURE_CONTROLS)) {
        visible.add(control);
        ordered.push(control);
      }
      continue;
    }

    if (control === "hscroll_left") {
      if (supportsAnyControl(supportedControls, HSCROLL_CONTROLS)) {
        visible.add(control);
        ordered.push(control);
      }
      continue;
    }

    if (supportsControl(supportedControls, control)) {
      visible.add(control);
      ordered.push(control);
    }
  }

  for (const control of supportedControls) {
    if (!visible.has(control)) {
      ordered.push(control);
    }
  }

  return ordered;
}

function notesForSupport(
  support: { notes: string[] },
  layout?: DeviceLayout | null,
) {
  const notes = [...support.notes];
  const layoutNote = layout?.note?.trim();
  if (layoutNote && !notes.includes(layoutNote)) {
    notes.push(layoutNote);
  }
  return notes;
}

function supportedHotspotsForLayout(
  layout: DeviceLayout,
  supportedControls: LogicalControl[],
) {
  return layout.hotspots.filter((hotspot) =>
    supportedControls.includes(hotspot.control),
  );
}

function controlSummaryLabel(supportedControls: LogicalControl[]) {
  const primaryControls = primaryControlsForDevice(supportedControls);
  if (primaryControls.length === 0) {
    return "No remappable controls exposed yet";
  }

  return primaryControls.map((control) => editorTitleFor(control)).join(", ");
}

function normalizeDeviceSettings(
  settings:
    | AppConfig["deviceDefaults"]
    | NonNullable<AppConfig["managedDevices"]>[number]["settings"]
    | null
    | undefined,
): NonNullable<AppConfig["deviceDefaults"]> {
  return {
    dpi: settings?.dpi ?? DEFAULT_DEVICE_SETTINGS.dpi,
    invertHorizontalScroll:
      settings?.invertHorizontalScroll ??
      DEFAULT_DEVICE_SETTINGS.invertHorizontalScroll,
    invertVerticalScroll:
      settings?.invertVerticalScroll ??
      DEFAULT_DEVICE_SETTINGS.invertVerticalScroll,
    macosThumbWheelSimulateTrackpad:
      settings?.macosThumbWheelSimulateTrackpad ??
      DEFAULT_DEVICE_SETTINGS.macosThumbWheelSimulateTrackpad,
    macosThumbWheelTrackpadHoldTimeoutMs:
      settings?.macosThumbWheelTrackpadHoldTimeoutMs ??
      DEFAULT_DEVICE_SETTINGS.macosThumbWheelTrackpadHoldTimeoutMs,
    gestureThreshold:
      settings?.gestureThreshold ?? DEFAULT_DEVICE_SETTINGS.gestureThreshold,
    gestureDeadzone:
      settings?.gestureDeadzone ?? DEFAULT_DEVICE_SETTINGS.gestureDeadzone,
    gestureTimeoutMs:
      settings?.gestureTimeoutMs ?? DEFAULT_DEVICE_SETTINGS.gestureTimeoutMs,
    gestureCooldownMs:
      settings?.gestureCooldownMs ?? DEFAULT_DEVICE_SETTINGS.gestureCooldownMs,
    manualLayoutOverride:
      settings?.manualLayoutOverride ??
      DEFAULT_DEVICE_SETTINGS.manualLayoutOverride,
  };
}

function normalizeOptionalText(value: string | null | undefined) {
  const trimmed = value?.trim();
  return trimmed ? trimmed : null;
}

function normalizeAppMatcherValue(kind: AppMatcherKind, value: string) {
  const trimmed = value.trim();
  if (!trimmed) {
    return "";
  }

  switch (kind) {
    case "executable": {
      const segments = trimmed.split("\\").join("/").split("/");
      return (segments[segments.length - 1] ?? trimmed).toLowerCase();
    }
    case "executable_path":
      return trimmed.split("\\").join("/").toLowerCase();
    case "bundle_id":
    case "package_family_name":
      return trimmed.toLowerCase();
  }
}

function matchersOverlap(left: AppMatcher[], right: AppMatcher[]) {
  return left.some((leftMatcher) =>
    right.some(
      (rightMatcher) =>
        leftMatcher.kind === rightMatcher.kind &&
        normalizeAppMatcherValue(leftMatcher.kind, leftMatcher.value) ===
          normalizeAppMatcherValue(rightMatcher.kind, rightMatcher.value),
    ),
  );
}

function profileMatchesDiscoveredApp(profile: Profile, app: DiscoveredApp) {
  return (
    profile.appMatchers.length > 0 &&
    matchersOverlap(profile.appMatchers, app.matchers)
  );
}

function resolveKnownApp(
  profile: Profile,
  knownApps: BootstrapPayload["knownApps"],
) {
  const executableMatcher = profile.appMatchers.find(
    (matcher) => matcher.kind === "executable",
  );
  if (!executableMatcher) {
    return null;
  }

  return (
    knownApps.find(
      (app) =>
        normalizeAppMatcherValue("executable", app.executable) ===
        normalizeAppMatcherValue("executable", executableMatcher.value),
    ) ?? null
  );
}

function resolveProfileApp(
  profile: Profile,
  discoveredApps: DiscoveredApp[],
  knownApps: BootstrapPayload["knownApps"],
) {
  return (
    discoveredApps.find((app) => profileMatchesDiscoveredApp(profile, app)) ??
    resolveKnownApp(profile, knownApps)
  );
}

function formatAppMatcher(matcher: AppMatcher) {
  switch (matcher.kind) {
    case "executable":
      return matcher.value;
    case "executable_path":
      return `Path: ${matcher.value}`;
    case "bundle_id":
      return `Bundle ID: ${matcher.value}`;
    case "package_family_name":
      return `Package: ${matcher.value}`;
  }
}

function parseDraftAppMatcher(value: string): AppMatcher {
  const trimmed = value.trim();
  const separators: Array<[AppMatcherKind, string]> = [
    ["bundle_id", "bundle id:"],
    ["bundle_id", "bundle:"],
    ["package_family_name", "package family:"],
    ["package_family_name", "package:"],
    ["executable_path", "path:"],
    ["executable", "exe:"],
  ];

  for (const [kind, prefix] of separators) {
    if (trimmed.toLowerCase().startsWith(prefix)) {
      return {
        kind,
        value: trimmed.slice(prefix.length).trim(),
      };
    }
  }

  return { kind: "executable", value: trimmed };
}

function formatDiscoverySource(
  source: DiscoveredApp["sourceKinds"][number] | undefined,
) {
  switch (source) {
    case "application_bundle":
      return "Applications";
    case "desktop_entry":
      return "Desktop";
    case "start_menu_shortcut":
      return "Start Menu";
    case "registry":
      return "Registry";
    case "package":
      return "Package";
    case "running_process":
      return "Running";
    case "catalog":
      return "Catalog";
    default:
      return "Installed";
  }
}

function AppIcon(props: {
  label: string;
  iconAsset?: string | null;
  sourcePath?: string | null;
  className: string;
  fallbackClassName: string;
  defer?: boolean;
}) {
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  const [shouldLoad, setShouldLoad] = useState(
    !props.defer || typeof IntersectionObserver === "undefined",
  );

  useEffect(() => {
    if (shouldLoad || !props.defer) {
      return;
    }

    if (typeof IntersectionObserver === "undefined") {
      setShouldLoad(true);
      return;
    }

    const node = sentinelRef.current;
    if (!node) {
      return;
    }

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          setShouldLoad(true);
          observer.disconnect();
        }
      },
      { rootMargin: "160px" },
    );
    observer.observe(node);

    return () => observer.disconnect();
  }, [props.defer, shouldLoad]);

  const iconQuery = useQuery({
    queryKey: ["app-icon", props.sourcePath],
    queryFn: () => appIconLoad(props.sourcePath ?? ""),
    enabled: !!props.sourcePath && shouldLoad,
    staleTime: Number.POSITIVE_INFINITY,
    gcTime: Number.POSITIVE_INFINITY,
  });

  const src = iconQuery.data ?? props.iconAsset ?? null;
  if (src) {
    return (
      <img
        alt={props.label}
        className={props.className}
        loading={props.defer ? "lazy" : undefined}
        src={src}
      />
    );
  }

  return (
    <div
      className={props.fallbackClassName}
      ref={props.defer ? sentinelRef : undefined}
    >
      {props.label.charAt(0).toUpperCase()}
    </div>
  );
}

function findManagedDevice(
  config: AppConfig,
  deviceKey: string | null | undefined,
) {
  if (!deviceKey) {
    return null;
  }

  return (
    config.managedDevices?.find((device) => device.id === deviceKey) ?? null
  );
}

function selectedDeviceSettings(
  config: AppConfig,
  deviceKey: string | null | undefined,
) {
  return normalizeDeviceSettings(
    findManagedDevice(config, deviceKey)?.settings ?? config.deviceDefaults,
  );
}

function resolveIsDarkAppearance(
  appearanceMode: AppConfig["settings"]["appearanceMode"],
  mediaQuery: MediaQueryList | null,
) {
  if (appearanceMode === "dark") {
    return true;
  }

  if (appearanceMode === "light") {
    return false;
  }

  return mediaQuery?.matches ?? false;
}

function App() {
  const queryClient = useQueryClient();
  useRuntimeEvents();

  const shellMode = useUiStore((state) => state.shellMode);
  const setShellMode = useUiStore((state) => state.setShellMode);
  const activeSection = useUiStore((state) => state.activeSection);
  const setActiveSection = useUiStore((state) => state.setActiveSection);
  const selectedProfileId = useUiStore((state) => state.selectedProfileId);
  const setSelectedProfileId = useUiStore(
    (state) => state.setSelectedProfileId,
  );
  const importDraft = useUiStore((state) => state.importDraft);
  const setImportDraft = useUiStore((state) => state.setImportDraft);

  const [newProfileLabel, setNewProfileLabel] = useState("");
  const [newProfileApp, setNewProfileApp] = useState("");
  const [importWarnings, setImportWarnings] = useState<string[]>([]);
  const [importSourcePath, setImportSourcePath] = useState("");
  const [isAddDeviceOpen, setAddDeviceOpen] = useState(false);
  const [isAppSidebarOpen, setAppSidebarOpen] = useState(false);
  const [isAppSettingsOpen, setAppSettingsOpen] = useState(false);
  const [appSearchQuery, setAppSearchQuery] = useState("");
  const lastActiveProfileIdRef = useRef<string | null>(null);

  const bootstrapQuery = useQuery({
    queryKey: ["bootstrap"],
    queryFn: bootstrapLoad,
  });

  const setBootstrapQueryData = (payload: BootstrapPayload) => {
    queryClient.setQueryData(["bootstrap"], payload);
  };

  const invalidateBootstrap = () =>
    queryClient.invalidateQueries({ queryKey: ["bootstrap"] });

  const saveConfigMutation = useMutation({
    mutationFn: configSave,
    onSuccess: setBootstrapQueryData,
  });
  const appDiscoveryRefreshMutation = useMutation({
    mutationFn: appDiscoveryRefresh,
    onSuccess: setBootstrapQueryData,
  });
  const resetDeviceToFactoryMutation = useMutation({
    mutationFn: devicesResetToFactory,
    onSuccess: setBootstrapQueryData,
  });
  const addDeviceMutation = useMutation({
    mutationFn: devicesAdd,
    onSuccess: setBootstrapQueryData,
  });
  const removeDeviceMutation = useMutation({
    mutationFn: devicesRemove,
    onSuccess: setBootstrapQueryData,
  });
  const selectDeviceMutation = useMutation({
    mutationFn: devicesSelect,
    onSuccess: () => void invalidateBootstrap(),
  });
  const importMutation = useMutation({
    mutationFn: importLegacyConfig,
    onSuccess: (report) => {
      setImportWarnings(report.warnings);
      void invalidateBootstrap();
    },
  });
  useEffect(() => {
    if (!importDraft) {
      setImportDraft(sampleLegacyConfig);
    }
  }, [importDraft, setImportDraft]);

  useEffect(() => {
    if (!bootstrapQuery.data) {
      return;
    }

    const root = document.documentElement;
    const appearanceMode = bootstrapQuery.data.config.settings.appearanceMode;
    const mediaQuery =
      typeof window.matchMedia === "function"
        ? window.matchMedia("(prefers-color-scheme: dark)")
        : null;

    const syncAppearance = () => {
      root.classList.toggle(
        "dark",
        resolveIsDarkAppearance(appearanceMode, mediaQuery),
      );
    };

    syncAppearance();

    if (appearanceMode !== "system" || mediaQuery == null) {
      return;
    }

    const handleChange = () => {
      syncAppearance();
    };

    mediaQuery.addEventListener("change", handleChange);
    return () => {
      mediaQuery.removeEventListener("change", handleChange);
    };
  }, [bootstrapQuery.data]);

  useEffect(() => {
    if (!bootstrapQuery.data) {
      return;
    }

    const profileIds = new Set(
      bootstrapQuery.data.config.profiles.map((profile) => profile.id),
    );
    const activeProfileId =
      bootstrapQuery.data.engineSnapshot.engineStatus.activeProfileId;
    const activeProfileChanged =
      lastActiveProfileIdRef.current != null &&
      lastActiveProfileIdRef.current !== activeProfileId;

    if (
      !selectedProfileId ||
      !profileIds.has(selectedProfileId) ||
      activeProfileChanged
    ) {
      setSelectedProfileId(activeProfileId);
    }

    lastActiveProfileIdRef.current = activeProfileId;
  }, [
    bootstrapQuery.data,
    selectedProfileId,
    setSelectedProfileId,
  ]);

  useEffect(() => {
    if (shellMode !== "detail" || !bootstrapQuery.data) {
      return;
    }

    if (bootstrapQuery.data.engineSnapshot.devices.length === 0) {
      setShellMode("dashboard");
    }
  }, [bootstrapQuery.data, setShellMode, shellMode]);

  useEffect(() => {
    if (shellMode === "dashboard") {
      setAppSidebarOpen(false);
    }
  }, [shellMode]);

  useEffect(() => {
    if (!bootstrapQuery.data?.config.settings.debugMode) {
      return;
    }

    const { config, engineSnapshot } = bootstrapQuery.data;
    const activeDevice = engineSnapshot.activeDevice;
    const deviceSettings = selectedDeviceSettings(config, activeDevice?.key);
    const liveDevice =
      activeDevice == null
        ? null
        : (engineSnapshot.detectedDevices.find((device) =>
            samePhysicalDevice(device, activeDevice),
          ) ?? null);

    console.debug("[mouser:dpi]", {
      configuredDpi: deviceSettings.dpi,
      configuredMatchesLive: liveDevice
        ? deviceSettings.dpi === liveDevice.currentDpi
        : null,
      activeDevice: activeDevice
        ? {
            key: activeDevice.key,
            displayName: activeDevice.displayName,
            currentDpi: activeDevice.currentDpi,
            dpiMin: activeDevice.dpiMin,
            dpiMax: activeDevice.dpiMax,
          }
        : null,
      liveDevice: liveDevice
        ? {
            key: liveDevice.key,
            displayName: liveDevice.displayName,
            currentDpi: liveDevice.currentDpi,
            dpiMin: liveDevice.dpiMin,
            dpiMax: liveDevice.dpiMax,
            transport: liveDevice.transport,
            source: liveDevice.source,
          }
        : null,
      detectedDevices: engineSnapshot.detectedDevices.map((device) => ({
        key: device.key,
        modelKey: device.modelKey,
        currentDpi: device.currentDpi,
        dpiMin: device.dpiMin,
        dpiMax: device.dpiMax,
      })),
    });
  }, [bootstrapQuery.data]);

  const bootstrap = bootstrapQuery.data;
  const isMutating =
    saveConfigMutation.isPending ||
    resetDeviceToFactoryMutation.isPending ||
    addDeviceMutation.isPending ||
    removeDeviceMutation.isPending ||
    selectDeviceMutation.isPending ||
    importMutation.isPending;

  if (bootstrapQuery.isLoading) {
    return (
      <main className="flex min-h-screen items-center justify-center bg-app-bg px-8 text-foreground">
        <Card className="px-6 py-4">Loading Mouser...</Card>
      </main>
    );
  }

  if (bootstrapQuery.isError || !bootstrap) {
    return (
      <main className="flex min-h-screen items-center justify-center bg-app-bg px-8 text-foreground">
        <Card className="max-w-xl p-6">
          <p className="text-sm font-semibold">Failed to load Mouser.</p>
          <pre className="mt-4 overflow-auto rounded-3xl border border-border bg-white p-4 text-xs text-muted-foreground">
            {String(bootstrapQuery.error)}
          </pre>
        </Card>
      </main>
    );
  }

  const {
    config,
    availableActions,
    knownApps,
    appDiscovery,
    engineSnapshot,
    layouts,
    platformCapabilities,
  } = bootstrap;
  const discoveredApps = appDiscovery.browseApps;
  const selectedProfile =
    config.profiles.find((profile) => profile.id === selectedProfileId) ??
    config.profiles.find(
      (profile) => profile.id === engineSnapshot.engineStatus.activeProfileId,
    ) ??
    config.profiles[0];
  const activeDevice = engineSnapshot.activeDevice;
  const activeLayout = resolveActiveLayout(activeDevice, config, layouts);
  const actionLookup = new Map(
    availableActions.map((action) => [action.id, action]),
  );
  const groupedActions = groupActions(availableActions);

  const mutateConfig = (editConfig: (nextConfig: AppConfig) => void) => {
    const nextConfig = cloneConfig(config);
    editConfig(nextConfig);
    saveConfigMutation.mutate(nextConfig);
  };

  const updateSelectedProfile = (mutateProfile: (profile: Profile) => void) => {
    mutateConfig((nextConfig) => {
      const existingProfile = nextConfig.profiles.find(
        (profile) => profile.id === selectedProfile.id,
      );
      if (!existingProfile) {
        return;
      }
      mutateProfile(existingProfile);
    });
  };

  const saveAppSettings = (
    mutateSettings: (nextSettings: AppConfig["settings"]) => void,
  ) => {
    mutateConfig((nextConfig) => {
      mutateSettings(nextConfig.settings);
    });
  };

  const saveDeviceDefaults = (
    mutateSettings: (
      nextSettings: NonNullable<AppConfig["deviceDefaults"]>,
    ) => void,
  ) => {
    mutateConfig((nextConfig) => {
      nextConfig.deviceDefaults = normalizeDeviceSettings(
        nextConfig.deviceDefaults,
      );
      mutateSettings(nextConfig.deviceDefaults);
    });
  };

  const saveManagedDevice = (
    deviceKey: string,
    mutateDevice: (
      nextDevice: NonNullable<AppConfig["managedDevices"]>[number],
    ) => void,
  ) => {
    mutateConfig((nextConfig) => {
      const managedDevices = nextConfig.managedDevices ?? [];
      nextConfig.managedDevices = managedDevices;
      const managedDevice = managedDevices.find((device) => device.id === deviceKey);
      if (!managedDevice) {
        return;
      }
      mutateDevice(managedDevice);
    });
  };

  const openDeviceDetail = (
    deviceKey: string,
    section: SectionName = "buttons",
  ) => {
    selectDeviceMutation.mutate(deviceKey, {
      onSuccess: () => {
        setShellMode("detail");
        setActiveSection(section);
      },
    });
  };

  const createProfileFromDraft = () => {
    const labelSource =
      newProfileLabel.trim() ||
      knownApps.find((app) => app.executable === newProfileApp.trim())?.label ||
      newProfileApp.trim();
    const executable = newProfileApp.trim();

    if (!labelSource) {
      return;
    }

    const id = makeProfileId(labelSource, config);
    mutateConfig((nextConfig) => {
      nextConfig.profiles.push({
        id,
        label: labelSource,
        appMatchers: executable
          ? [{ kind: "executable", value: executable }]
          : [],
        bindings: selectedProfile.bindings.map((binding) => ({ ...binding })),
      });
    });
    setNewProfileLabel("");
    setNewProfileApp("");
    setSelectedProfileId(id);
    setActiveSection("profiles");
  };

  const openAppDiscovery = () => {
    setAppSidebarOpen(true);
    setAppSearchQuery("");
    if (
      appDiscovery.browseApps.length === 0 &&
      !appDiscoveryRefreshMutation.isPending
    ) {
      appDiscoveryRefreshMutation.mutate();
    }
  };

  const selectOrCreateProfileForApp = (app: DiscoveredApp) => {
    const existingProfile = config.profiles.find((profile) =>
      profileMatchesDiscoveredApp(profile, app),
    );
    if (existingProfile) {
      setSelectedProfileId(existingProfile.id);
      setActiveSection("profiles");
      setAppSidebarOpen(false);
      return;
    }

    const id = makeProfileId(app.label, config);
    mutateConfig((nextConfig) => {
      nextConfig.profiles.push({
        id,
        label: app.label,
        appMatchers: app.matchers.map((matcher) => ({ ...matcher })),
        bindings: selectedProfile.bindings.map((binding) => ({ ...binding })),
      });
    });
    setSelectedProfileId(id);
    setActiveSection("profiles");
    setAppSidebarOpen(false);
  };

  const shellTitle =
    activeDevice?.displayName ?? SECTION_META[activeSection].label;
  const activeManagedDevice = findManagedDevice(config, activeDevice?.key);
  const batteryLabel = batteryStatusLabel(activeDevice) ?? "N/A";
  const connectionStatus = activeDevice?.connected
    ? { tone: "success" as const, value: "Connected" }
    : activeDevice
      ? { tone: "neutral" as const, value: "Added" }
      : { tone: "neutral" as const, value: "No device" };
  const isDashboard = shellMode === "dashboard";
  const runtimeHealth = engineSnapshot.engineStatus.runtimeHealth;

  return (
    <main className="min-h-screen bg-app-bg text-foreground antialiased">
      {isDashboard ? (
        <DashboardShell
          activeDevice={activeDevice}
          engineSnapshot={engineSnapshot}
          isAddDeviceOpen={isAddDeviceOpen}
          isMutating={isMutating}
          onAddDevice={(modelKey) => addDeviceMutation.mutate(modelKey)}
          onCloseAddDevice={() => setAddDeviceOpen(false)}
          onOpenAddDevice={() => setAddDeviceOpen(true)}
          onOpenAppSettings={() => setAppSettingsOpen(true)}
          onOpenDevice={(deviceKey, section) =>
            openDeviceDetail(deviceKey, section)
          }
          onRemoveDevice={removeDeviceMutation.mutate}
          runtimeHealth={runtimeHealth}
          supportedDevices={bootstrap.supportedDevices}
        />
      ) : (
        <div className="relative min-h-screen bg-app-bg">
          <header className="fixed inset-x-0 top-0 z-30 flex items-center justify-between  bg-surface px-8 py-4 backdrop-blur-xl">
            <div className="flex items-center gap-3">
              <button
                className="flex h-9 w-9 items-center justify-center rounded-xl text-foreground transition hover:bg-accent"
                onClick={() => setShellMode("dashboard")}
                type="button"
              >
                <CaretLeft size={20} weight="bold" />
              </button>
              <h1 className="text-[22px] font-bold tracking-[-0.04em] text-foreground">
                {shellTitle}
              </h1>
            </div>

            <div className="flex items-center gap-2">
              {isMutating && <StatusPill tone="accent" value="Applying" />}
              {activeManagedDevice ? (
                <Button
                  disabled={resetDeviceToFactoryMutation.isPending}
                  onClick={() =>
                    resetDeviceToFactoryMutation.mutate(activeManagedDevice.id)
                  }
                  size="sm"
                  title="Restore this mouse to the imported Logitech default profile and baseline Mouser tuning."
                  variant="outline"
                >
                  <ArrowCounterClockwise className="mr-2 h-4 w-4" />
                  {resetDeviceToFactoryMutation.isPending
                    ? "Resetting..."
                    : "Factory reset"}
                </Button>
              ) : null}
              {config.profiles.slice(0, 5).map((profile) => {
                const app = resolveProfileApp(
                  profile,
                  discoveredApps,
                  knownApps,
                );
                return (
                  <button
                    className={cn(
                      "flex h-9 w-9 items-center justify-center rounded-xl border-2 transition",
                      profile.id === selectedProfile.id
                        ? "border-[#10b981] bg-[#10b981]/10"
                        : "border-transparent bg-card-muted hover:bg-card",
                    )}
                    key={profile.id}
                    onClick={() => {
                      setSelectedProfileId(profile.id);
                      setActiveSection("profiles");
                    }}
                    title={profile.label}
                    type="button"
                  >
                    <AppIcon
                      className="h-6 w-6 rounded-lg object-cover"
                      fallbackClassName="flex h-6 w-6 items-center justify-center text-xs font-bold text-foreground"
                      iconAsset={app?.iconAsset}
                      label={profile.label}
                      sourcePath={
                        app && "sourcePath" in app ? app.sourcePath : null
                      }
                    />
                  </button>
                );
              })}
              <button
                className="flex h-9 w-9 items-center justify-center rounded-xl text-muted-foreground transition hover:bg-black/5 hover:text-foreground"
                onClick={openAppDiscovery}
                type="button"
              >
                <Plus size={18} weight="bold" />
              </button>
            </div>
          </header>

          <nav className="fixed left-7 top-1/2 z-20 hidden -translate-y-1/2 lg:block">
            <div className="flex flex-col gap-0.5 rounded-[20px] bg-surface p-2 shadow-[0_8px_40px_rgba(0,0,0,0.16)] ring-1 ring-border-soft backdrop-blur-xl">
              {SECTION_ORDER.map((section) => (
                <SectionNavButton
                  active={activeSection === section}
                  icon={SECTION_META[section].icon}
                  key={section}
                  label={SECTION_META[section].label}
                  onClick={() => setActiveSection(section)}
                />
              ))}
              <div className="mx-3 my-1 h-px bg-border-soft" />
              <button
                className="flex items-center gap-3 rounded-2xl px-4 py-2.5 text-sm font-medium text-muted-foreground transition hover:bg-accent hover:text-foreground"
                onClick={() => setAppSettingsOpen(true)}
                type="button"
              >
                <GearSix className="h-[18px] w-[18px]" />
                <span>Settings</span>
              </button>
            </div>
          </nav>

          <div className="min-h-screen overflow-y-auto px-6 pb-8 pt-[72px] lg:pl-[220px]">
            <RuntimeHealthNotice
              className="mb-6"
              runtimeHealth={runtimeHealth}
            />
            {activeSection === "buttons" && (
              <ButtonsView
                actionLookup={actionLookup}
                activeDevice={activeDevice}
                activeLayout={activeLayout}
                config={config}
                groupedActions={groupedActions}
                mappingEngineReady={platformCapabilities.mappingEngineReady}
                platformCapabilities={platformCapabilities}
                profile={selectedProfile}
                updateSelectedProfile={updateSelectedProfile}
              />
            )}

            {activeSection === "devices" && (
              <DeviceDetailView
                activeDevice={activeDevice}
                activeManagedDevice={activeManagedDevice}
                activeLayout={activeLayout}
                config={config}
                layoutChoices={bootstrap.manualLayoutChoices}
                platformCapabilities={platformCapabilities}
                profiles={config.profiles}
                setSelectedProfileId={setSelectedProfileId}
                updateDeviceNickname={(deviceKey, nickname) =>
                  saveManagedDevice(deviceKey, (device) => {
                    device.nickname = nickname;
                  })
                }
                updateDeviceProfile={(deviceKey, profileId) =>
                  saveManagedDevice(deviceKey, (device) => {
                    device.profileId = profileId;
                  })
                }
                updateDeviceSettings={(deviceKey, settings) =>
                  saveManagedDevice(deviceKey, (device) => {
                    device.settings = settings;
                  })
                }
              />
            )}

            {activeSection === "profiles" && (
              <ProfilesView
                createProfileFromDraft={createProfileFromDraft}
                deleteProfile={(profileId) =>
                  mutateConfig((nextConfig) => {
                    nextConfig.profiles = nextConfig.profiles.filter(
                      (profile) => profile.id !== profileId,
                    );
                  })
                }
                discoveredApps={discoveredApps}
                knownApps={knownApps}
                newProfileApp={newProfileApp}
                newProfileLabel={newProfileLabel}
                profile={selectedProfile}
                profiles={config.profiles}
                setNewProfileApp={setNewProfileApp}
                setNewProfileLabel={setNewProfileLabel}
                setSelectedProfileId={setSelectedProfileId}
                updateSelectedProfile={updateSelectedProfile}
              />
            )}

            {activeSection === "debug" && (
              <DebugView
                activeDevice={activeDevice}
                config={config}
                importDraft={importDraft}
                importSourcePath={importSourcePath}
                importWarnings={importWarnings}
                onImport={() =>
                  importMutation.mutate(
                    buildImportRequest(importSourcePath, importDraft),
                  )
                }
                platformCapabilities={platformCapabilities}
                saveAppSettings={saveAppSettings}
                setImportDraft={setImportDraft}
                setImportSourcePath={setImportSourcePath}
              />
            )}
          </div>

          {activeDevice && (
            <div className="fixed bottom-6 left-8 z-20 flex items-center gap-3">
              <span className="text-sm font-semibold text-foreground">
                {batteryLabel}
              </span>
              <StatusPill
                tone={connectionStatus.tone}
                value={connectionStatus.value}
              />
            </div>
          )}

          <AnimatePresence initial={false}>
            {isAppSidebarOpen ? (
              <motion.aside
                animate={{ opacity: 1, x: 0 }}
                className="fixed right-0 top-0 z-40 flex h-full min-h-0 w-full max-w-[420px] flex-col overflow-hidden border-l border-border-soft bg-surface text-foreground shadow-[-24px_0_64px_rgba(15,23,42,0.12)] backdrop-blur-xl"
                exit={{ opacity: 0, x: 32 }}
                initial={{ opacity: 0, x: 32 }}
                transition={{ duration: 0.22, ease: [0.22, 1, 0.36, 1] }}
              >
                <AppDiscoverySheet
                  appDiscovery={appDiscovery}
                  isRefreshing={appDiscoveryRefreshMutation.isPending}
                  onClose={() => setAppSidebarOpen(false)}
                  onRefresh={() => appDiscoveryRefreshMutation.mutate()}
                  onSelectApp={selectOrCreateProfileForApp}
                  searchQuery={appSearchQuery}
                  setSearchQuery={setAppSearchQuery}
                />
              </motion.aside>
            ) : null}
          </AnimatePresence>
        </div>
      )}
      <AppSettingsDialog
        config={config}
        layoutChoices={bootstrap.manualLayoutChoices}
        onClose={() => setAppSettingsOpen(false)}
        open={isAppSettingsOpen}
        platformCapabilities={platformCapabilities}
        saveAppSettings={saveAppSettings}
        saveDeviceDefaults={saveDeviceDefaults}
      />
    </main>
  );
}

function ButtonsView(props: {
  profile: Profile;
  config: AppConfig;
  activeDevice: DeviceInfo | null;
  activeLayout: DeviceLayout;
  actionLookup: Map<string, ActionDefinition>;
  groupedActions: Array<[string, ActionDefinition[]]>;
  platformCapabilities: BootstrapPayload["platformCapabilities"];
  mappingEngineReady: boolean;
  updateSelectedProfile: (mutateProfile: (profile: Profile) => void) => void;
}) {
  const [selectedControl, setSelectedControl] = useState<LogicalControl | null>(
    null,
  );
  const activeDevice = props.activeDevice;
  const visibleHotspots = activeDevice
    ? supportedHotspotsForLayout(
        props.activeLayout,
        activeDevice.supportedControls,
      )
    : [];
  const fallbackControls = activeDevice
    ? primaryControlsForDevice(activeDevice.supportedControls)
    : [];
  const supportNotes = activeDevice
    ? notesForSupport(activeDevice.support, props.activeLayout)
    : [];
  const hasInteractiveMatrix =
    props.activeLayout.interactive && visibleHotspots.length > 0;
  const visibleControls = hasInteractiveMatrix
    ? visibleHotspots.map((hotspot) => hotspot.control)
    : fallbackControls;

  useEffect(() => {
    if (!activeDevice) {
      setSelectedControl(null);
      return;
    }

    const visibleControlSet = new Set(visibleControls);
    setSelectedControl((current) =>
      current && visibleControlSet.has(current) ? current : null,
    );
  }, [activeDevice, visibleControls]);

  const setBinding = (control: LogicalControl, actionId: string) => {
    props.updateSelectedProfile((nextProfile) => {
      upsertBinding(nextProfile, control, actionId);
    });
  };

  return (
    <div className="relative min-h-[calc(100vh-120px)]">
      {activeDevice ? (
        <>
          {hasInteractiveMatrix ? (
            <ButtonsWorkbench
              actionLookup={props.actionLookup}
              activeDevice={activeDevice}
              controlSpecs={activeDevice.controls ?? []}
              hotspots={visibleHotspots}
              layout={props.activeLayout}
              notes={supportNotes}
              profile={props.profile}
              selectedControl={selectedControl}
              onSelectControl={setSelectedControl}
            />
          ) : fallbackControls.length > 0 ? (
            <ButtonsMatrixList
              actionLookup={props.actionLookup}
              controls={fallbackControls}
              controlSpecs={activeDevice.controls ?? []}
              deviceName={activeDevice.displayName}
              notes={supportNotes}
              profile={props.profile}
              selectedControl={selectedControl}
              onSelectControl={setSelectedControl}
            />
          ) : (
            <EmptyStage
              body={`${activeDevice.displayName} is recognized, but Mouser does not expose a validated remapping matrix for it yet. Add the supported controls in the catalog before enabling button editing for this model.`}
              title="No remappable controls yet"
            />
          )}

          <AnimatePresence initial={false}>
            {selectedControl && (
              <motion.aside
                animate={{ opacity: 1, x: 0 }}
                className="fixed right-0 top-0 z-40 flex h-full w-[400px] flex-col border-l border-border-soft bg-surface px-8 pb-8 pt-20 backdrop-blur-xl"
                exit={{ opacity: 0, x: 24 }}
                initial={{ opacity: 0, x: 24 }}
                key={selectedControl}
                transition={{ duration: 0.22, ease: [0.22, 1, 0.36, 1] }}
              >
                <ButtonsControlSheet
                  actionLookup={props.actionLookup}
                  control={selectedControl}
                  controlSpecs={activeDevice.controls ?? []}
                  groupedActions={props.groupedActions}
                  mappingEngineReady={props.mappingEngineReady}
                  platformCapabilities={props.platformCapabilities}
                  profile={props.profile}
                  supportedControls={activeDevice.supportedControls}
                  setBinding={setBinding}
                  onClose={() => setSelectedControl(null)}
                />
              </motion.aside>
            )}
          </AnimatePresence>
        </>
      ) : (
        <EmptyStage
          body="Connect a supported mouse to inspect mapped controls."
          title="No device detected"
        />
      )}
    </div>
  );
}

function DashboardShell(props: {
  activeDevice: DeviceInfo | null;
  engineSnapshot: BootstrapPayload["engineSnapshot"];
  isAddDeviceOpen: boolean;
  isMutating: boolean;
  onAddDevice: (modelKey: string) => void;
  onCloseAddDevice: () => void;
  onOpenAddDevice: () => void;
  onOpenAppSettings: () => void;
  onOpenDevice: (deviceKey: string, section: SectionName) => void;
  onRemoveDevice: (deviceKey: string) => void;
  runtimeHealth?: RuntimeHealth;
  supportedDevices: KnownDeviceSpec[];
}) {
  const detectedModelKeys = new Set(
    props.engineSnapshot.detectedDevices.map((device) => device.modelKey),
  );
  const unmanagedDetectedDevices = props.engineSnapshot.detectedDevices.filter(
    (device) =>
      !props.engineSnapshot.devices.some((managed) =>
        samePhysicalDevice(managed, device),
      ),
  );
  const selectedDeviceKey =
    props.engineSnapshot.activeDeviceKey ?? props.activeDevice?.key ?? null;

  return (
    <div className="flex min-h-screen flex-col bg-app-bg">
      <div className="mx-auto w-full max-w-[1680px] px-6 py-8 sm:px-10 sm:py-10">
        <header className="flex flex-wrap items-start justify-between gap-6">
          <div>
            <p className="text-[38px] font-semibold tracking-[-0.06em] text-foreground sm:text-[48px]">
              {currentGreeting()}
            </p>
          </div>

          <div className="flex flex-wrap items-center gap-3">
            {props.isMutating && <StatusPill tone="accent" value="Applying" />}
            <Button onClick={props.onOpenAddDevice} variant="ghost">
              + Add device
            </Button>
            <Button
              aria-label="App settings"
              onClick={props.onOpenAppSettings}
              size="icon"
              variant="ghost"
            >
              <GearSix className="size-4" />
            </Button>
          </div>
        </header>
      </div>

      <div className="mx-auto flex w-full max-w-[1680px] flex-1 flex-col px-6 pb-10 sm:px-10">
        <RuntimeHealthNotice className="mb-8" runtimeHealth={props.runtimeHealth} />
        <div className="flex flex-1 items-center justify-center">
          {props.engineSnapshot.devices.length > 0 ? (
            <div className="flex flex-wrap items-start justify-center gap-10">
              {props.engineSnapshot.devices.map((device) => (
                <div className="w-[280px] shrink-0" key={device.key}>
                  <button
                    className="group w-full text-center"
                    data-testid={
                      device.key === selectedDeviceKey
                        ? "device-layout-card"
                        : undefined
                    }
                    onClick={() => props.onOpenDevice(device.key, "devices")}
                    type="button"
                  >
                    <DeviceHeroImage
                      alt={device.displayName}
                      connected={device.connected}
                      selected={device.key === selectedDeviceKey}
                      src={device.imageAsset}
                    />

                    <div className="mt-5 flex items-center justify-center gap-2">
                      <StatusPill
                        tone={device.connected ? "success" : "neutral"}
                        value={
                          batteryStatusLabel(device) ??
                          (device.connected ? "Connected" : "Added")
                        }
                      />
                      <StatusPill
                        tone={device.connected ? "success" : "neutral"}
                        value={device.connected ? "Live" : "Waiting"}
                      />
                    </div>

                    <p className="mt-4 text-base font-semibold text-foreground">
                      {device.displayName}
                    </p>
                  </button>

                  <div className="mt-3 flex items-center justify-center gap-2">
                    <Button
                      onClick={() => {
                        props.onOpenDevice(device.key, "buttons");
                      }}
                      size="sm"
                      variant="outline"
                    >
                      Buttons
                    </Button>
                    <Button
                      onClick={() => {
                        props.onOpenDevice(device.key, "devices");
                      }}
                      size="sm"
                      variant="outline"
                    >
                      Tune
                    </Button>
                    <Button
                      onClick={() => props.onRemoveDevice(device.key)}
                      size="sm"
                      variant="ghost"
                    >
                      Remove
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <div className="flex min-h-[420px] flex-col items-center justify-center text-center">
              <h3 className="text-[28px] font-semibold tracking-[-0.05em] text-foreground">
                No devices added
              </h3>
              <p className="mt-3 max-w-md text-sm text-muted-foreground">
                Build your device library first, then select a device when you
                want to customize it.
              </p>
              <Button
                className="mt-6"
                onClick={props.onOpenAddDevice}
                variant="outline"
              >
                + Add device
              </Button>
            </div>
          )}
        </div>

        {unmanagedDetectedDevices.length > 0 && (
          <div className="mt-8 rounded-[32px] border border-border bg-card-muted px-6 py-6">
            <div className="flex items-center justify-between gap-4">
              <div>
                <p className="text-[11px] font-semibold uppercase tracking-[0.24em] text-muted-foreground">
                  Detected Now
                </p>
                <p className="mt-2 text-sm text-muted-foreground">
                  Devices the backend can see right now but that are not yet in
                  your library.
                </p>
              </div>
              <StatusPill
                tone="neutral"
                value={String(unmanagedDetectedDevices.length)}
              />
            </div>

            <div className="mt-5 grid gap-3 md:grid-cols-2 xl:grid-cols-3">
              {unmanagedDetectedDevices.map((device) => (
                <div
                  className="flex items-center justify-between gap-3 rounded-[24px] bg-card px-4 py-4 ring-1 ring-border"
                  key={device.key}
                >
                  <div className="min-w-0">
                    <p className="text-sm font-semibold text-foreground">
                      {device.displayName}
                    </p>
                    <p className="mt-1 truncate text-xs text-muted-foreground">
                      {device.transport ?? "Unknown transport"}
                    </p>
                  </div>
                  <Button
                    onClick={() => props.onAddDevice(device.modelKey)}
                    variant="outline"
                  >
                    Add
                  </Button>
                </div>
              ))}
            </div>
          </div>
        )}

        <AddDeviceModal
          detectedModelKeys={detectedModelKeys}
          managedDevices={props.engineSnapshot.devices}
          onAddDevice={props.onAddDevice}
          onClose={props.onCloseAddDevice}
          open={props.isAddDeviceOpen}
          supportedDevices={props.supportedDevices}
        />
      </div>
    </div>
  );
}

function DeviceHeroImage(props: {
  alt: string;
  connected: boolean;
  selected: boolean;
  src: string;
}) {
  const [hovered, setHovered] = useState(false);
  const active = hovered;

  return (
    <div
      className="relative mx-auto flex h-[320px] w-full items-center justify-center"
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <motion.div
        animate={{
          opacity: active ? 0.5 : props.connected ? 0.18 : 0.08,
          scale: active ? 1.15 : 1,
        }}
        className="pointer-events-none absolute bg-[radial-gradient(ellipse_50%_50%_at_center,rgba(100,116,139,0.55)_0%,transparent_100%)]"
        style={{ width: 340, height: 200, bottom: -10 }}
        transition={{ duration: 0.3, ease: "easeOut" }}
      />
      <motion.img
        alt={props.alt}
        animate={{
          scale: active ? 1.15 : 1,
          opacity: props.connected ? 1 : 0.55,
        }}
        className="relative max-h-[260px] w-auto object-contain"
        data-testid={props.selected ? "device-layout-image" : undefined}
        src={props.src}
        transition={{ duration: 0.3, ease: "easeOut" }}
      />
    </div>
  );
}

function AddDeviceModal(props: {
  detectedModelKeys: Set<string>;
  managedDevices: DeviceInfo[];
  onAddDevice: (modelKey: string) => void;
  onClose: () => void;
  open: boolean;
  supportedDevices: KnownDeviceSpec[];
}) {
  const managedCounts = props.managedDevices.reduce<Map<string, number>>(
    (counts, device) => {
      counts.set(device.modelKey, (counts.get(device.modelKey) ?? 0) + 1);
      return counts;
    },
    new Map<string, number>(),
  );

  return (
    <Dialog
      open={props.open}
      onOpenChange={(nextOpen) => !nextOpen && props.onClose()}
    >
      <DialogContent className="max-w-4xl overflow-hidden p-0 sm:max-w-4xl">
        <DialogHeader className="border-b border-border px-6 py-5">
          <DialogTitle className="text-[24px]">Add Device</DialogTitle>
          <DialogDescription>
            Add supported devices to your managed library, then connect them
            when ready.
          </DialogDescription>
        </DialogHeader>

        <ScrollArea className="max-h-[70vh]">
          <div className="grid gap-4 px-6 py-6 sm:grid-cols-2">
            {props.supportedDevices.map((device) => {
              const addedCount = managedCounts.get(device.key) ?? 0;
              const isDetected = props.detectedModelKeys.has(device.key);
              const supportTone = supportLevelTone(device.support.level);
              const supportLabel = supportLevelLabel(device.support.level);
              const supportNotes = notesForSupport(device.support);
              return (
                <div
                  className="rounded-[28px] border border-border bg-card-muted p-5"
                  key={device.key}
                >
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <p className="text-sm font-semibold text-foreground">
                        {device.displayName}
                      </p>
                      <p className="mt-1 text-xs text-muted-foreground">
                        DPI {device.dpiMin}-{device.dpiMax}
                      </p>
                    </div>
                    <div className="flex flex-col items-end gap-2">
                      {isDetected ? (
                        <StatusPill tone="success" value="Detected" />
                      ) : addedCount > 0 ? (
                        <StatusPill
                          tone="neutral"
                          value={`Added ${addedCount}`}
                        />
                      ) : (
                        <StatusPill tone="neutral" value="Catalog" />
                      )}
                      <StatusPill tone={supportTone} value={supportLabel} />
                    </div>
                  </div>

                  <div className="mt-4 flex flex-wrap items-center gap-2">
                    <StatusPill
                      tone={device.supportedControls.length > 0 ? "accent" : "neutral"}
                      value={
                        device.supportedControls.length > 0
                          ? `${primaryControlsForDevice(device.supportedControls).length} control groups`
                          : "No control matrix yet"
                      }
                    />
                    {device.support.supportsDpiConfiguration ? (
                      <StatusPill tone="accent" value="DPI tuning" />
                    ) : null}
                    {device.support.supportsBatteryStatus ? (
                      <StatusPill tone="accent" value="Battery" />
                    ) : null}
                    <StatusPill
                      tone={device.support.hasInteractiveLayout ? "accent" : "neutral"}
                      value={
                        device.support.hasInteractiveLayout
                          ? "Interactive overlay"
                          : "Generic UI"
                      }
                    />
                  </div>

                  <div className="mt-5 flex items-center justify-between gap-3">
                    <p className="text-xs text-muted-foreground">
                      {supportNotes[0] ?? device.aliases[0] ?? "Supported in Mouser"}
                    </p>
                    <Button
                      onClick={() => {
                        props.onAddDevice(device.key);
                        props.onClose();
                      }}
                      variant="outline"
                    >
                      Add device
                    </Button>
                  </div>
                </div>
              );
            })}
          </div>
        </ScrollArea>
      </DialogContent>
    </Dialog>
  );
}

function AppDiscoverySheet(props: {
  appDiscovery: BootstrapPayload["appDiscovery"];
  isRefreshing: boolean;
  onClose: () => void;
  onRefresh: () => void;
  onSelectApp: (app: DiscoveredApp) => void;
  searchQuery: string;
  setSearchQuery: (value: string) => void;
}) {
  const query = props.searchQuery.trim().toLowerCase();
  const matchesQuery = (app: DiscoveredApp) => {
    if (!query) {
      return true;
    }

    return [
      app.label,
      app.description ?? "",
      ...app.matchers.map((matcher) => matcher.value),
    ].some((value) => value.toLowerCase().includes(query));
  };

  const suggestedApps = props.appDiscovery.suggestedApps.filter(matchesQuery);
  const suggestedIds = new Set(suggestedApps.map((app) => app.id));
  const browseApps = props.appDiscovery.browseApps.filter(
    (app) => !suggestedIds.has(app.id) && matchesQuery(app),
  );

  return (
    <>
      <div className="flex items-center justify-between border-b border-black/[0.06] px-6 py-5">
        <div>
          <p className="text-[24px] font-semibold tracking-[-0.05em] text-foreground">
            Add app profile
          </p>
          <p className="mt-1 text-sm text-muted-foreground">
            Pick an installed app to create or jump to its profile.
          </p>
        </div>
        <button
          className="flex h-10 w-10 items-center justify-center rounded-2xl text-muted-foreground transition hover:bg-accent hover:text-foreground"
          onClick={props.onClose}
          type="button"
        >
          <X size={18} weight="bold" />
        </button>
      </div>

      <div className="border-b border-black/[0.06] px-6 py-4">
        <div className="flex items-center gap-3">
          <Input
            placeholder="Search installed apps"
            value={props.searchQuery}
            onChange={(event) =>
              props.setSearchQuery(event.currentTarget.value)
            }
          />
          <Button
            disabled={props.isRefreshing}
            onClick={props.onRefresh}
            type="button"
            variant="outline"
          >
            {props.isRefreshing ? "Refreshing..." : "Refresh"}
          </Button>
        </div>
      </div>

      <ScrollArea className="min-h-0 flex-1">
        <div className="space-y-8 px-6 py-6">
          <div className="space-y-3">
            <div className="flex items-center justify-between gap-3">
              <p className="text-[11px] font-semibold uppercase tracking-[0.24em] text-muted-foreground">
                Suggested For This Machine
              </p>
              <StatusPill tone="neutral" value={String(suggestedApps.length)} />
            </div>

            {suggestedApps.length > 0 ? (
              suggestedApps.map((app) => (
                <AppDiscoveryRow
                  app={app}
                  key={`suggested-${app.id}`}
                  onSelect={() => props.onSelectApp(app)}
                />
              ))
            ) : (
              <EmptyState
                body="Refresh discovery or broaden the search if you expected a curated app to appear here."
                title="No suggested apps"
              />
            )}
          </div>

          <div className="space-y-3">
            <div className="flex items-center justify-between gap-3">
              <p className="text-[11px] font-semibold uppercase tracking-[0.24em] text-muted-foreground">
                Browse Apps
              </p>
              <StatusPill tone="neutral" value={String(browseApps.length)} />
            </div>

            {browseApps.length > 0 ? (
              browseApps.map((app) => (
                <AppDiscoveryRow
                  app={app}
                  key={`browse-${app.id}`}
                  onSelect={() => props.onSelectApp(app)}
                />
              ))
            ) : (
              <EmptyState
                body="No additional installed apps matched the current search."
                title="Nothing else to show"
              />
            )}
          </div>
        </div>
      </ScrollArea>
    </>
  );
}

function AppDiscoveryRow(props: { app: DiscoveredApp; onSelect: () => void }) {
  return (
    <button
      className="flex w-full items-center gap-4 rounded-[24px] bg-card-muted px-4 py-4 text-left ring-1 ring-border transition hover:bg-card"
      onClick={props.onSelect}
      type="button"
    >
      <AppIcon
        className="h-12 w-12 rounded-[18px] object-contain drop-shadow-[0_10px_18px_rgba(0,0,0,0.28)]"
        defer
        fallbackClassName="flex h-12 w-12 items-center justify-center rounded-[18px] bg-card text-sm font-semibold text-foreground ring-1 ring-border-soft"
        iconAsset={props.app.iconAsset}
        label={props.app.label}
        sourcePath={props.app.sourcePath}
      />

      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <p className="truncate text-sm font-semibold text-foreground">
            {props.app.label}
          </p>
          {props.app.suggested ? (
            <StatusPill tone="accent" value="Suggested" />
          ) : null}
        </div>
        <p className="mt-1 truncate text-xs text-muted-foreground">
          {props.app.description ??
            props.app.matchers.map(formatAppMatcher).join(", ")}
        </p>
        <p className="mt-2 text-[11px] font-medium uppercase tracking-[0.18em] text-muted-foreground">
          {formatDiscoverySource(props.app.sourceKinds[0])}
        </p>
      </div>
    </button>
  );
}

function ProfilesView(props: {
  profiles: Profile[];
  profile: Profile;
  discoveredApps: DiscoveredApp[];
  knownApps: BootstrapPayload["knownApps"];
  createProfileFromDraft: () => void;
  newProfileApp: string;
  newProfileLabel: string;
  setNewProfileApp: (value: string) => void;
  setNewProfileLabel: (value: string) => void;
  setSelectedProfileId: (profileId: string | null) => void;
  updateSelectedProfile: (mutateProfile: (profile: Profile) => void) => void;
  deleteProfile: (profileId: string) => void;
}) {
  return (
    <div className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_380px]">
      <div className="space-y-6">
        <Panel title="New Profile">
          <div className="space-y-4">
            <Field label="Label">
              <Input
                placeholder="Design apps"
                value={props.newProfileLabel}
                onChange={(event) =>
                  props.setNewProfileLabel(event.currentTarget.value)
                }
              />
            </Field>

            <Field label="Primary app matcher (optional)">
              <Input
                list="known-apps"
                placeholder="Code.exe"
                value={props.newProfileApp}
                onChange={(event) =>
                  props.setNewProfileApp(event.currentTarget.value)
                }
              />
            </Field>
            <datalist id="known-apps">
              {props.knownApps.map((app) => (
                <option key={app.executable} value={app.executable}>
                  {app.label}
                </option>
              ))}
            </datalist>

            <Button
              className="w-full"
              disabled={
                !props.newProfileLabel.trim() && !props.newProfileApp.trim()
              }
              onClick={props.createProfileFromDraft}
            >
              Create profile
            </Button>
          </div>
        </Panel>

        <Panel title="Profiles">
          <div className="space-y-3">
            {props.profiles.map((profile) => {
              const profileApp = resolveProfileApp(
                profile,
                props.discoveredApps,
                props.knownApps,
              );
              return (
                <button
                  className={[
                    "flex w-full items-center justify-between gap-4 rounded-[24px] px-4 py-4 text-left transition ring-1",
                    profile.id === props.profile.id
                      ? "bg-card shadow-[0_16px_34px_rgba(37,99,235,0.10)] ring-[#c3d8fb]"
                      : "bg-card-muted ring-border hover:bg-card",
                  ].join(" ")}
                  key={profile.id}
                  onClick={() => props.setSelectedProfileId(profile.id)}
                  type="button"
                >
                  <div className="min-w-0">
                    <p className="truncate text-sm font-semibold text-foreground">
                      {profile.label}
                    </p>
                    <p className="mt-1 truncate text-xs text-muted-foreground">
                      {profile.appMatchers.map(formatAppMatcher).join(", ") ||
                        "All applications"}
                    </p>
                  </div>
                  {profileApp ? (
                    <AppIcon
                      className="h-11 w-11 rounded-[16px] object-contain drop-shadow-[0_8px_16px_rgba(0,0,0,0.24)]"
                      fallbackClassName="flex h-11 w-11 items-center justify-center rounded-[16px] bg-card text-sm font-semibold text-foreground ring-1 ring-border-soft"
                      iconAsset={profileApp.iconAsset}
                      label={profileApp.label}
                      sourcePath={
                        "sourcePath" in profileApp
                          ? profileApp.sourcePath
                          : null
                      }
                    />
                  ) : (
                    <StatusPill tone="neutral" value={profile.id} />
                  )}
                </button>
              );
            })}
          </div>
        </Panel>
      </div>

      <Panel title="Profile">
        <div className="space-y-4">
          <Field label="Label">
            <Input
              data-testid="profile-label-input"
              value={props.profile.label}
              onChange={(event) =>
                props.updateSelectedProfile((nextProfile) => {
                  nextProfile.label = event.currentTarget.value;
                })
              }
            />
          </Field>

          <Field label="App matchers">
            <Textarea
              className="min-h-[180px] resize-y"
              rows={6}
              value={props.profile.appMatchers.map(formatAppMatcher).join("\n")}
              onChange={(event) =>
                props.updateSelectedProfile((nextProfile) => {
                  nextProfile.appMatchers = event.currentTarget.value
                    .split("\n")
                    .map((value) => value.trim())
                    .filter(Boolean)
                    .map((value) => parseDraftAppMatcher(value));
                })
              }
            />
          </Field>

          <Card className="bg-card-muted shadow-none ring-1 ring-border">
            <CardContent className="px-4 py-4">
              <p className="text-[11px] font-semibold uppercase tracking-[0.24em] text-muted-foreground">
                Current selection
              </p>
              <p
                className="mt-3 text-sm font-semibold text-foreground"
                data-testid="profile-label-display"
              >
                {props.profile.label}
              </p>
            </CardContent>
          </Card>

          <Button
            className="w-full"
            disabled={props.profile.id === "default"}
            onClick={() => props.deleteProfile(props.profile.id)}
            variant="destructive"
          >
            Delete profile
          </Button>
        </div>
      </Panel>
    </div>
  );
}

function DeviceDetailView(props: {
  config: AppConfig;
  activeDevice: DeviceInfo | null;
  activeManagedDevice: NonNullable<AppConfig["managedDevices"]>[number] | null;
  activeLayout: DeviceLayout;
  layoutChoices: BootstrapPayload["manualLayoutChoices"];
  platformCapabilities: BootstrapPayload["platformCapabilities"];
  profiles: Profile[];
  setSelectedProfileId: (profileId: string | null) => void;
  updateDeviceSettings: (
    deviceKey: string,
    settings: NonNullable<AppConfig["deviceDefaults"]>,
  ) => void;
  updateDeviceProfile: (deviceKey: string, profileId: string | null) => void;
  updateDeviceNickname: (deviceKey: string, nickname: string | null) => void;
}) {
  const activeDevice = props.activeDevice;
  const supportNotes = activeDevice
    ? notesForSupport(activeDevice.support, props.activeLayout)
    : [];
  const supportsDpiConfiguration =
    activeDevice?.support.supportsDpiConfiguration ?? false;
  const supportsBatteryStatus =
    activeDevice?.support.supportsBatteryStatus ?? false;
  const supportsGestureControls = activeDevice
    ? supportsAnyControl(activeDevice.supportedControls, GESTURE_CONTROLS)
    : false;
  const supportsHorizontalScroll = activeDevice
    ? supportsAnyControl(activeDevice.supportedControls, HSCROLL_CONTROLS)
    : false;
  const deviceSettings = normalizeDeviceSettings(
    props.activeManagedDevice?.settings ?? props.config.deviceDefaults,
  );
  const showThumbWheelTrackpadToggle =
    props.platformCapabilities.platform === "macos" &&
    isMxMasterFamilyDevice(activeDevice) &&
    supportsHorizontalScroll;
  const dpiMin = activeDevice?.dpiMin ?? 200;
  const dpiMax = activeDevice?.dpiMax ?? 8000;
  const configuredDpi = snapDpi(deviceSettings.dpi, dpiMin, dpiMax);
  const liveDpi = activeDevice
    ? snapDpi(activeDevice.currentDpi, dpiMin, dpiMax)
    : configuredDpi;
  const externalDpi = activeDevice?.connected ? liveDpi : configuredDpi;
  const [dpiDraft, setDpiDraft] = useState(externalDpi);
  const [pendingDpi, setPendingDpi] = useState<number | null>(null);
  const [nicknameDraft, setNicknameDraft] = useState(
    props.activeManagedDevice?.nickname ?? "",
  );
  const updateDeviceSettingsRef = useRef(props.updateDeviceSettings);
  const availableDpiPresets = DPI_PRESETS.filter(
    (preset) => preset >= dpiMin && preset <= dpiMax,
  );
  const assignedProfile = props.activeManagedDevice?.profileId
    ? (props.profiles.find(
        (profile) => profile.id === props.activeManagedDevice?.profileId,
      ) ?? null)
    : null;
  const profileOptions = [
    { label: "Auto by app", value: "" },
    ...props.profiles.map(
      (profile) =>
        ({
          label: profile.label,
          value: profile.id,
        }) satisfies AppSelectOption,
    ),
  ];
  const updateManagedDeviceSettings = (
    mutateSettings: (
      settings: NonNullable<AppConfig["deviceDefaults"]>,
    ) => void,
  ) => {
    if (!props.activeManagedDevice) {
      return;
    }

    const nextSettings = normalizeDeviceSettings(
      props.activeManagedDevice.settings,
    );
    mutateSettings(nextSettings);
    props.updateDeviceSettings(props.activeManagedDevice.id, nextSettings);
  };

  useEffect(() => {
    updateDeviceSettingsRef.current = props.updateDeviceSettings;
  }, [props.updateDeviceSettings]);

  useEffect(() => {
    setNicknameDraft(props.activeManagedDevice?.nickname ?? "");
  }, [props.activeManagedDevice?.id, props.activeManagedDevice?.nickname]);

  useEffect(() => {
    if (!activeDevice) {
      setPendingDpi(null);
      setDpiDraft(configuredDpi);
      return;
    }

    if (pendingDpi != null) {
      const pendingSettled =
        configuredDpi === pendingDpi || liveDpi === pendingDpi;
      if (!pendingSettled) {
        return;
      }
      setPendingDpi(null);
    }

    setDpiDraft(externalDpi);
  }, [
    activeDevice?.connected,
    activeDevice?.key,
    configuredDpi,
    externalDpi,
    liveDpi,
    pendingDpi,
  ]);

  useEffect(() => {
    if (!props.activeManagedDevice || pendingDpi == null) {
      return;
    }

    const managedDevice = props.activeManagedDevice;
    const timeout = window.setTimeout(() => {
      const nextSettings = normalizeDeviceSettings(managedDevice.settings);
      nextSettings.dpi = pendingDpi;
      updateDeviceSettingsRef.current(managedDevice.id, nextSettings);
    }, 400);

    return () => window.clearTimeout(timeout);
  }, [pendingDpi, props.activeManagedDevice]);

  if (!activeDevice) {
    return (
      <EmptyStage
        body="Pick a device from the dashboard to tune scroll behavior, DPI, and layout handling."
        title="No device selected"
      />
    );
  }

  const commitNickname = () => {
    if (!props.activeManagedDevice) {
      return;
    }

    const nextNickname = normalizeOptionalText(nicknameDraft);
    const currentNickname = normalizeOptionalText(
      props.activeManagedDevice.nickname,
    );
    if (nextNickname === currentNickname) {
      return;
    }

    props.updateDeviceNickname(props.activeManagedDevice.id, nextNickname);
  };

  const layoutOptions = props.layoutChoices.map(
    (choice) =>
      ({
        label: choice.label,
        value: choice.key,
      }) satisfies AppSelectOption,
  );
  const queueDpiChange = (value: number) => {
    const nextDpi = snapDpi(value, activeDevice.dpiMin, activeDevice.dpiMax);
    setDpiDraft(nextDpi);
    setPendingDpi(
      nextDpi === configuredDpi &&
        (!activeDevice.connected || nextDpi === liveDpi)
        ? null
        : nextDpi,
    );
  };

  return (
    <div className="grid gap-8 xl:grid-cols-[minmax(0,1fr)_340px]">
      <div className="space-y-8">
        <div>
          <div className="flex min-h-[420px] flex-col items-center justify-center px-8 py-10 text-center">
            <img
              alt={activeDevice.displayName}
              className="max-h-[300px] w-auto object-contain drop-shadow-[0_28px_44px_rgba(15,23,42,0.16)]"
              data-testid="device-layout-image"
              src={props.activeLayout.imageAsset}
            />
            <div className="mt-6 flex flex-wrap items-center justify-center gap-2">
              <StatusPill
                tone={activeDevice.connected ? "success" : "neutral"}
                value={batteryStatusPillValue(activeDevice)}
              />
              <StatusPill
                tone={activeDevice.connected ? "success" : "neutral"}
                value={activeDevice.transport ?? "Unknown transport"}
              />
              <StatusPill
                tone="neutral"
                value={`DPI ${activeDevice.dpiMin}-${activeDevice.dpiMax}`}
              />
            </div>
          </div>
        </div>

        {supportNotes.length > 0 ? (
          <SupportNotesPanel notes={supportNotes} />
        ) : null}

        <Panel title={`${activeDevice.displayName} Tuning`}>
          <div className="grid gap-4 md:grid-cols-2">
            <Field label="Nickname (optional)">
              <Input
                placeholder={activeDevice.displayName}
                value={nicknameDraft}
                onBlur={commitNickname}
                onChange={(event) =>
                  setNicknameDraft(event.currentTarget.value)
                }
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    commitNickname();
                    event.currentTarget.blur();
                  }
                }}
              />
            </Field>

            <Field label="Assigned profile">
              <AppSelect
                ariaLabel="Assigned profile"
                options={profileOptions}
                value={props.activeManagedDevice?.profileId ?? ""}
                onValueChange={(value) => {
                  props.updateDeviceProfile(
                    activeDevice.key,
                    normalizeOptionalText(value),
                  );
                  props.setSelectedProfileId(normalizeOptionalText(value));
                }}
              />
            </Field>

            <div className="space-y-2.5 md:col-span-2">
              <Label className="text-sm font-medium text-foreground">
                DPI
              </Label>
              {supportsDpiConfiguration ? (
                <div className="rounded-[24px] bg-card-muted p-5 ring-1 ring-border">
                  <div className="flex flex-wrap items-start justify-between gap-3">
                    <div className="space-y-1">
                      <p className="text-sm font-medium text-foreground">
                        Pointer speed
                      </p>
                      <p className="text-xs text-muted-foreground">
                        Drag to choose a DPI, then pause briefly to apply it.
                        {activeDevice.connected
                          ? ` The device is currently reporting ${liveDpi} DPI.`
                          : " Changes will apply once the device reconnects."}
                      </p>
                    </div>
                    <div className="rounded-full bg-card px-3 py-1.5 text-sm font-semibold text-foreground ring-1 ring-border">
                      {dpiDraft} DPI
                    </div>
                  </div>

                  <div className="mt-5 flex items-center gap-3">
                    <span className="w-12 text-xs text-muted-foreground">
                      {activeDevice.dpiMin}
                    </span>
                    <Slider
                      aria-label="Pointer speed"
                      className="flex-1"
                      data-testid="dpi-slider"
                      max={activeDevice.dpiMax}
                      min={activeDevice.dpiMin}
                      step={50}
                      value={[dpiDraft]}
                      onValueChange={(values) => {
                        const nextValue = values[0];
                        if (nextValue != null) {
                          queueDpiChange(nextValue);
                        }
                      }}
                    />
                    <span className="w-12 text-right text-xs text-muted-foreground">
                      {activeDevice.dpiMax}
                    </span>
                  </div>

                  <div className="mt-4 flex flex-wrap items-center gap-2">
                    {availableDpiPresets.map((preset) => (
                      <Button
                        key={preset}
                        data-testid={`dpi-preset-${preset}`}
                        size="sm"
                        type="button"
                        variant={dpiDraft === preset ? "default" : "outline"}
                        onClick={() => queueDpiChange(preset)}
                      >
                        {preset}
                      </Button>
                    ))}
                    {pendingDpi != null ? (
                      <span className="text-xs text-muted-foreground">
                        Applying {pendingDpi} DPI...
                      </span>
                    ) : null}
                  </div>
                </div>
              ) : (
                <Card className="bg-card-muted shadow-none ring-1 ring-border">
                  <CardContent className="px-4 py-4">
                    <p className="text-sm font-medium text-foreground">
                      DPI tuning is not part of the validated support matrix for
                      this device yet.
                    </p>
                  </CardContent>
                </Card>
              )}
            </div>

            <Field label="Manual layout override">
              <AppSelect
                ariaLabel="Manual layout override"
                options={layoutOptions}
                placeholder="Auto-detect"
                value={deviceSettings.manualLayoutOverride ?? ""}
                onValueChange={(value) =>
                  updateManagedDeviceSettings((settings) => {
                    settings.manualLayoutOverride = value || null;
                  })
                }
              />
            </Field>

            {supportsHorizontalScroll ? (
              <SwitchRow
                checked={deviceSettings.invertHorizontalScroll}
                label="Invert thumb wheel"
                onChange={(value) =>
                  updateManagedDeviceSettings((settings) => {
                    settings.invertHorizontalScroll = value;
                  })
                }
              />
            ) : null}
            <SwitchRow
              checked={deviceSettings.invertVerticalScroll}
              label="Invert vertical scroll"
              onChange={(value) =>
                updateManagedDeviceSettings((settings) => {
                  settings.invertVerticalScroll = value;
                })
              }
            />
            {showThumbWheelTrackpadToggle ? (
              <SwitchRow
                checked={
                  deviceSettings.macosThumbWheelSimulateTrackpad ?? false
                }
                description="Alpha: converts the MX Master thumb wheel into macOS-style trackpad horizontal swipe events for apps that need them."
                label={
                  <span className="flex flex-wrap items-center gap-2">
                    <span>Simulate trackpad swipe from thumb wheel</span>
                    <Badge variant="secondary">Alpha</Badge>
                  </span>
                }
                onChange={(value) =>
                  updateManagedDeviceSettings((settings) => {
                    settings.macosThumbWheelSimulateTrackpad = value;
                  })
                }
              />
            ) : null}
            {showThumbWheelTrackpadToggle &&
            deviceSettings.macosThumbWheelSimulateTrackpad ? (
              <Field label="Thumb wheel swipe hold (ms)">
                <Input
                  aria-label="Thumb wheel swipe hold (ms)"
                  max={5000}
                  min={0}
                  step={50}
                  type="number"
                  value={deviceSettings.macosThumbWheelTrackpadHoldTimeoutMs}
                  onChange={(event) =>
                    updateManagedDeviceSettings((settings) => {
                      settings.macosThumbWheelTrackpadHoldTimeoutMs = Number(
                        event.currentTarget.value,
                      );
                    })
                  }
                />
              </Field>
            ) : null}
            {supportsGestureControls ? (
              <>
                <Field label="Gesture threshold">
                  <Input
                    type="number"
                    value={deviceSettings.gestureThreshold}
                    onChange={(event) =>
                      updateManagedDeviceSettings((settings) => {
                        settings.gestureThreshold = Number(
                          event.currentTarget.value,
                        );
                      })
                    }
                  />
                </Field>
                <Field label="Gesture deadzone">
                  <Input
                    type="number"
                    value={deviceSettings.gestureDeadzone}
                    onChange={(event) =>
                      updateManagedDeviceSettings((settings) => {
                        settings.gestureDeadzone = Number(
                          event.currentTarget.value,
                        );
                      })
                    }
                  />
                </Field>
                <Field label="Gesture timeout (ms)">
                  <Input
                    type="number"
                    value={deviceSettings.gestureTimeoutMs}
                    onChange={(event) =>
                      updateManagedDeviceSettings((settings) => {
                        settings.gestureTimeoutMs = Number(
                          event.currentTarget.value,
                        );
                      })
                    }
                  />
                </Field>
                <Field label="Gesture cooldown (ms)">
                  <Input
                    type="number"
                    value={deviceSettings.gestureCooldownMs}
                    onChange={(event) =>
                      updateManagedDeviceSettings((settings) => {
                        settings.gestureCooldownMs = Number(
                          event.currentTarget.value,
                        );
                      })
                    }
                  />
                </Field>
              </>
            ) : null}
          </div>
        </Panel>
      </div>

      <Panel title="Status">
        <div className="space-y-3">
          <CapabilityRow
            label="Support"
            value={supportLevelLabel(activeDevice.support.level)}
          />
          <CapabilityRow
            label="Live DPI"
            value={
              supportsDpiConfiguration
                ? activeDevice.connected
                  ? `${liveDpi}`
                  : "Not connected"
                : "Not validated yet"
            }
          />
          <CapabilityRow
            label="Configured DPI"
            value={
              supportsDpiConfiguration ? `${configuredDpi}` : "Not available"
            }
          />
          <CapabilityRow
            label="Transport"
            value={activeDevice.transport ?? "Unknown"}
          />
          <CapabilityRow
            label="Battery"
            value={
              batteryStatusLabel(activeDevice) ??
              (supportsBatteryStatus
                  ? "N/A"
                  : "Not validated yet")
            }
          />
          <CapabilityRow
            label="Layout family"
            value={props.activeLayout.label}
          />
          <CapabilityRow
            label="Remappable controls"
            value={controlSummaryLabel(activeDevice.supportedControls)}
          />
          <CapabilityRow
            label="Assigned profile"
            value={assignedProfile?.label ?? "Auto by app"}
          />
          <CapabilityRow
            label="Product"
            value={activeDevice.productName ?? activeDevice.displayName}
          />
          <CapabilityRow
            label="Status"
            value={activeDevice.connected ? "Connected" : "Added"}
          />
        </div>
      </Panel>
    </div>
  );
}

function AppSettingsDialog(props: {
  config: AppConfig;
  layoutChoices: BootstrapPayload["manualLayoutChoices"];
  open: boolean;
  onClose: () => void;
  platformCapabilities: BootstrapPayload["platformCapabilities"];
  saveAppSettings: (
    mutateSettings: (nextSettings: AppConfig["settings"]) => void,
  ) => void;
  saveDeviceDefaults: (
    mutateSettings: (
      nextSettings: NonNullable<AppConfig["deviceDefaults"]>,
    ) => void,
  ) => void;
}) {
  const appearanceOptions = [
    { label: "System", value: "system" },
    { label: "Light", value: "light" },
    { label: "Dark", value: "dark" },
  ] satisfies AppSelectOption[];
  const defaultSettings = normalizeDeviceSettings(props.config.deviceDefaults);
  const [defaultDpiDraft, setDefaultDpiDraft] = useState(defaultSettings.dpi);
  const [pendingDefaultDpi, setPendingDefaultDpi] = useState<number | null>(
    null,
  );
  const saveDeviceDefaultsRef = useRef(props.saveDeviceDefaults);
  const defaultLayoutOptions = props.layoutChoices.map(
    (choice) =>
      ({
        label: choice.label,
        value: choice.key,
      }) satisfies AppSelectOption,
  );

  useEffect(() => {
    saveDeviceDefaultsRef.current = props.saveDeviceDefaults;
  }, [props.saveDeviceDefaults]);

  useEffect(() => {
    if (
      pendingDefaultDpi != null &&
      defaultSettings.dpi !== pendingDefaultDpi
    ) {
      return;
    }

    setPendingDefaultDpi(null);
    setDefaultDpiDraft(defaultSettings.dpi);
  }, [defaultSettings.dpi, pendingDefaultDpi]);

  useEffect(() => {
    if (pendingDefaultDpi == null) {
      return;
    }

    const timeout = window.setTimeout(() => {
      const nextSettings = normalizeDeviceSettings(props.config.deviceDefaults);
      nextSettings.dpi = pendingDefaultDpi;
      saveDeviceDefaultsRef.current((settings) => {
        Object.assign(settings, nextSettings);
      });
    }, 400);

    return () => window.clearTimeout(timeout);
  }, [pendingDefaultDpi, props.config.deviceDefaults]);

  return (
    <Dialog
      open={props.open}
      onOpenChange={(nextOpen) => !nextOpen && props.onClose()}
    >
      <DialogContent className="max-w-4xl overflow-hidden p-0 sm:max-w-4xl">
        <DialogHeader className="border-b border-border px-6 py-5">
          <DialogTitle className="text-[24px]">App Settings</DialogTitle>
          <DialogDescription>
            Settings here affect Mouser globally, not just the currently
            selected device.
          </DialogDescription>
        </DialogHeader>

        <ScrollArea className="max-h-[76vh]">
          <div className="grid gap-6 px-6 py-6 xl:grid-cols-[minmax(0,1fr)_320px]">
            <div className="space-y-6">
              <Panel title="General">
                <div className="grid gap-4 md:grid-cols-2">
                  <SwitchRow
                    checked={props.config.settings.startMinimized}
                    label="Start minimized"
                    onChange={(value) =>
                      props.saveAppSettings((nextSettings) => {
                        nextSettings.startMinimized = value;
                      })
                    }
                  />
                  <SwitchRow
                    checked={props.config.settings.startAtLogin}
                    label="Start at login"
                    onChange={(value) =>
                      props.saveAppSettings((nextSettings) => {
                        nextSettings.startAtLogin = value;
                      })
                    }
                  />
                  <SwitchRow
                    checked={props.config.settings.debugMode}
                    label="Enable debug mode"
                    onChange={(value) =>
                      props.saveAppSettings((nextSettings) => {
                        nextSettings.debugMode = value;
                      })
                    }
                  />
                  <Field label="Appearance mode">
                    <AppSelect
                      ariaLabel="Appearance mode"
                      options={appearanceOptions}
                      value={props.config.settings.appearanceMode}
                      onValueChange={(value) =>
                        props.saveAppSettings((nextSettings) => {
                          nextSettings.appearanceMode =
                            value as AppConfig["settings"]["appearanceMode"];
                        })
                      }
                    />
                  </Field>
                </div>
              </Panel>

              <Panel
                subtitle="These values seed newly added devices before they get their own settings."
                title="Defaults For New Devices"
              >
                <div className="grid gap-4 md:grid-cols-2">
                  <div className="space-y-2.5 md:col-span-2">
                    <Label className="text-sm font-medium text-foreground">
                      Default DPI
                    </Label>
                    <div className="rounded-[24px] bg-card-muted p-5 ring-1 ring-border">
                      <div className="flex flex-wrap items-start justify-between gap-3">
                        <div className="space-y-1">
                          <p className="text-sm font-medium text-foreground">
                            Pointer speed for new devices
                          </p>
                          <p className="text-xs text-muted-foreground">
                            Drag to choose a default, then pause briefly to save
                            it.
                          </p>
                        </div>
                        <div className="rounded-full bg-card px-3 py-1.5 text-sm font-semibold text-foreground ring-1 ring-border">
                          {defaultDpiDraft} DPI
                        </div>
                      </div>

                      <div className="mt-5 flex items-center gap-3">
                        <span className="w-12 text-xs text-muted-foreground">
                          200
                        </span>
                        <Slider
                          aria-label="Default pointer speed"
                          className="flex-1"
                          max={8000}
                          min={200}
                          step={50}
                          value={[defaultDpiDraft]}
                          onValueChange={(values) => {
                            const nextValue = values[0];
                            if (nextValue == null) {
                              return;
                            }
                            const nextDpi = snapDpi(nextValue, 200, 8000);
                            setDefaultDpiDraft(nextDpi);
                            setPendingDefaultDpi(
                              nextDpi === defaultSettings.dpi ? null : nextDpi,
                            );
                          }}
                        />
                        <span className="w-12 text-right text-xs text-muted-foreground">
                          8000
                        </span>
                      </div>

                      <div className="mt-4 flex flex-wrap items-center gap-2">
                        {DPI_PRESETS.map((preset) => (
                          <Button
                            key={preset}
                            size="sm"
                            type="button"
                            variant={
                              defaultDpiDraft === preset ? "default" : "outline"
                            }
                            onClick={() => {
                              setDefaultDpiDraft(preset);
                              setPendingDefaultDpi(
                                preset === defaultSettings.dpi ? null : preset,
                              );
                            }}
                          >
                            {preset}
                          </Button>
                        ))}
                        {pendingDefaultDpi != null ? (
                          <span className="text-xs text-muted-foreground">
                            Saving {pendingDefaultDpi} DPI...
                          </span>
                        ) : null}
                      </div>
                    </div>
                  </div>

                  <Field label="Default manual layout override">
                    <AppSelect
                      ariaLabel="Default manual layout override"
                      options={defaultLayoutOptions}
                      placeholder="Auto-detect"
                      value={defaultSettings.manualLayoutOverride ?? ""}
                      onValueChange={(value) =>
                        props.saveDeviceDefaults((nextSettings) => {
                          nextSettings.manualLayoutOverride = value || null;
                        })
                      }
                    />
                  </Field>

                  <SwitchRow
                    checked={defaultSettings.invertHorizontalScroll}
                    label="Invert thumb wheel by default"
                    onChange={(value) =>
                      props.saveDeviceDefaults((nextSettings) => {
                        nextSettings.invertHorizontalScroll = value;
                      })
                    }
                  />
                  <SwitchRow
                    checked={defaultSettings.invertVerticalScroll}
                    label="Invert vertical scroll by default"
                    onChange={(value) =>
                      props.saveDeviceDefaults((nextSettings) => {
                        nextSettings.invertVerticalScroll = value;
                      })
                    }
                  />
                  <Field label="Default gesture threshold">
                    <Input
                      type="number"
                      value={defaultSettings.gestureThreshold}
                      onChange={(event) =>
                        props.saveDeviceDefaults((nextSettings) => {
                          nextSettings.gestureThreshold = Number(
                            event.currentTarget.value,
                          );
                        })
                      }
                    />
                  </Field>
                  <Field label="Default gesture deadzone">
                    <Input
                      type="number"
                      value={defaultSettings.gestureDeadzone}
                      onChange={(event) =>
                        props.saveDeviceDefaults((nextSettings) => {
                          nextSettings.gestureDeadzone = Number(
                            event.currentTarget.value,
                          );
                        })
                      }
                    />
                  </Field>
                  <Field label="Default gesture timeout (ms)">
                    <Input
                      type="number"
                      value={defaultSettings.gestureTimeoutMs}
                      onChange={(event) =>
                        props.saveDeviceDefaults((nextSettings) => {
                          nextSettings.gestureTimeoutMs = Number(
                            event.currentTarget.value,
                          );
                        })
                      }
                    />
                  </Field>
                  <Field label="Default gesture cooldown (ms)">
                    <Input
                      type="number"
                      value={defaultSettings.gestureCooldownMs}
                      onChange={(event) =>
                        props.saveDeviceDefaults((nextSettings) => {
                          nextSettings.gestureCooldownMs = Number(
                            event.currentTarget.value,
                          );
                        })
                      }
                    />
                  </Field>
                </div>
              </Panel>
            </div>

            <Panel title="Platform">
              <div className="space-y-3">
                <CapabilityRow
                  label="Platform"
                  value={props.platformCapabilities.platform}
                />
                <CapabilityRow
                  label="Live HID"
                  value={
                    props.platformCapabilities.liveHidAvailable
                      ? "Ready"
                      : "Fallback"
                  }
                />
                <CapabilityRow
                  label="Live remapping"
                  value={
                    props.platformCapabilities.mappingEngineReady
                      ? "Ready"
                      : "Not yet"
                  }
                />
                <CapabilityRow
                  label="Tray"
                  value={
                    props.platformCapabilities.trayReady ? "Ready" : "Pending"
                  }
                />
                <CapabilityRow
                  label="HID backend"
                  value={props.platformCapabilities.activeHidBackend}
                />
                <CapabilityRow
                  label="Focus backend"
                  value={props.platformCapabilities.activeFocusBackend}
                />
              </div>
            </Panel>
          </div>
        </ScrollArea>
      </DialogContent>
    </Dialog>
  );
}

function DebugView(props: {
  activeDevice: DeviceInfo | null;
  config: AppConfig;
  platformCapabilities: BootstrapPayload["platformCapabilities"];
  importDraft: string;
  importSourcePath: string;
  importWarnings: string[];
  saveAppSettings: (
    mutateSettings: (nextSettings: AppConfig["settings"]) => void,
  ) => void;
  onImport: () => void;
  setImportDraft: (value: string) => void;
  setImportSourcePath: (value: string) => void;
}) {
  const debugLogGroups = {
    ...DEFAULT_DEBUG_LOG_GROUPS,
    ...props.config.settings.debugLogGroups,
  };

  const updateDebugLogGroups = (
    mutateGroups: (
      nextGroups: NonNullable<AppConfig["settings"]["debugLogGroups"]>,
    ) => void,
  ) => {
    props.saveAppSettings((nextSettings) => {
      nextSettings.debugLogGroups = {
        ...DEFAULT_DEBUG_LOG_GROUPS,
        ...nextSettings.debugLogGroups,
      };
      mutateGroups(nextSettings.debugLogGroups);
    });
  };

  return (
    <div className="grid gap-6 xl:grid-cols-[minmax(0,1.2fr)_360px]">
      <Panel title="Logging">
        <div className="space-y-5">
          <div className="flex flex-wrap items-center gap-3">
            <StatusPill
              tone={props.config.settings.debugMode ? "accent" : "neutral"}
              value={props.config.settings.debugMode ? "Debug on" : "Debug off"}
            />
            <p className="text-sm leading-6 text-muted-foreground">
              Backend logs now go to the Rust console only. Run Mouser from a
              terminal to inspect them.
            </p>
          </div>

          <div className="grid gap-3 md:grid-cols-2">
            <CapabilityRow
              label="Active HID backend"
              value={props.platformCapabilities.activeHidBackend}
            />
            <CapabilityRow
              label="Active hook backend"
              value={props.platformCapabilities.activeHookBackend}
            />
            <CapabilityRow
              label="Active focus backend"
              value={props.platformCapabilities.activeFocusBackend}
            />
            <CapabilityRow
              label="iokit backend"
              value={
                props.platformCapabilities.iokitAvailable ? "Ready" : "Not ported"
              }
            />
          </div>

          <div className="grid gap-3 rounded-[28px] bg-card-muted p-5 ring-1 ring-border md:grid-cols-2">
            <SwitchRow
              checked={props.config.settings.debugMode}
              label="Enable debug mode"
              onChange={(value) =>
                props.saveAppSettings((nextSettings) => {
                  nextSettings.debugMode = value;
                })
              }
            />
            <SwitchRow
              checked={debugLogGroups.runtime}
              label="Runtime"
              onChange={(value) =>
                updateDebugLogGroups((nextGroups) => {
                  nextGroups.runtime = value;
                })
              }
            />
            <SwitchRow
              checked={debugLogGroups.hookRouting}
              label="Hook routing"
              onChange={(value) =>
                updateDebugLogGroups((nextGroups) => {
                  nextGroups.hookRouting = value;
                })
              }
            />
            <SwitchRow
              checked={debugLogGroups.gestures}
              label="Gestures"
              onChange={(value) =>
                updateDebugLogGroups((nextGroups) => {
                  nextGroups.gestures = value;
                })
              }
            />
            <SwitchRow
              checked={debugLogGroups.thumbWheel}
              label="Thumb wheel"
              onChange={(value) =>
                updateDebugLogGroups((nextGroups) => {
                  nextGroups.thumbWheel = value;
                })
              }
            />
            <SwitchRow
              checked={debugLogGroups.hid}
              label="Raw HID"
              onChange={(value) =>
                updateDebugLogGroups((nextGroups) => {
                  nextGroups.hid = value;
                })
              }
            />
          </div>

          <div className="rounded-[28px] border border-dashed border-border-strong bg-card-muted p-5">
            <p className="text-sm font-semibold text-foreground">
              Frontend debug log disabled
            </p>
            <p className="mt-2 text-sm leading-6 text-muted-foreground">
              High-volume backend logs are no longer streamed into the UI. Use
              the switches above to choose which groups get printed by the Rust
              backend.
            </p>
          </div>
        </div>
      </Panel>

      <div className="space-y-6">
        <Panel title="Battery Telemetry">
          {props.activeDevice?.battery ? (
            <div className="space-y-4">
              <div className="grid gap-3 md:grid-cols-2">
                <CapabilityRow
                  label="Device"
                  value={props.activeDevice.displayName}
                />
                <CapabilityRow
                  label="Normalized label"
                  value={props.activeDevice.battery.label}
                />
                <CapabilityRow
                  label="Battery kind"
                  value={props.activeDevice.battery.kind}
                />
                <CapabilityRow
                  label="Source feature"
                  value={props.activeDevice.battery.sourceFeature ?? "Unknown"}
                />
                <CapabilityRow
                  label="Percentage"
                  value={
                    props.activeDevice.battery.percentage != null
                      ? `${props.activeDevice.battery.percentage}%`
                      : "N/A"
                  }
                />
                <CapabilityRow
                  label="Connected"
                  value={props.activeDevice.connected ? "Yes" : "No"}
                />
              </div>

              <div className="space-y-3">
                <Field label="Raw capabilities bytes">
                  <div className="rounded-[20px] bg-card-muted px-4 py-3 font-mono text-xs text-muted-foreground ring-1 ring-border">
                    {formatHexByteList(props.activeDevice.battery.rawCapabilities)}
                  </div>
                </Field>
                <Field label="Raw status bytes">
                  <div className="rounded-[20px] bg-card-muted px-4 py-3 font-mono text-xs text-muted-foreground ring-1 ring-border">
                    {formatHexByteList(props.activeDevice.battery.rawStatus)}
                  </div>
                </Field>
              </div>
            </div>
          ) : (
            <EmptyState
              title="No battery telemetry"
              body={
                props.activeDevice
                  ? "The selected device has not reported battery data yet, or the current backend could not read it."
                  : "Select a device to inspect its live battery telemetry."
              }
            />
          )}
        </Panel>
        <Panel title="Import">
          <div className="space-y-4">
            <Field label="Optional source path">
              <Input
                placeholder="~/Library/Application Support/Mouser/config.json"
                value={props.importSourcePath}
                onChange={(event) =>
                  props.setImportSourcePath(event.currentTarget.value)
                }
              />
            </Field>
            <Field label="Legacy Mouser JSON">
              <Textarea
                className="min-h-[280px] resize-y font-mono text-xs leading-6"
                data-testid="legacy-import-input"
                rows={12}
                value={props.importDraft}
                onChange={(event) =>
                  props.setImportDraft(event.currentTarget.value)
                }
              />
            </Field>
            <Button
              className="w-full"
              data-testid="legacy-import-button"
              onClick={props.onImport}
            >
              Import legacy config
            </Button>
            {props.importWarnings.length > 0 && (
              <ul className="space-y-2 rounded-[24px] border border-[#efd8af] bg-[#fff9ef] p-4 text-sm text-[#8b5f1b]">
                {props.importWarnings.map((warning) => (
                  <li key={warning}>{warning}</li>
                ))}
              </ul>
            )}
          </div>
        </Panel>
      </div>
    </div>
  );
}

function SectionNavButton(props: {
  label: string;
  active: boolean;
  compact?: boolean;
  onClick: () => void;
  icon: (props: SVGProps<SVGSVGElement>) => ReactNode;
}) {
  const Icon = props.icon;

  return (
    <button
      aria-label={props.label}
      className={cn(
        "flex items-center gap-3 rounded-2xl px-4 py-2.5 text-left text-sm font-semibold transition-all",
        props.active
          ? "bg-[#10b981] text-white shadow-[0_4px_12px_rgba(16,185,129,0.3)]"
          : "text-muted-foreground hover:bg-black/5 hover:text-foreground",
      )}
      onClick={props.onClick}
      type="button"
    >
      <Icon className="h-[18px] w-[18px]" />
      <span>{props.label}</span>
    </button>
  );
}

function Panel(props: {
  title: string;
  subtitle?: string;
  children: ReactNode;
}) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>{props.title}</CardTitle>
        {props.subtitle ? (
          <CardDescription>{props.subtitle}</CardDescription>
        ) : null}
      </CardHeader>
      <CardContent>{props.children}</CardContent>
    </Card>
  );
}

function SupportNotesPanel(props: { notes: string[]; className?: string }) {
  if (props.notes.length === 0) {
    return null;
  }

  return (
    <Card className={cn("bg-card-muted shadow-none ring-1 ring-border", props.className)}>
      <CardContent className="px-4 py-4">
        <p className="text-[11px] font-semibold uppercase tracking-[0.24em] text-muted-foreground">
          Support Notes
        </p>
        <ul className="mt-3 space-y-2 text-sm leading-6 text-muted-foreground">
          {props.notes.map((note) => (
            <li key={note}>{note}</li>
          ))}
        </ul>
      </CardContent>
    </Card>
  );
}

function RuntimeHealthNotice(props: {
  runtimeHealth?: RuntimeHealth;
  className?: string;
}) {
  const issues = runtimeHealthIssues(props.runtimeHealth);

  if (issues.length === 0) {
    return null;
  }

  const hasError = issues.some((issue) => issue.state === "error");

  return (
    <Card
      className={cn(
        "shadow-none ring-1",
        hasError
          ? "border-red-200 bg-red-50/80 ring-red-100"
          : "border-amber-200 bg-amber-50/80 ring-amber-100",
        props.className,
      )}
    >
      <CardContent className="px-5 py-4">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <WarningCircle
                className={cn(
                  "h-4 w-4",
                  hasError ? "text-red-600" : "text-amber-600",
                )}
                weight="fill"
              />
              <p className="text-sm font-semibold text-foreground">
                Backend health needs attention
              </p>
            </div>
            <p className="mt-2 text-sm leading-6 text-muted-foreground">
              Mouser is still running, but some backend subsystems are stale or
              unavailable.
            </p>
          </div>
          <StatusPill
            tone={hasError ? "danger" : "warning"}
            value={hasError ? "Degraded" : "Stale"}
          />
        </div>

        <div className="mt-4 grid gap-3 md:grid-cols-2">
          {issues.map((issue) => (
            <div
              className="rounded-[20px] bg-white/80 px-4 py-3 ring-1 ring-black/5"
              key={issue.key}
            >
              <div className="flex items-center justify-between gap-3">
                <p className="text-sm font-medium text-foreground">
                  {issue.label}
                </p>
                <StatusPill
                  tone={runtimeHealthTone(issue.state)}
                  value={issue.state === "error" ? "Error" : "Stale"}
                />
              </div>
              <p className="mt-2 text-xs leading-5 text-muted-foreground">
                {issue.message}
              </p>
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  );
}

function Field(props: { label: string; children: ReactNode }) {
  return (
    <div className="space-y-2.5">
      <Label className="text-sm font-medium text-foreground">
        {props.label}
      </Label>
      {props.children}
    </div>
  );
}

function AppSelect(props: {
  ariaLabel: string;
  className?: string;
  onValueChange: (value: string) => void;
  options: AppSelectOption[];
  placeholder?: string;
  value: string;
}) {
  const groupedOptions = props.options.reduce<Map<string, AppSelectOption[]>>(
    (groups, option) => {
      const groupKey = option.group ?? "";
      const next = groups.get(groupKey) ?? [];
      next.push(option);
      groups.set(groupKey, next);
      return groups;
    },
    new Map<string, AppSelectOption[]>(),
  );
  const selectValue = props.value === "" ? EMPTY_SELECT_VALUE : props.value;
  const optionTestId = (value: string) =>
    `select-option-${(value || "empty").replace(/[^a-z0-9_-]+/gi, "-")}`;

  return (
    <Select
      onValueChange={(value) =>
        props.onValueChange(value === EMPTY_SELECT_VALUE ? "" : value)
      }
      value={selectValue}
    >
      <SelectTrigger aria-label={props.ariaLabel} className={props.className}>
        <SelectValue placeholder={props.placeholder} />
      </SelectTrigger>
      <SelectContent>
        {[...groupedOptions.entries()].map(([group, options]) =>
          group ? (
            <SelectGroup key={group}>
              <SelectLabel>{group}</SelectLabel>
              {options.map((option) => (
                <SelectItem
                  data-testid={optionTestId(option.value)}
                  disabled={option.disabled}
                  key={option.value || EMPTY_SELECT_VALUE}
                  value={
                    option.value === "" ? EMPTY_SELECT_VALUE : option.value
                  }
                >
                  <span>{option.label}</span>
                  {option.badge ? (
                    <Badge className="ml-auto" variant="outline">
                      {option.badge}
                    </Badge>
                  ) : null}
                </SelectItem>
              ))}
            </SelectGroup>
          ) : (
            options.map((option) => (
              <SelectItem
                data-testid={optionTestId(option.value)}
                disabled={option.disabled}
                key={option.value || EMPTY_SELECT_VALUE}
                value={option.value === "" ? EMPTY_SELECT_VALUE : option.value}
              >
                <span>{option.label}</span>
                {option.badge ? (
                  <Badge className="ml-auto" variant="outline">
                    {option.badge}
                  </Badge>
                ) : null}
              </SelectItem>
            ))
          ),
        )}
      </SelectContent>
    </Select>
  );
}

function SwitchRow(props: {
  label: ReactNode;
  checked: boolean;
  description?: ReactNode;
  onChange: (value: boolean) => void;
}) {
  const switchId = useId();

  return (
    <div className="flex items-start justify-between gap-4 rounded-[24px] bg-card-muted px-4 py-4 text-sm ring-1 ring-border">
      <div className="space-y-1.5">
        <Label
          className="font-medium text-foreground"
          htmlFor={switchId}
        >
          {props.label}
        </Label>
        {props.description ? (
          <p className="max-w-md text-xs leading-5 text-muted-foreground">
            {props.description}
          </p>
        ) : null}
      </div>
      <Switch
        className="mt-0.5 shrink-0"
        checked={props.checked}
        id={switchId}
        onCheckedChange={props.onChange}
      />
    </div>
  );
}

function ButtonsWorkbench(props: {
  activeDevice: DeviceInfo;
  hotspots: DeviceLayout["hotspots"];
  layout: DeviceLayout;
  notes: string[];
  profile: Profile;
  actionLookup: Map<string, ActionDefinition>;
  controlSpecs: DeviceControlSpec[];
  selectedControl: LogicalControl | null;
  onSelectControl: (control: LogicalControl) => void;
}) {
  const workbenchRef = useRef<HTMLDivElement | null>(null);
  const workbenchSize = useElementSize(workbenchRef);
  const viewportSize = useViewportSize();
  const controlSpecLookup = new Map(
    props.controlSpecs.map((spec) => [spec.control, spec]),
  );
  const minCanvasWidth = props.layout.imageWidth + 220;
  const maxCanvasWidth = props.layout.imageWidth + 520;
  const naturalCanvasWidth = clamp(
    props.layout.imageWidth + 420,
    minCanvasWidth,
    maxCanvasWidth,
  );
  const naturalCanvasHeight = props.layout.imageHeight + 320;
  const horizontalPadding = workbenchSize.width >= 640 ? 64 : 32;
  const availableWidth = Math.max(
    (workbenchSize.width || naturalCanvasWidth) - horizontalPadding,
    280,
  );
  const availableHeight = Math.max(
    (viewportSize.height || naturalCanvasHeight + 220) - 220,
    460,
  );
  const canvasScale = clamp(
    Math.min(
      availableWidth / naturalCanvasWidth,
      availableHeight / naturalCanvasHeight,
      1,
    ),
    0.4,
    1,
  );
  const stageWidth = availableWidth;
  const imageWidth = props.layout.imageWidth * canvasScale;
  const imageHeight = props.layout.imageHeight * canvasScale;
  const imageTop = Math.max(72, 124 * canvasScale);
  const imageLeft = (stageWidth - imageWidth) / 2;
  const stageHeight = Math.max(
    imageTop + imageHeight + Math.max(104, 196 * canvasScale),
    520,
  );

  return (
    <div className="relative" ref={workbenchRef}>
      {props.notes.length > 0 ? (
        <SupportNotesPanel className="mb-6" notes={props.notes} />
      ) : null}
      <div
        className="relative mx-auto min-w-full overflow-hidden px-4 py-6 sm:px-8"
        style={{ minHeight: stageHeight + 48 }}
      >
        <div
          className="relative mx-auto"
          style={{ width: stageWidth, height: stageHeight }}
        >
          <img
            alt={props.layout.label}
            className="absolute object-contain drop-shadow-[0_28px_44px_rgba(15,23,42,0.16)]"
            data-testid="device-layout-image"
            src={props.layout.imageAsset}
            style={{
              height: imageHeight,
              left: imageLeft,
              top: imageTop,
              width: imageWidth,
            }}
          />

          {props.hotspots.map((hotspot) => {
            const isSelected = props.selectedControl === hotspot.control;
            const summary = stageHotspotSummary(
              props.profile,
              hotspot.control,
              props.actionLookup,
              controlSpecLookup,
            );
            const pointX = imageLeft + hotspot.normX * imageWidth;
            const pointY = imageTop + hotspot.normY * imageHeight;
            const cardMetrics = stageCardMetrics(hotspot.control);
            const rawLabelX = pointX + hotspot.labelOffX * canvasScale;
            const rawLabelY = pointY + hotspot.labelOffY * canvasScale;
            const labelX = clamp(
              rawLabelX,
              20,
              stageWidth - cardMetrics.width - 20,
            );
            const labelY = clamp(
              rawLabelY,
              28,
              stageHeight - cardMetrics.height - 28,
            );
            const labelSide = labelX >= pointX ? "right" : "left";
            const connector = connectorStyle(
              pointX,
              pointY,
              labelSide,
              labelX,
              labelY,
              cardMetrics,
            );

            return (
              <div key={hotspot.control}>
                <span
                  className={cn(
                    "pointer-events-none absolute z-[15] h-[2px] origin-left rounded-full transition",
                    isSelected ? "bg-[#89b7ff]" : "bg-[#d2dae8]",
                  )}
                  style={connector}
                />
                <span
                  className={cn(
                    "absolute z-10 h-4 w-4 -translate-x-1/2 -translate-y-1/2 rounded-full border-[3px] bg-[#10131a] transition",
                    isSelected
                      ? "border-[#d7e6ff] bg-accent shadow-[0_0_0_10px_rgba(37,99,235,0.14)]"
                      : "border-white shadow-[0_10px_24px_rgba(15,23,42,0.14)]",
                  )}
                  style={{
                    left: pointX,
                    top: pointY,
                  }}
                  title={hotspot.label}
                />
                <button
                  aria-label={hotspot.label}
                  aria-pressed={isSelected}
                  className={cn(
                    "absolute z-20 rounded-2xl border px-4 py-3 text-left transition-colors transition-shadow",
                    isSelected
                      ? "border-border-strong bg-card shadow-[0_12px_28px_rgba(15,23,42,0.18)]"
                      : "border-border bg-card-muted shadow-[0_8px_22px_rgba(15,23,42,0.12)] hover:border-border-strong hover:bg-card hover:shadow-[0_12px_28px_rgba(15,23,42,0.16)]",
                  )}
                  data-testid={`hotspot-card-${hotspot.control}`}
                  onClick={() => props.onSelectControl(hotspot.control)}
                  style={{
                    left: labelX,
                    minHeight: cardMetrics.height,
                    top: labelY,
                    width: cardMetrics.width,
                  }}
                  type="button"
                >
                  <p className="text-[14px] font-semibold tracking-[-0.02em] text-foreground">
                    {hotspot.label}
                  </p>
                  <p className="mt-2 text-[12px] leading-5 text-muted-foreground">
                    {summary}
                  </p>
                </button>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

function ButtonsMatrixList(props: {
  controls: LogicalControl[];
  deviceName: string;
  notes: string[];
  profile: Profile;
  actionLookup: Map<string, ActionDefinition>;
  controlSpecs: DeviceControlSpec[];
  selectedControl: LogicalControl | null;
  onSelectControl: (control: LogicalControl) => void;
}) {
  const controlSpecLookup = new Map(
    props.controlSpecs.map((spec) => [spec.control, spec]),
  );

  return (
    <div className="space-y-6">
      {props.notes.length > 0 ? <SupportNotesPanel notes={props.notes} /> : null}
      <Panel
        title={`${props.deviceName} Controls`}
        subtitle="This device does not have a dedicated visual overlay yet, so Mouser exposes its validated controls as a generic matrix."
      >
        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
          {props.controls.map((control) => {
            const selected = props.selectedControl === control;
            return (
              <button
                className={cn(
                  "rounded-[24px] border px-5 py-5 text-left transition",
                  selected
                    ? "border-border-strong bg-card shadow-[0_12px_28px_rgba(15,23,42,0.18)]"
                    : "border-border bg-card-muted hover:border-border-strong hover:bg-card hover:shadow-[0_12px_28px_rgba(15,23,42,0.16)]",
                )}
                key={control}
                onClick={() => props.onSelectControl(control)}
                type="button"
              >
                <div className="flex items-center justify-between gap-3">
                  <p className="text-sm font-semibold text-foreground">
                    {editorTitleFor(control)}
                  </p>
                  <StatusPill
                    tone={selected ? "accent" : "neutral"}
                    value={selected ? "Editing" : "Supported"}
                  />
                </div>
                <p className="mt-3 text-sm leading-6 text-muted-foreground">
                  {stageHotspotSummary(
                    props.profile,
                    control,
                    props.actionLookup,
                    controlSpecLookup,
                  )}
                </p>
                <p className="mt-4 text-xs leading-5 text-muted-foreground">
                  {editorDescriptionFor(control)}
                </p>
              </button>
            );
          })}
        </div>
      </Panel>
    </div>
  );
}

function ButtonsControlSheet(props: {
  control: LogicalControl;
  profile: Profile;
  actionLookup: Map<string, ActionDefinition>;
  groupedActions: Array<[string, ActionDefinition[]]>;
  mappingEngineReady: boolean;
  platformCapabilities: BootstrapPayload["platformCapabilities"];
  controlSpecs: DeviceControlSpec[];
  supportedControls: LogicalControl[];
  setBinding: (control: LogicalControl, actionId: string) => void;
  onClose: () => void;
}) {
  const controlSpecLookup = new Map(
    props.controlSpecs.map((spec) => [spec.control, spec]),
  );
  const controls = editorControlsFor(props.control).filter((control) =>
    props.supportedControls.includes(control),
  );
  const visibleControls = controls.length > 0 ? controls : [props.control];
  const title = editorTitleFor(props.control);
  const description = editorDescriptionFor(props.control);
  const gestureControl = props.control.startsWith("gesture_");
  const note =
    gestureControl && !props.platformCapabilities.gestureDiversionAvailable
      ? props.platformCapabilities.platform === "macos" ||
        props.platformCapabilities.platform === "linux"
        ? "Gesture remapping will appear when the Logitech gesture channel connects."
        : "Gesture remapping is unavailable on this platform."
      : !props.mappingEngineReady
        ? "Live remapping is unavailable because the native input hook did not start."
        : null;

  return (
    <section
      className="flex h-full min-h-[520px] flex-col pt-2"
      data-testid="buttons-editor-sheet"
    >
      <div className="flex items-start justify-between gap-4 pb-3">
        <div>
          <h3 className="text-[34px] font-semibold tracking-[-0.05em] text-foreground">
            {title}
          </h3>
          {note ? (
            <p className="mt-3 max-w-sm text-sm leading-7 text-muted-foreground">
              {note}
            </p>
          ) : (
            <p className="mt-3 text-sm leading-7 text-muted-foreground">
              {description}
            </p>
          )}
        </div>

        <Button
          aria-label="Close button editor"
          onClick={props.onClose}
          size="icon"
          variant="ghost"
        >
          <X className="h-4 w-4" />
        </Button>
      </div>

      <Card className="mt-2 bg-card-muted shadow-none ring-1 ring-border">
        <CardContent className="px-5 py-4">
          <p className="text-base leading-8 text-foreground">
            {summarizeHotspot(
              props.profile,
              props.control,
              props.actionLookup,
              controlSpecLookup,
            )}
          </p>
        </CardContent>
      </Card>

      <ScrollArea className="mt-5 flex-1 pr-1">
        <div className="space-y-4">
          {visibleControls.map((control) => (
            <SheetActionField
              actionLookup={props.actionLookup}
              control={control}
              controlSpec={props.controlSpecs.find(
                (spec) => spec.control === control,
              )}
              groupedActions={props.groupedActions}
              key={control}
              label={sheetFieldLabelFor(control)}
              onChange={(actionId) => props.setBinding(control, actionId)}
              profile={props.profile}
            />
          ))}
        </div>
      </ScrollArea>
    </section>
  );
}

function SheetActionField(props: {
  control: LogicalControl;
  label: string;
  profile: Profile;
  actionLookup: Map<string, ActionDefinition>;
  controlSpec?: DeviceControlSpec;
  groupedActions: Array<[string, ActionDefinition[]]>;
  onChange: (actionId: string) => void;
}) {
  const currentBinding = bindingFor(props.profile, props.control);
  const actionOptions = actionOptionsForControl(
    props.controlSpec,
    props.groupedActions,
    props.actionLookup,
  );

  return (
    <Card className="bg-card">
      <CardContent className="p-4">
        <div className="flex items-start justify-between gap-3">
          <p className="text-sm font-semibold text-foreground">
            {props.label}
          </p>
          <Badge variant="default">
            {describeControlAction(
              props.profile,
              props.control,
              props.actionLookup,
              props.controlSpec,
            )}
          </Badge>
        </div>
        <AppSelect
          ariaLabel={props.label}
          className="mt-4"
          onValueChange={props.onChange}
          options={actionOptions}
          value={currentBinding.actionId}
        />
        {factoryDefaultSummary(props.controlSpec) ? (
          <p className="mt-3 text-xs leading-5 text-muted-foreground">
            {factoryDefaultSummary(props.controlSpec)}
          </p>
        ) : null}
      </CardContent>
    </Card>
  );
}

function StatusPill(props: {
  tone: "success" | "accent" | "neutral" | "warning" | "danger";
  value: string;
}) {
  const toneProps: {
    className: string;
    variant: ComponentProps<typeof Badge>["variant"];
  } =
    props.tone === "success"
      ? {
          variant: "secondary",
          className: "border-emerald-200 bg-emerald-50 text-emerald-700",
        }
      : props.tone === "accent"
        ? {
            variant: "secondary",
            className: "border-sky-200 bg-sky-50 text-sky-700",
          }
        : props.tone === "warning"
          ? {
              variant: "secondary",
              className: "border-amber-200 bg-amber-50 text-amber-700",
            }
          : props.tone === "danger"
            ? {
                variant: "secondary",
                className: "border-red-200 bg-red-50 text-red-700",
              }
          : {
              variant: "outline",
              className: "bg-white text-muted-foreground",
            };

  return (
    <Badge className={toneProps.className} variant={toneProps.variant}>
      {props.value}
    </Badge>
  );
}

function CapabilityRow(props: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between rounded-[20px] bg-card-muted px-4 py-3 text-sm ring-1 ring-border">
      <span className="font-medium text-foreground">
        {props.label}
      </span>
      <span className="text-foreground">{props.value}</span>
    </div>
  );
}

function EmptyState(props: { title: string; body: string }) {
  return (
    <div className="rounded-[28px] border border-dashed border-border-strong bg-card-muted p-8 text-center">
      <p className="text-base font-semibold text-foreground">
        {props.title}
      </p>
      <p className="mx-auto mt-3 max-w-lg text-sm leading-6 text-muted-foreground">
        {props.body}
      </p>
    </div>
  );
}

function EmptyStage(props: { title: string; body: string }) {
  return (
    <div className="flex min-h-[520px] items-center justify-center rounded-[32px] border border-dashed border-border-strong bg-card p-8">
      <EmptyState body={props.body} title={props.title} />
    </div>
  );
}

function useElementSize<T extends HTMLElement>(ref: RefObject<T | null>) {
  const [size, setSize] = useState({ width: 0, height: 0 });

  useEffect(() => {
    const node = ref.current;
    if (!node) {
      return;
    }

    const updateSize = () => {
      const rect = node.getBoundingClientRect();
      setSize({ height: rect.height, width: rect.width });
    };

    updateSize();

    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", updateSize);
      return () => window.removeEventListener("resize", updateSize);
    }

    const observer = new ResizeObserver(([entry]) => {
      setSize({
        width: entry.contentRect.width,
        height: entry.contentRect.height,
      });
    });

    observer.observe(node);

    return () => observer.disconnect();
  }, [ref]);

  return size;
}

function useViewportSize() {
  const [size, setSize] = useState(() => ({
    width: typeof window === "undefined" ? 0 : window.innerWidth,
    height: typeof window === "undefined" ? 0 : window.innerHeight,
  }));

  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    const updateSize = () => {
      setSize({ width: window.innerWidth, height: window.innerHeight });
    };

    window.addEventListener("resize", updateSize);
    return () => window.removeEventListener("resize", updateSize);
  }, []);

  return size;
}

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max);
}

function snapDpi(value: number, min: number, max: number) {
  return clamp(Math.round(value / 50) * 50, min, max);
}

function editorControlsFor(control: LogicalControl) {
  if (control === "gesture_press") {
    return [
      "gesture_press",
      "gesture_up",
      "gesture_down",
      "gesture_left",
      "gesture_right",
    ] satisfies LogicalControl[];
  }

  if (control === "hscroll_left") {
    return ["hscroll_left", "hscroll_right"] satisfies LogicalControl[];
  }

  return [control];
}

function stageCardMetrics(control: LogicalControl) {
  if (control === "gesture_press") {
    return { width: 196, height: 98 };
  }

  if (control === "hscroll_left") {
    return { width: 182, height: 94 };
  }

  return { width: 170, height: 80 };
}

function connectorStyle(
  pointX: number,
  pointY: number,
  labelSide: "left" | "right",
  labelX: number,
  labelY: number,
  cardMetrics: { width: number; height: number },
) {
  const anchorX = labelSide === "right" ? labelX : labelX + cardMetrics.width;
  const anchorY = labelY + cardMetrics.height / 2;
  const dx = anchorX - pointX;
  const dy = anchorY - pointY;
  const angle = Math.atan2(dy, dx);
  const inset = 12;
  const startX = pointX + Math.cos(angle) * inset;
  const startY = pointY + Math.sin(angle) * inset;
  const length = Math.max(Math.hypot(dx, dy) - inset, 0);

  return {
    left: startX,
    top: startY,
    width: length,
    transform: `translateY(-50%) rotate(${(angle * 180) / Math.PI}deg)`,
  };
}

function editorTitleFor(control: LogicalControl) {
  if (control === "gesture_press") {
    return "Gesture button";
  }

  if (control === "hscroll_left") {
    return "Thumb wheel";
  }

  return CONTROL_LABELS[control];
}

function editorDescriptionFor(control: LogicalControl) {
  if (control === "gesture_press") {
    return "Press, hold, and directional swipe actions stay grouped here so the gesture button behaves like one coherent control.";
  }

  if (control === "hscroll_left") {
    return "Horizontal scroll is split into left and right actions. Keep both directions together so the thumb wheel stays predictable.";
  }

  if (
    control === "smartshift_toggle" ||
    control === "mission_control_button" ||
    control === "smart_zoom_button" ||
    control === "precision_mode_button" ||
    control === "dpi_button" ||
    control === "emoji_button"
  ) {
    return "This Logitech-specific control comes from the imported device catalog. Recommended Logitech actions are shown first, and Logitech-only actions Mouser cannot execute stay visible but disabled.";
  }

  return "Adjust the action for this control on the active profile. Changes save immediately and apply to the currently selected app profile.";
}

function stageHotspotSummary(
  profile: Profile,
  control: LogicalControl,
  actionLookup: Map<string, ActionDefinition>,
  controlSpecLookup: Map<LogicalControl, DeviceControlSpec>,
) {
  if (control === "hscroll_left") {
    const left = compactStageActionLabel(
      describeControlAction(
        profile,
        "hscroll_left",
        actionLookup,
        controlSpecLookup.get("hscroll_left"),
      ),
    );
    const right = compactStageActionLabel(
      describeControlAction(
        profile,
        "hscroll_right",
        actionLookup,
        controlSpecLookup.get("hscroll_right"),
      ),
    );
    return `L ${left} / R ${right}`;
  }

  if (control === "gesture_press") {
    const tap = compactStageActionLabel(
      describeControlAction(
        profile,
        "gesture_press",
        actionLookup,
        controlSpecLookup.get("gesture_press"),
      ),
    );
    const swipeConfigured = [
      "gesture_left",
      "gesture_right",
      "gesture_up",
      "gesture_down",
    ].some((gestureControl) =>
      hasConfiguredOrFactoryDefaultAction(
        profile,
        gestureControl as LogicalControl,
        controlSpecLookup.get(gestureControl as LogicalControl),
      ),
    );
    return swipeConfigured ? `Tap ${tap} + swipes` : `Tap ${tap}`;
  }

  return compactStageActionLabel(
    describeControlAction(
      profile,
      control,
      actionLookup,
      controlSpecLookup.get(control),
    ),
  );
}

function compactStageActionLabel(label: string) {
  const normalized =
    label === "Do Nothing (Pass-through)"
      ? "No action"
      : label.replace(/\s*\([^)]*\)\s*/g, "").trim();

  return normalized.length > 24 ? `${normalized.slice(0, 22)}...` : normalized;
}

function sheetFieldLabelFor(control: LogicalControl) {
  switch (control) {
    case "gesture_press":
      return "Press action";
    case "gesture_left":
      return "Swipe left";
    case "gesture_right":
      return "Swipe right";
    case "gesture_up":
      return "Swipe up";
    case "gesture_down":
      return "Swipe down";
    case "hscroll_left":
      return "Scroll left";
    case "hscroll_right":
      return "Scroll right";
    default:
      return CONTROL_LABELS[control];
  }
}

function summarizeHotspot(
  profile: Profile,
  control: LogicalControl,
  actionLookup: Map<string, ActionDefinition>,
  controlSpecLookup?: Map<LogicalControl, DeviceControlSpec>,
) {
  if (control === "hscroll_left") {
    const left = describeControlAction(
      profile,
      "hscroll_left",
      actionLookup,
      controlSpecLookup?.get("hscroll_left"),
    );
    const right = describeControlAction(
      profile,
      "hscroll_right",
      actionLookup,
      controlSpecLookup?.get("hscroll_right"),
    );
    return `Left: ${left} | Right: ${right}`;
  }

  if (control === "gesture_press") {
    const tap = describeControlAction(
      profile,
      "gesture_press",
      actionLookup,
      controlSpecLookup?.get("gesture_press"),
    );
    const swipeConfigured = [
      "gesture_left",
      "gesture_right",
      "gesture_up",
      "gesture_down",
    ].some((gestureControl) =>
      hasConfiguredOrFactoryDefaultAction(
        profile,
        gestureControl as LogicalControl,
        controlSpecLookup?.get(gestureControl as LogicalControl),
      ),
    );
    return swipeConfigured ? `Tap: ${tap} | Swipes configured` : `Tap: ${tap}`;
  }

  return describeControlAction(
    profile,
    control,
    actionLookup,
    controlSpecLookup?.get(control),
  );
}

function actionFor(
  profile: Profile,
  control: LogicalControl,
  actionLookup: Map<string, ActionDefinition>,
) {
  const actionId = bindingFor(profile, control).actionId;
  return actionLookup.get(actionId)?.label ?? "Do Nothing (Pass-through)";
}

function factoryDefaultSummary(controlSpec?: DeviceControlSpec) {
  const label = controlSpec?.factoryDefaultLabel?.trim();
  if (!label) {
    return null;
  }

  const source = controlSpec?.factoryDefaultSource;

  return `${source === "recommendation" ? "Suggested default" : "Factory default"}: ${label}`;
}

function describeControlAction(
  profile: Profile,
  control: LogicalControl,
  actionLookup: Map<string, ActionDefinition>,
  controlSpec?: DeviceControlSpec,
) {
  const currentAction = actionFor(profile, control, actionLookup);
  if (currentAction !== "Do Nothing (Pass-through)") {
    return currentAction;
  }

  return factoryDefaultSummary(controlSpec) ?? currentAction;
}

function hasConfiguredOrFactoryDefaultAction(
  profile: Profile,
  control: LogicalControl,
  controlSpec?: DeviceControlSpec,
) {
  return (
    bindingFor(profile, control).actionId !== "none" ||
    Boolean(controlSpec?.factoryDefaultLabel)
  );
}

function bindingFor(profile: Profile, control: LogicalControl): Binding {
  return (
    profile.bindings.find((binding) => binding.control === control) ??
    ({ control, actionId: "none" } satisfies Binding)
  );
}

function upsertBinding(
  profile: Profile,
  control: LogicalControl,
  actionId: string,
) {
  const target = profile.bindings.find(
    (binding) => binding.control === control,
  );
  if (target) {
    target.actionId = actionId;
    return;
  }

  profile.bindings.push({ control, actionId });
}

function cloneProfile(profile: Profile): Profile {
  return {
    ...profile,
    appMatchers: profile.appMatchers.map((matcher) => ({ ...matcher })),
    bindings: profile.bindings.map((binding) => ({ ...binding })),
  };
}

function cloneConfig(config: AppConfig): AppConfig {
  return {
    ...config,
    settings: {
      ...config.settings,
      debugLogGroups: {
        ...DEFAULT_DEBUG_LOG_GROUPS,
        ...(config.settings.debugLogGroups ?? {}),
      },
    },
    deviceDefaults: normalizeDeviceSettings(config.deviceDefaults),
    profiles: config.profiles.map(cloneProfile),
    managedDevices: (config.managedDevices ?? []).map((device) => ({
      ...device,
      settings: normalizeDeviceSettings(device.settings),
    })),
  };
}

function makeProfileId(label: string, config: AppConfig) {
  const base =
    label
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "_")
      .replace(/^_+|_+$/g, "") || "profile";
  let candidate = base;
  let suffix = 2;
  const ids = new Set(config.profiles.map((profile) => profile.id));
  while (ids.has(candidate)) {
    candidate = `${base}_${suffix}`;
    suffix += 1;
  }
  return candidate;
}

function resolveActiveLayout(
  device: DeviceInfo | null,
  config: AppConfig,
  layouts: DeviceLayout[],
) {
  if (!device) {
    return (
      layouts.find((layout) => layout.key === "generic_mouse") ?? layouts[0]
    );
  }

  const overrideKey = normalizeDeviceSettings(
    findManagedDevice(config, device.key)?.settings,
  ).manualLayoutOverride;
  const targetKey = overrideKey || device.uiLayout;
  return layouts.find((layout) => layout.key === targetKey) ?? layouts[0];
}

function groupActions(actions: ActionDefinition[]) {
  const groups = new Map<string, ActionDefinition[]>();
  for (const action of actions) {
    const next = groups.get(action.category) ?? [];
    next.push(action);
    groups.set(action.category, next);
  }
  return [...groups.entries()];
}

function actionOptionsForControl(
  controlSpec: DeviceControlSpec | undefined,
  groupedActions: Array<[string, ActionDefinition[]]>,
  actionLookup: Map<string, ActionDefinition>,
) {
  const options: AppSelectOption[] = [];
  const seen = new Set<string>();

  for (const actionId of controlSpec?.recommendedActionIds ?? []) {
    const action = actionLookup.get(actionId);
    if (!action) {
      continue;
    }

    seen.add(action.id);
    options.push({
      group: "Recommended",
      label: action.label,
      value: action.id,
      disabled: !action.supported,
      badge: action.supported ? "Recommended" : "Unsupported",
    });
  }

  for (const [group, actions] of groupedActions) {
    for (const action of actions) {
      if (seen.has(action.id)) {
        continue;
      }

      const unsupported = action.supported === false;
      options.push({
        group: unsupported ? `${group} (Unsupported)` : group,
        label: action.label,
        value: action.id,
        disabled: unsupported,
        badge: unsupported ? "Unsupported" : undefined,
      });
    }
  }

  return options;
}

function buildImportRequest(
  sourcePath: string,
  rawJson: string,
): ImportLegacyRequest {
  const trimmedSourcePath = sourcePath.trim();
  return {
    sourcePath: trimmedSourcePath || null,
    rawJson: trimmedSourcePath ? null : rawJson,
  };
}

function currentGreeting() {
  const hour = new Date().getHours();
  if (hour < 12) {
    return "Good Morning";
  }
  if (hour < 18) {
    return "Good Afternoon";
  }
  return "Good Evening";
}

export default App;
