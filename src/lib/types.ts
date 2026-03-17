export type AppearanceMode = "system" | "light" | "dark";
export type AppMatcherKind = "executable";
export type DebugEventKind = "info" | "warning" | "gesture";
export type HotspotSummaryType = "mapping" | "gesture" | "hscroll";
export type LabelSide = "left" | "right";
export type LogicalControl =
  | "middle"
  | "gesture_press"
  | "gesture_left"
  | "gesture_right"
  | "gesture_up"
  | "gesture_down"
  | "back"
  | "forward"
  | "hscroll_left"
  | "hscroll_right";

export interface Binding {
  control: LogicalControl;
  actionId: string;
}

export interface AppMatcher {
  kind: AppMatcherKind;
  value: string;
}

export interface Profile {
  id: string;
  label: string;
  appMatchers: AppMatcher[];
  bindings: Binding[];
}

export interface Settings {
  startMinimized: boolean;
  startAtLogin: boolean;
  invertHorizontalScroll: boolean;
  invertVerticalScroll: boolean;
  dpi: number;
  gestureThreshold: number;
  gestureDeadzone: number;
  gestureTimeoutMs: number;
  gestureCooldownMs: number;
  appearanceMode: AppearanceMode;
  debugMode: boolean;
  deviceLayoutOverrides: Record<string, string>;
}

export interface AppConfig {
  version: number;
  activeProfileId: string;
  profiles: Profile[];
  settings: Settings;
}

export interface ActionDefinition {
  id: string;
  label: string;
  category: string;
}

export interface DeviceHotspot {
  control: LogicalControl;
  label: string;
  summaryType: HotspotSummaryType;
  normX: number;
  normY: number;
  labelSide: LabelSide;
  labelOffX: number;
  labelOffY: number;
  isHscroll: boolean;
}

export interface DeviceLayout {
  key: string;
  label: string;
  imageAsset: string;
  imageWidth: number;
  imageHeight: number;
  interactive: boolean;
  manualSelectable: boolean;
  note: string;
  hotspots: DeviceHotspot[];
}

export interface DeviceInfo {
  key: string;
  displayName: string;
  productId: number | null;
  productName: string | null;
  transport: string | null;
  source: string | null;
  uiLayout: string;
  imageAsset: string;
  supportedControls: LogicalControl[];
  gestureCids: number[];
  dpiMin: number;
  dpiMax: number;
  connected: boolean;
  batteryLevel: number | null;
  currentDpi: number;
}

export interface DebugEventRecord {
  kind: DebugEventKind;
  message: string;
  timestampMs: number;
}

export interface EngineStatus {
  enabled: boolean;
  connected: boolean;
  activeProfileId: string;
  frontmostApp: string | null;
  selectedDeviceKey: string | null;
  debugMode: boolean;
  debugLog: DebugEventRecord[];
}

export interface EngineSnapshot {
  devices: DeviceInfo[];
  activeDeviceKey: string | null;
  activeDevice: DeviceInfo | null;
  engineStatus: EngineStatus;
}

export interface PlatformCapabilities {
  platform: string;
  windowsSupported: boolean;
  macosSupported: boolean;
  liveHooksAvailable: boolean;
  liveHidAvailable: boolean;
  trayReady: boolean;
}

export interface LayoutChoice {
  key: string;
  label: string;
}

export interface BootstrapPayload {
  config: AppConfig;
  availableActions: ActionDefinition[];
  layouts: DeviceLayout[];
  engineSnapshot: EngineSnapshot;
  platformCapabilities: PlatformCapabilities;
  manualLayoutChoices: LayoutChoice[];
}

export interface LegacyImportReport {
  config: AppConfig;
  warnings: string[];
  sourcePath: string | null;
  importedProfiles: number;
}

export interface ImportLegacyRequest {
  sourcePath?: string | null;
  rawJson?: string | null;
}
