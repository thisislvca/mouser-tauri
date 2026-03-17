import { useEffect, useRef, useState, type ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import "./App.css";
import {
  bootstrapLoad,
  configSave,
  devicesSelectMock,
  importLegacyConfig,
  profilesCreate,
  profilesDelete,
  profilesUpdate,
} from "./lib/api";
import { sampleLegacyConfig } from "./lib/sampleLegacyConfig";
import type {
  AppConfig,
  Binding,
  BootstrapPayload,
  DeviceInfo,
  DeviceLayout,
  LogicalControl,
  Profile,
} from "./lib/types";
import { useRuntimeEvents } from "./hooks/useRuntimeEvents";
import { useUiStore } from "./store/uiStore";

const CONTROL_ORDER: LogicalControl[] = [
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
];

const CONTROL_LABELS: Record<LogicalControl, string> = {
  middle: "Middle button",
  gesture_press: "Gesture button",
  gesture_left: "Gesture swipe left",
  gesture_right: "Gesture swipe right",
  gesture_up: "Gesture swipe up",
  gesture_down: "Gesture swipe down",
  back: "Back button",
  forward: "Forward button",
  hscroll_left: "Horizontal scroll left",
  hscroll_right: "Horizontal scroll right",
};

function App() {
  const queryClient = useQueryClient();
  useRuntimeEvents();

  const activeSection = useUiStore((state) => state.activeSection);
  const setActiveSection = useUiStore((state) => state.setActiveSection);
  const selectedProfileId = useUiStore((state) => state.selectedProfileId);
  const setSelectedProfileId = useUiStore((state) => state.setSelectedProfileId);
  const importDraft = useUiStore((state) => state.importDraft);
  const setImportDraft = useUiStore((state) => state.setImportDraft);
  const eventLog = useUiStore((state) => state.eventLog);
  const hydrateDebugLog = useUiStore((state) => state.hydrateDebugLog);

  const [newProfileLabel, setNewProfileLabel] = useState("");
  const [newProfileApp, setNewProfileApp] = useState("");
  const [importWarnings, setImportWarnings] = useState<string[]>([]);
  const [importSourcePath, setImportSourcePath] = useState("");
  const lastActiveProfileIdRef = useRef<string | null>(null);

  const bootstrapQuery = useQuery({
    queryKey: ["bootstrap"],
    queryFn: bootstrapLoad,
  });

  const invalidateBootstrap = () =>
    queryClient.invalidateQueries({ queryKey: ["bootstrap"] });

  const configMutation = useMutation({
    mutationFn: configSave,
    onSuccess: invalidateBootstrap,
  });
  const createProfileMutation = useMutation({
    mutationFn: profilesCreate,
    onSuccess: invalidateBootstrap,
  });
  const updateProfileMutation = useMutation({
    mutationFn: profilesUpdate,
    onSuccess: invalidateBootstrap,
  });
  const deleteProfileMutation = useMutation({
    mutationFn: profilesDelete,
    onSuccess: invalidateBootstrap,
  });
  const selectDeviceMutation = useMutation({
    mutationFn: devicesSelectMock,
    onSuccess: invalidateBootstrap,
  });
  const importMutation = useMutation({
    mutationFn: importLegacyConfig,
    onSuccess: (report) => {
      setImportWarnings(report.warnings);
      setSelectedProfileId(report.config.activeProfileId);
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

    hydrateDebugLog(bootstrapQuery.data.engineSnapshot.engineStatus.debugLog);

    const profileIds = new Set(bootstrapQuery.data.config.profiles.map((profile) => profile.id));
    const activeProfileId = bootstrapQuery.data.config.activeProfileId;
    const activeProfileChanged =
      lastActiveProfileIdRef.current != null && lastActiveProfileIdRef.current !== activeProfileId;

    if (!selectedProfileId || !profileIds.has(selectedProfileId) || activeProfileChanged) {
      setSelectedProfileId(activeProfileId);
    }

    lastActiveProfileIdRef.current = activeProfileId;
  }, [
    bootstrapQuery.data,
    hydrateDebugLog,
    selectedProfileId,
    setSelectedProfileId,
  ]);

  const bootstrap = bootstrapQuery.data;
  const isMutating =
    configMutation.isPending ||
    createProfileMutation.isPending ||
    updateProfileMutation.isPending ||
    deleteProfileMutation.isPending ||
    selectDeviceMutation.isPending ||
    importMutation.isPending;

  if (bootstrapQuery.isLoading) {
    return <main className="shell loading">Loading Mouser Tauri...</main>;
  }

  if (bootstrapQuery.isError || !bootstrap) {
    return (
      <main className="shell loading">
        Failed to load Mouser Tauri.
        <pre>{String(bootstrapQuery.error)}</pre>
      </main>
    );
  }

  const { config, availableActions, engineSnapshot, layouts, platformCapabilities } = bootstrap;
  const selectedProfile =
    config.profiles.find((profile) => profile.id === selectedProfileId) ??
    config.profiles.find((profile) => profile.id === config.activeProfileId) ??
    config.profiles[0];
  const activeDevice = engineSnapshot.activeDevice;
  const activeLayout = resolveActiveLayout(activeDevice, config, layouts);
  const groupedActions = groupActions(availableActions);

  const updateSelectedProfile = (mutateProfile: (profile: Profile) => void) => {
    const nextProfile = cloneProfile(selectedProfile);
    mutateProfile(nextProfile);
    updateProfileMutation.mutate(nextProfile);
  };

  const saveSettings = (mutateConfig: (nextConfig: AppConfig) => void) => {
    const nextConfig = cloneConfig(config);
    mutateConfig(nextConfig);
    configMutation.mutate(nextConfig);
  };

  return (
    <main className="shell">
      <aside className="sidebar">
        <div className="brand">
          <img alt="Mouser logo" className="brand-mark" src="/assets/logo_icon.png" />
          <div>
            <p className="eyebrow">Clean-room rewrite</p>
            <h1>Mouser Tauri</h1>
          </div>
        </div>

        <div className="status-card">
          <span className={`status-dot ${engineSnapshot.engineStatus.connected ? "live" : ""}`} />
          <div>
            <p>{activeDevice?.displayName ?? "No device selected"}</p>
            <strong>
              {engineSnapshot.engineStatus.connected ? "Mock device connected" : "Disconnected"}
            </strong>
          </div>
        </div>

        <nav className="section-nav">
          {[
            ["devices", "Devices"],
            ["buttons", "Buttons"],
            ["profiles", "Profiles"],
            ["settings", "Settings"],
            ["debug", "Debug"],
          ].map(([key, label]) => (
            <button
              key={key}
              className={key === activeSection ? "nav-button active" : "nav-button"}
              onClick={() => setActiveSection(key as typeof activeSection)}
              type="button"
            >
              {label}
            </button>
          ))}
        </nav>

        <div className="sidebar-footer">
          <p>Runtime</p>
          <strong>{platformCapabilities.platform}</strong>
          <span>{platformCapabilities.trayReady ? "Tray-ready shell" : "Tray pending"}</span>
        </div>
      </aside>

      <section className="content">
        <header className="hero">
          <div>
            <p className="eyebrow">Milestone 1 shell</p>
            <h2>Mock-backed settings surface for the future HID engine</h2>
            <p className="hero-copy">
              This repo now has a typed Tauri command layer, a new config model,
              a legacy importer, platform traits, and a React shell that renders the
              current Mouser use case without shipping live hook or HID logic yet.
            </p>
          </div>
          <div className="hero-metrics">
            <Metric label="Active profile" value={config.activeProfileId} />
            <Metric label="Frontmost app" value={engineSnapshot.engineStatus.frontmostApp ?? "None"} />
            <Metric
              label="Battery"
              value={activeDevice?.batteryLevel != null ? `${activeDevice.batteryLevel}%` : "N/A"}
            />
          </div>
        </header>

        {activeSection === "devices" && (
          <section className="panel-grid">
            <Panel
              title="Mock device roster"
              subtitle="Switch the active mock device to exercise layouts, DPI bounds, and state propagation."
            >
              <div className="device-list">
                {engineSnapshot.devices.map((device) => (
                  <button
                    key={device.key}
                    className={device.key === engineSnapshot.activeDeviceKey ? "device-chip active" : "device-chip"}
                    onClick={() => selectDeviceMutation.mutate(device.key)}
                    type="button"
                  >
                    <span>{device.displayName}</span>
                    <small>{device.transport ?? "Unknown transport"}</small>
                  </button>
                ))}
              </div>
            </Panel>

            <Panel
              title="Active layout"
              subtitle="The shell reuses Mouser layout metadata and applies manual overrides from the new config schema."
            >
              {activeDevice && activeLayout ? (
                <div className="layout-card" data-testid="device-layout-card">
                  <div
                    className="layout-stage"
                    style={{ width: activeLayout.imageWidth, height: activeLayout.imageHeight }}
                  >
                    <img
                      alt={activeLayout.label}
                      className="layout-image"
                      data-testid="device-layout-image"
                      src={activeLayout.imageAsset}
                    />
                    {activeLayout.hotspots.map((hotspot) => (
                      <div
                        key={hotspot.control}
                        className="hotspot"
                        style={{
                          left: `${hotspot.normX * 100}%`,
                          top: `${hotspot.normY * 100}%`,
                        }}
                        title={hotspot.label}
                      />
                    ))}
                  </div>
                  <div className="layout-meta">
                    <strong>{activeLayout.label}</strong>
                    <p>{activeLayout.note || "Interactive overlay active for the current device."}</p>
                    <label className="field">
                      <span>Manual layout override</span>
                      <select
                        value={config.settings.deviceLayoutOverrides[activeDevice.key] ?? ""}
                        onChange={(event) =>
                          saveSettings((nextConfig) => {
                            if (event.currentTarget.value) {
                              nextConfig.settings.deviceLayoutOverrides[activeDevice.key] =
                                event.currentTarget.value;
                            } else {
                              delete nextConfig.settings.deviceLayoutOverrides[activeDevice.key];
                            }
                          })
                        }
                      >
                        {bootstrap.manualLayoutChoices.map((choice) => (
                          <option key={choice.key || "auto"} value={choice.key}>
                            {choice.label}
                          </option>
                        ))}
                      </select>
                    </label>
                  </div>
                </div>
              ) : (
                <p>No device selected.</p>
              )}
            </Panel>
          </section>
        )}

        {activeSection === "buttons" && (
          <section className="panel-grid single">
            <Panel
              title="Button and gesture bindings"
              subtitle="Bindings are stored on the selected profile in the new Rust config model."
            >
              <div className="binding-table">
                {CONTROL_ORDER.map((control) => {
                  const currentBinding =
                    selectedProfile.bindings.find((binding) => binding.control === control) ??
                    ({
                      control,
                      actionId: "none",
                    } satisfies Binding);
                  return (
                    <div className="binding-row" key={control}>
                      <div>
                        <strong>{CONTROL_LABELS[control]}</strong>
                        <p>{control}</p>
                      </div>
                      <select
                        value={currentBinding.actionId}
                        onChange={(event) =>
                          updateSelectedProfile((nextProfile) => {
                            const target = nextProfile.bindings.find(
                              (binding) => binding.control === control,
                            );
                            if (target) {
                              target.actionId = event.currentTarget.value;
                            }
                          })
                        }
                      >
                        {groupedActions.map(([category, actions]) => (
                          <optgroup key={category} label={category}>
                            {actions.map((action) => (
                              <option key={action.id} value={action.id}>
                                {action.label}
                              </option>
                            ))}
                          </optgroup>
                        ))}
                      </select>
                    </div>
                  );
                })}
              </div>
            </Panel>
          </section>
        )}

        {activeSection === "profiles" && (
          <section className="panel-grid">
            <Panel
              title="Profiles"
              subtitle="Profiles drive per-application remapping and mirror the target Mouser use case."
            >
              <div className="profile-list">
                {config.profiles.map((profile) => (
                  <button
                    key={profile.id}
                    className={profile.id === selectedProfile.id ? "profile-chip active" : "profile-chip"}
                    onClick={() => setSelectedProfileId(profile.id)}
                    type="button"
                  >
                    <span>{profile.label}</span>
                    <small>{profile.appMatchers.map((matcher) => matcher.value).join(", ") || "All apps"}</small>
                  </button>
                ))}
              </div>
              <div className="create-profile">
                <input
                  placeholder="New profile label"
                  value={newProfileLabel}
                  onChange={(event) => setNewProfileLabel(event.currentTarget.value)}
                />
                <input
                  placeholder="Optional executable (e.g. Code.exe)"
                  value={newProfileApp}
                  onChange={(event) => setNewProfileApp(event.currentTarget.value)}
                />
                <button
                  onClick={() => {
                    const label = newProfileLabel.trim();
                    if (!label) {
                      return;
                    }
                    const id = makeProfileId(label, config);
                    createProfileMutation.mutate({
                      id,
                      label,
                      appMatchers: newProfileApp.trim()
                        ? [{ kind: "executable", value: newProfileApp.trim() }]
                        : [],
                      bindings: selectedProfile.bindings.map((binding) => ({ ...binding })),
                    });
                    setNewProfileLabel("");
                    setNewProfileApp("");
                    setSelectedProfileId(id);
                  }}
                  type="button"
                >
                  Create profile
                </button>
              </div>
            </Panel>

            <Panel
              title="Selected profile"
              subtitle="Editing this card calls the explicit profile commands instead of saving the whole config."
            >
              <label className="field">
                <span>Label</span>
                <input
                  data-testid="profile-label-input"
                  value={selectedProfile.label}
                  onChange={(event) =>
                    updateSelectedProfile((nextProfile) => {
                      nextProfile.label = event.currentTarget.value;
                    })
                  }
                />
              </label>
              <label className="field">
                <span>Executable matchers</span>
                <textarea
                  rows={4}
                  value={selectedProfile.appMatchers.map((matcher) => matcher.value).join("\n")}
                  onChange={(event) =>
                    updateSelectedProfile((nextProfile) => {
                      nextProfile.appMatchers = event.currentTarget.value
                        .split("\n")
                        .map((value) => value.trim())
                        .filter(Boolean)
                        .map((value) => ({ kind: "executable", value }));
                    })
                  }
                />
              </label>
              <div className="profile-actions">
                <span className="pill" data-testid="profile-label-display">
                  {selectedProfile.label}
                </span>
                <button
                  className="danger"
                  disabled={selectedProfile.id === "default"}
                  onClick={() => deleteProfileMutation.mutate(selectedProfile.id)}
                  type="button"
                >
                  Delete profile
                </button>
              </div>
            </Panel>
          </section>
        )}

        {activeSection === "settings" && (
          <section className="panel-grid">
            <Panel
              title="General settings"
              subtitle="These settings live on the new AppConfig schema and persist through config_save."
            >
              <div className="settings-grid">
                <ToggleField
                  label="Start minimized"
                  checked={config.settings.startMinimized}
                  onChange={(value) =>
                    saveSettings((nextConfig) => {
                      nextConfig.settings.startMinimized = value;
                    })
                  }
                />
                <ToggleField
                  label="Start at login"
                  checked={config.settings.startAtLogin}
                  onChange={(value) =>
                    saveSettings((nextConfig) => {
                      nextConfig.settings.startAtLogin = value;
                    })
                  }
                />
                <ToggleField
                  label="Invert horizontal scroll"
                  checked={config.settings.invertHorizontalScroll}
                  onChange={(value) =>
                    saveSettings((nextConfig) => {
                      nextConfig.settings.invertHorizontalScroll = value;
                    })
                  }
                />
                <ToggleField
                  label="Invert vertical scroll"
                  checked={config.settings.invertVerticalScroll}
                  onChange={(value) =>
                    saveSettings((nextConfig) => {
                      nextConfig.settings.invertVerticalScroll = value;
                    })
                  }
                />
                <ToggleField
                  label="Debug mode"
                  checked={config.settings.debugMode}
                  onChange={(value) =>
                    saveSettings((nextConfig) => {
                      nextConfig.settings.debugMode = value;
                    })
                  }
                />
                <label className="field">
                  <span>DPI</span>
                  <input
                    data-testid="dpi-input"
                    max={activeDevice?.dpiMax ?? 8000}
                    min={activeDevice?.dpiMin ?? 200}
                    type="number"
                    value={config.settings.dpi}
                    onChange={(event) =>
                      saveSettings((nextConfig) => {
                        nextConfig.settings.dpi = Number(event.currentTarget.value);
                      })
                    }
                  />
                </label>
                <label className="field">
                  <span>Gesture threshold</span>
                  <input
                    type="number"
                    value={config.settings.gestureThreshold}
                    onChange={(event) =>
                      saveSettings((nextConfig) => {
                        nextConfig.settings.gestureThreshold = Number(event.currentTarget.value);
                      })
                    }
                  />
                </label>
                <label className="field">
                  <span>Appearance mode</span>
                  <select
                    value={config.settings.appearanceMode}
                    onChange={(event) =>
                      saveSettings((nextConfig) => {
                        nextConfig.settings.appearanceMode = event.currentTarget.value as AppConfig["settings"]["appearanceMode"];
                      })
                    }
                  >
                    <option value="system">System</option>
                    <option value="light">Light</option>
                    <option value="dark">Dark</option>
                  </select>
                </label>
              </div>
            </Panel>

            <Panel
              title="Capability snapshot"
              subtitle="Live hook and HID integrations remain intentionally stubbed in this milestone."
            >
              <ul className="capability-list">
                <li>Windows target: {platformCapabilities.windowsSupported ? "planned" : "no"}</li>
                <li>macOS target: {platformCapabilities.macosSupported ? "planned" : "no"}</li>
                <li>Live hooks: {platformCapabilities.liveHooksAvailable ? "ready" : "stubbed"}</li>
                <li>Live HID: {platformCapabilities.liveHidAvailable ? "ready" : "stubbed"}</li>
                <li>Tray shell: {platformCapabilities.trayReady ? "wired" : "pending"}</li>
              </ul>
            </Panel>
          </section>
        )}

        {activeSection === "debug" && (
          <section className="panel-grid">
            <Panel
              title="Legacy importer"
              subtitle="Paste an old Mouser config.json payload to exercise the clean importer and hydrate the new runtime."
            >
              <label className="field">
                <span>Optional source path</span>
                <input
                  placeholder="~/Library/Application Support/Mouser/config.json"
                  value={importSourcePath}
                  onChange={(event) => setImportSourcePath(event.currentTarget.value)}
                />
              </label>
              <label className="field">
                <span>Legacy Mouser JSON</span>
                <textarea
                  data-testid="legacy-import-input"
                  rows={14}
                  value={importDraft}
                  onChange={(event) => setImportDraft(event.currentTarget.value)}
                />
              </label>
              <div className="profile-actions">
                <button
                  data-testid="legacy-import-button"
                  onClick={() =>
                    importMutation.mutate({
                      sourcePath: importSourcePath.trim() || null,
                      rawJson: importDraft,
                    })
                  }
                  type="button"
                >
                  Import legacy config
                </button>
                <span>{importWarnings.length} warning(s)</span>
              </div>
              {importWarnings.length > 0 && (
                <ul className="warning-list">
                  {importWarnings.map((warning) => (
                    <li key={warning}>{warning}</li>
                  ))}
                </ul>
              )}
            </Panel>

            <Panel
              title="Runtime state"
              subtitle="Tauri events update this pane and invalidate the bootstrap query."
            >
              <div className="debug-grid">
                <Metric label="Enabled" value={engineSnapshot.engineStatus.enabled ? "Yes" : "No"} />
                <Metric label="Frontmost app" value={engineSnapshot.engineStatus.frontmostApp ?? "None"} />
                <Metric label="Selected device" value={engineSnapshot.engineStatus.selectedDeviceKey ?? "None"} />
                <Metric label="Profile" value={engineSnapshot.engineStatus.activeProfileId} />
              </div>
              <div className="log-stream">
                {(eventLog.length > 0 ? eventLog : engineSnapshot.engineStatus.debugLog).map((event) => (
                  <article className={`log-entry ${event.kind}`} key={`${event.timestampMs}-${event.message}`}>
                    <strong>{event.kind}</strong>
                    <span>{new Date(event.timestampMs).toLocaleTimeString()}</span>
                    <p>{event.message}</p>
                  </article>
                ))}
              </div>
            </Panel>
          </section>
        )}

        {isMutating && <div className="busy-banner">Applying changes...</div>}
      </section>
    </main>
  );
}

function Panel(props: { title: string; subtitle: string; children: ReactNode }) {
  return (
    <article className="panel">
      <header className="panel-header">
        <div>
          <h3>{props.title}</h3>
          <p>{props.subtitle}</p>
        </div>
      </header>
      {props.children}
    </article>
  );
}

function Metric(props: { label: string; value: string }) {
  return (
    <div className="metric">
      <span>{props.label}</span>
      <strong>{props.value}</strong>
    </div>
  );
}

function ToggleField(props: {
  label: string;
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <label className="toggle-field">
      <span>{props.label}</span>
      <input
        checked={props.checked}
        onChange={(event) => props.onChange(event.currentTarget.checked)}
        type="checkbox"
      />
    </label>
  );
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
    profiles: config.profiles.map(cloneProfile),
    settings: {
      ...config.settings,
      deviceLayoutOverrides: { ...config.settings.deviceLayoutOverrides },
    },
  };
}

function makeProfileId(label: string, config: AppConfig) {
  const base = label
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
    return layouts.find((layout) => layout.key === "generic_mouse") ?? layouts[0];
  }

  const overrideKey = config.settings.deviceLayoutOverrides[device.key];
  const targetKey = overrideKey || device.uiLayout;
  return layouts.find((layout) => layout.key === targetKey) ?? layouts[0];
}

function groupActions(actions: BootstrapPayload["availableActions"]) {
  const groups = new Map<string, BootstrapPayload["availableActions"]>();
  for (const action of actions) {
    const next = groups.get(action.category) ?? [];
    next.push(action);
    groups.set(action.category, next);
  }
  return [...groups.entries()];
}

export default App;
