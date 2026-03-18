import type {
  ActionDefinition as BindActionDefinition,
  AppMatcher as BindAppMatcher,
  AppMatcherKind as BindAppMatcherKind,
  AppearanceMode as BindAppearanceMode,
  Binding as BindBinding,
  BootstrapPayload as BindBootstrapPayload,
  DebugEvent as BindDebugEvent,
  DebugEventKind as BindDebugEventKind,
  DeviceFingerprint as BindDeviceFingerprint,
  DeviceHotspot as BindDeviceHotspot,
  DeviceInfo as BindDeviceInfo,
  DeviceLayout as BindDeviceLayout,
  DeviceSettings as BindDeviceSettings,
  EngineSnapshot as BindEngineSnapshot,
  EngineStatus as BindEngineStatus,
  HotspotSummaryType as BindHotspotSummaryType,
  ImportLegacyConfigRequest as BindImportLegacyConfigRequest,
  KnownApp as BindKnownApp,
  KnownDeviceSpec as BindKnownDeviceSpec,
  LabelSide as BindLabelSide,
  LayoutChoice as BindLayoutChoice,
  LegacyImportReport as BindLegacyImportReport,
  LogicalControl as BindLogicalControl,
  ManagedDevice as BindManagedDevice,
  PlatformCapabilities as BindPlatformCapabilities,
  Profile as BindProfile,
  Settings as BindSettings,
  AppConfig as BindAppConfig,
} from "./bindings";

export type ActionDefinition = BindActionDefinition;
export type AppMatcher = BindAppMatcher;
export type AppMatcherKind = BindAppMatcherKind;
export type AppearanceMode = BindAppearanceMode;
export type Binding = BindBinding;
export type DebugEventRecord = BindDebugEvent;
export type DebugEventKind = BindDebugEventKind;
export type DeviceFingerprint = BindDeviceFingerprint;
export type DeviceHotspot = BindDeviceHotspot;
export type DeviceInfo = BindDeviceInfo;
export type DeviceLayout = BindDeviceLayout;
export type EngineSnapshot = BindEngineSnapshot;
export type EngineStatus = BindEngineStatus;
export type HotspotSummaryType = BindHotspotSummaryType;
export type ImportLegacyRequest = BindImportLegacyConfigRequest;
export type KnownApp = BindKnownApp;
export type KnownDeviceSpec = BindKnownDeviceSpec;
export type LabelSide = BindLabelSide;
export type LayoutChoice = BindLayoutChoice;
export type LogicalControl = BindLogicalControl;
export type PlatformCapabilities = BindPlatformCapabilities;
export type Profile = BindProfile;
export type Settings = BindSettings;

export type DeviceSettings = BindDeviceSettings & {
  macosThumbWheelSimulateTrackpad?: boolean;
  macosThumbWheelTrackpadHoldTimeoutMs?: number;
};

export type ManagedDevice = Omit<BindManagedDevice, "settings"> & {
  settings?: DeviceSettings;
};

export type AppConfig = Omit<
  BindAppConfig,
  "managedDevices" | "deviceDefaults"
> & {
  managedDevices?: ManagedDevice[];
  deviceDefaults?: DeviceSettings;
};

export type BootstrapPayload = Omit<BindBootstrapPayload, "config"> & {
  config: AppConfig;
};

export type LegacyImportReport = Omit<BindLegacyImportReport, "config"> & {
  config: AppConfig;
};
