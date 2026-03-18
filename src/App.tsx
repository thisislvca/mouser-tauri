import { useEffect, useRef, useState, type ReactNode, type SVGProps } from "react";
import { AnimatePresence, motion } from "framer-motion";
import {
  BugBeetle,
  CaretLeft,
  MouseLeftClick,
  MouseScroll,
  SlidersHorizontal,
  Stack,
  X,
} from "@phosphor-icons/react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  bootstrapLoad,
  configSave,
  debugClearLog,
  devicesSelectMock,
  importLegacyConfig,
  profilesCreate,
  profilesDelete,
  profilesUpdate,
} from "./lib/api";
import { sampleLegacyConfig } from "./lib/sampleLegacyConfig";
import type {
  ActionDefinition,
  AppConfig,
  Binding,
  BootstrapPayload,
  DebugEventRecord,
  DeviceInfo,
  DeviceLayout,
  LogicalControl,
  Profile,
} from "./lib/types";
import { useRuntimeEvents } from "./hooks/useRuntimeEvents";
import { type SectionName, useUiStore } from "./store/uiStore";

const SECTION_ORDER: SectionName[] = [
  "buttons",
  "devices",
  "profiles",
  "settings",
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
    label: "Point + Scroll",
    icon: MouseScroll,
  },
  profiles: {
    label: "Profiles",
    icon: Stack,
  },
  settings: {
    label: "Settings",
    icon: SlidersHorizontal,
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
};

const BUTTONS_SHEET_TRANSITION = {
  type: "spring" as const,
  stiffness: 340,
  damping: 32,
  mass: 0.92,
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
  const clearDebugEvents = useUiStore((state) => state.clearDebugEvents);

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
  const clearDebugLogMutation = useMutation({
    mutationFn: debugClearLog,
    onSuccess: () => {
      clearDebugEvents();
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
    importMutation.isPending ||
    clearDebugLogMutation.isPending;

  if (bootstrapQuery.isLoading) {
    return (
      <main className="flex min-h-screen items-center justify-center bg-white px-8 text-[#13141b]">
        <div className="rounded-[28px] border border-[#e4e7ef] bg-white px-6 py-4 text-sm font-medium">
          Loading Mouser...
        </div>
      </main>
    );
  }

  if (bootstrapQuery.isError || !bootstrap) {
    return (
      <main className="flex min-h-screen items-center justify-center bg-white px-8 text-[#13141b]">
        <div className="max-w-xl rounded-[30px] border border-[#e4e7ef] bg-white p-6">
          <p className="text-sm font-semibold">Failed to load Mouser.</p>
          <pre className="mt-4 overflow-auto rounded-3xl border border-[#e4e7ef] bg-white p-4 text-xs text-[#5d6472]">
            {String(bootstrapQuery.error)}
          </pre>
        </div>
      </main>
    );
  }

  const {
    config,
    availableActions,
    knownApps,
    engineSnapshot,
    layouts,
    platformCapabilities,
  } = bootstrap;
  const selectedProfile =
    config.profiles.find((profile) => profile.id === selectedProfileId) ??
    config.profiles.find((profile) => profile.id === config.activeProfileId) ??
    config.profiles[0];
  const activeDevice = engineSnapshot.activeDevice;
  const activeLayout = resolveActiveLayout(activeDevice, config, layouts);
  const actionLookup = new Map(availableActions.map((action) => [action.id, action]));
  const groupedActions = groupActions(availableActions);
  const runtimeEvents = eventLog.length > 0 ? eventLog : engineSnapshot.engineStatus.debugLog;

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
    createProfileMutation.mutate({
      id,
      label: labelSource,
      appMatchers: executable ? [{ kind: "executable", value: executable }] : [],
      bindings: selectedProfile.bindings.map((binding) => ({ ...binding })),
    });
    setNewProfileLabel("");
    setNewProfileApp("");
    setSelectedProfileId(id);
    setActiveSection("profiles");
  };

  const shellTitle = activeDevice?.displayName ?? "Mouser";
  const selectedAppSummary =
    engineSnapshot.engineStatus.frontmostApp ?? "All applications";
  const batteryLabel =
    activeDevice?.batteryLevel != null ? `${activeDevice.batteryLevel}%` : "N/A";

  return (
    <main className="min-h-screen bg-white text-[#11121a] antialiased">
      <div className="mx-auto grid min-h-screen max-w-[1700px] grid-rows-[92px_minmax(0,1fr)] overflow-hidden bg-white">
        <header className="grid grid-cols-[220px_minmax(0,1fr)] border-b border-[#eceef4]">
          <div className="border-r border-[#eceef4]" />

          <div className="flex min-w-0 items-center justify-between gap-6 px-8">
            <div className="flex min-w-0 items-center gap-4">
              <button
                aria-label="Back"
                className="flex h-10 w-10 items-center justify-center rounded-full text-[#1c2028] transition hover:bg-[#f3f4f7]"
                type="button"
              >
                <CaretLeft className="h-5 w-5" />
              </button>
              <h1 className="truncate text-[22px] font-semibold tracking-[-0.04em] text-[#111318]">
                {shellTitle}
              </h1>
            </div>

            <div className="flex shrink-0 items-center gap-3">
              {isMutating && <StatusPill tone="accent" value="Applying" />}
              <button
                className="rounded-full border border-[#d9dce6] bg-white px-5 py-3 text-sm font-semibold text-[#171b24] transition hover:bg-[#f7f8fb]"
                onClick={() => setActiveSection("profiles")}
                type="button"
              >
                + Add Application
              </button>
            </div>
          </div>
        </header>

        <div className="grid min-h-0 grid-cols-[220px_minmax(0,1fr)]">
          <aside className="flex flex-col border-r border-[#eceef4] bg-white px-5 py-7">
            <nav className="space-y-1">
              {SECTION_ORDER.map((section) => (
                <SectionNavButton
                  active={activeSection === section}
                  icon={SECTION_META[section].icon}
                  key={section}
                  label={SECTION_META[section].label}
                  onClick={() => setActiveSection(section)}
                />
              ))}
            </nav>

            <div className="mt-auto">
              <div className="inline-flex items-center gap-2 rounded-[14px] border border-[#dfe4ea] bg-white px-3 py-2 text-sm font-semibold text-[#35a95c]">
                <span>{batteryLabel}</span>
                <span className="text-[#7d8395]">battery</span>
              </div>
            </div>
          </aside>

          <section className="min-w-0 overflow-y-auto bg-white px-8 py-8">
            {activeSection === "buttons" && (
              <ButtonsView
                actionLookup={actionLookup}
                activeDevice={activeDevice}
                activeLayout={activeLayout}
                config={config}
                frontmostApp={engineSnapshot.engineStatus.frontmostApp}
                groupedActions={groupedActions}
                mappingEngineReady={platformCapabilities.mappingEngineReady}
                platformCapabilities={platformCapabilities}
                profile={selectedProfile}
                updateSelectedProfile={updateSelectedProfile}
              />
            )}

            {activeSection === "devices" && (
              <PointAndScrollView
                actionLookup={actionLookup}
                activeDevice={activeDevice}
                activeLayout={activeLayout}
                bootstrap={bootstrap}
                config={config}
                engineSnapshot={engineSnapshot}
                layoutChoices={bootstrap.manualLayoutChoices}
                profile={selectedProfile}
                saveSettings={saveSettings}
                selectDevice={selectDeviceMutation.mutate}
              />
            )}

            {activeSection === "profiles" && (
              <ProfilesView
                deleteProfile={deleteProfileMutation.mutate}
                knownApps={knownApps}
                profile={selectedProfile}
                profiles={config.profiles}
                setSelectedProfileId={setSelectedProfileId}
                updateSelectedProfile={updateSelectedProfile}
              />
            )}

            {activeSection === "settings" && (
              <div className="space-y-6">
                <div className="grid gap-4 rounded-[26px] border border-[#eceef4] bg-white p-5 md:grid-cols-[minmax(0,1fr)_360px]">
                  <div className="space-y-3">
                    <p className="text-[11px] font-semibold uppercase tracking-[0.24em] text-[#9095a3]">
                      Quick add
                    </p>
                    <div className="grid gap-3 md:grid-cols-2">
                      <input
                        className="field-input"
                        list="known-apps"
                        placeholder="Known app executable"
                        value={newProfileApp}
                        onChange={(event) => setNewProfileApp(event.currentTarget.value)}
                      />
                      <input
                        className="field-input"
                        placeholder="Optional custom label"
                        value={newProfileLabel}
                        onChange={(event) => setNewProfileLabel(event.currentTarget.value)}
                      />
                    </div>
                    <datalist id="known-apps">
                      {knownApps.map((app) => (
                        <option key={app.executable} value={app.executable}>
                          {app.label}
                        </option>
                      ))}
                    </datalist>
                  </div>

                  <div className="flex items-end justify-end">
                    <button
                      className="rounded-full bg-[#2563eb] px-5 py-3 text-sm font-semibold text-white transition hover:bg-[#1d4ed8]"
                      onClick={createProfileFromDraft}
                      type="button"
                    >
                      Create profile
                    </button>
                  </div>
                </div>

                <SettingsView
                  activeDevice={activeDevice}
                  config={config}
                  platformCapabilities={platformCapabilities}
                  saveSettings={saveSettings}
                />
              </div>
            )}

            {activeSection === "debug" && (
              <DebugView
                clearDebugLog={clearDebugLogMutation.mutate}
                config={config}
                debugEvents={runtimeEvents}
                importDraft={importDraft}
                importSourcePath={importSourcePath}
                importWarnings={importWarnings}
                isClearing={clearDebugLogMutation.isPending}
                onImport={() =>
                  importMutation.mutate({
                    sourcePath: importSourcePath.trim() || null,
                    rawJson: importDraft,
                  })
                }
                platformCapabilities={platformCapabilities}
                saveSettings={saveSettings}
                setImportDraft={setImportDraft}
                setImportSourcePath={setImportSourcePath}
              />
            )}
          </section>
        </div>
      </div>
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
  frontmostApp: string | null | undefined;
  mappingEngineReady: boolean;
  updateSelectedProfile: (mutateProfile: (profile: Profile) => void) => void;
}) {
  const [selectedControl, setSelectedControl] = useState<LogicalControl | null>(null);
  const selectedHotspot =
    props.activeLayout.hotspots.find((hotspot) => hotspot.control === selectedControl) ?? null;

  useEffect(() => {
    if (!props.activeDevice) {
      setSelectedControl(null);
      return;
    }

    const visibleControls = new Set(props.activeLayout.hotspots.map((hotspot) => hotspot.control));
    setSelectedControl((current) =>
      current && visibleControls.has(current) ? current : null,
    );
  }, [props.activeDevice, props.activeLayout]);

  const setBinding = (control: LogicalControl, actionId: string) => {
    props.updateSelectedProfile((nextProfile) => {
      upsertBinding(nextProfile, control, actionId);
    });
  };

  return (
    <div className="min-h-full">
      {props.activeDevice ? (
        <div className="flex min-h-[760px] flex-col gap-8 xl:flex-row xl:items-stretch">
          <motion.div
            layout
            className="min-w-0 flex-1"
            transition={BUTTONS_SHEET_TRANSITION}
          >
            <ButtonsWorkbench
              actionLookup={props.actionLookup}
              activeDevice={props.activeDevice}
              layout={props.activeLayout}
              profile={props.profile}
              selectedControl={selectedControl}
              onSelectControl={setSelectedControl}
            />
          </motion.div>

          <AnimatePresence initial={false}>
            {selectedHotspot && (
              <motion.aside
                animate={{ maxWidth: 440, opacity: 1 }}
                className="w-full overflow-hidden xl:flex-none"
                exit={{ maxWidth: 0, opacity: 0 }}
                initial={{ maxWidth: 0, opacity: 0 }}
                key={selectedHotspot.control}
                transition={BUTTONS_SHEET_TRANSITION}
              >
                <motion.div
                  animate={{ x: 0, opacity: 1 }}
                  className="h-full w-full border-t border-[#eceef4] pt-6 xl:w-[420px] xl:border-t-0 xl:border-l xl:pl-8 xl:pt-0"
                  exit={{ x: 28, opacity: 0 }}
                  initial={{ x: 28, opacity: 0 }}
                  transition={{ duration: 0.22, ease: [0.22, 1, 0.36, 1] }}
                >
                  <ButtonsControlSheet
                    actionLookup={props.actionLookup}
                    control={selectedHotspot.control}
                    frontmostApp={props.frontmostApp}
                    groupedActions={props.groupedActions}
                    mappingEngineReady={props.mappingEngineReady}
                    platformCapabilities={props.platformCapabilities}
                    profile={props.profile}
                    setBinding={setBinding}
                    onClose={() => setSelectedControl(null)}
                  />
                </motion.div>
              </motion.aside>
            )}
          </AnimatePresence>
        </div>
      ) : (
        <EmptyStage
          body="Connect a supported mouse to inspect mapped controls."
          title="No device detected"
        />
      )}
    </div>
  );
}

function PointAndScrollView(props: {
  actionLookup: Map<string, ActionDefinition>;
  bootstrap: BootstrapPayload;
  config: AppConfig;
  activeDevice: DeviceInfo | null;
  activeLayout: DeviceLayout;
  engineSnapshot: BootstrapPayload["engineSnapshot"];
  layoutChoices: BootstrapPayload["manualLayoutChoices"];
  profile: Profile;
  saveSettings: (mutateConfig: (nextConfig: AppConfig) => void) => void;
  selectDevice: (deviceKey: string) => void;
}) {
  return (
    <div className="space-y-6">
      <div className="grid gap-6 xl:grid-cols-[minmax(0,1.1fr)_380px]">
        <StagePanel
          eyebrow="Device"
          title={props.activeDevice?.displayName ?? "Point and scroll"}
          subtitle="DPI, wheel direction, and manual layout overrides stay on the active device."
        >
          {props.activeDevice ? (
            <DeviceCanvas
              actionLookup={props.actionLookup}
              device={props.activeDevice}
              layout={props.activeLayout}
              profile={props.profile}
            />
          ) : (
            <EmptyStage
              body="Select or connect a Logitech device to tune DPI and wheel behavior."
              title="No active device"
            />
          )}
        </StagePanel>

        <Panel
          eyebrow="Tuning"
          title="Point + scroll"
          subtitle="These settings mirror the device-level controls you expect from Options+."
        >
          <div className="space-y-4">
            <Field label="DPI">
              <input
                className="field-input"
                data-testid="dpi-input"
                max={props.activeDevice?.dpiMax ?? 8000}
                min={props.activeDevice?.dpiMin ?? 200}
                type="number"
                value={props.config.settings.dpi}
                onChange={(event) =>
                  props.saveSettings((nextConfig) => {
                    nextConfig.settings.dpi = Number(event.currentTarget.value);
                  })
                }
              />
            </Field>

            <Field label="Manual layout override">
              <select
                className="field-input"
                value={props.activeDevice ? props.config.settings.deviceLayoutOverrides[props.activeDevice.key] ?? "" : ""}
                onChange={(event) =>
                  props.saveSettings((nextConfig) => {
                    if (!props.activeDevice) {
                      return;
                    }

                    if (event.currentTarget.value) {
                      nextConfig.settings.deviceLayoutOverrides[props.activeDevice.key] =
                        event.currentTarget.value;
                    } else {
                      delete nextConfig.settings.deviceLayoutOverrides[props.activeDevice.key];
                    }
                  })
                }
              >
                {props.layoutChoices.map((choice) => (
                  <option key={choice.key || "auto"} value={choice.key}>
                    {choice.label}
                  </option>
                ))}
              </select>
            </Field>

            <SwitchRow
              checked={props.config.settings.invertHorizontalScroll}
              label="Invert thumb wheel"
              onChange={(value) =>
                props.saveSettings((nextConfig) => {
                  nextConfig.settings.invertHorizontalScroll = value;
                })
              }
            />
            <SwitchRow
              checked={props.config.settings.invertVerticalScroll}
              label="Invert vertical scroll"
              onChange={(value) =>
                props.saveSettings((nextConfig) => {
                  nextConfig.settings.invertVerticalScroll = value;
                })
              }
            />
            <Field label="Gesture threshold">
              <input
                className="field-input"
                type="number"
                value={props.config.settings.gestureThreshold}
                onChange={(event) =>
                  props.saveSettings((nextConfig) => {
                    nextConfig.settings.gestureThreshold = Number(event.currentTarget.value);
                  })
                }
              />
            </Field>
            <Field label="Gesture deadzone">
              <input
                className="field-input"
                type="number"
                value={props.config.settings.gestureDeadzone}
                onChange={(event) =>
                  props.saveSettings((nextConfig) => {
                    nextConfig.settings.gestureDeadzone = Number(event.currentTarget.value);
                  })
                }
              />
            </Field>
          </div>
        </Panel>
      </div>

      <div className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_340px]">
        <Panel
          eyebrow="Devices"
          title="Connected roster"
          subtitle="Use this to verify which Logitech interface the runtime is probing."
        >
          <div className="space-y-3">
            {props.engineSnapshot.devices.length > 0 ? (
              props.engineSnapshot.devices.map((device) => (
                <button
                  className={[
                    "w-full rounded-[24px] border px-4 py-4 text-left transition",
                    device.key === props.engineSnapshot.activeDeviceKey
                      ? "border-[#93c5fd] bg-white text-[#111318] shadow-[0_12px_28px_rgba(37,99,235,0.08)]"
                      : "border-[#e7eaf2] bg-white hover:border-[#d4d9ea]",
                  ].join(" ")}
                  key={device.key}
                  onClick={() => props.selectDevice(device.key)}
                  type="button"
                >
                  <div className="flex items-start justify-between gap-4">
                    <div>
                      <p className="text-sm font-semibold">{device.displayName}</p>
                      <p
                        className={[
                          "mt-1 text-xs",
                          device.key === props.engineSnapshot.activeDeviceKey
                            ? "text-[#717787]"
                            : "text-[#717787]",
                        ].join(" ")}
                      >
                        {device.transport ?? "Unknown transport"}
                      </p>
                    </div>
                    <StatusPill
                      tone={device.connected ? "success" : "neutral"}
                      value={device.connected ? "Active" : "Seen"}
                    />
                  </div>
                </button>
              ))
            ) : (
              <EmptyState
                body="No Logitech HID interface was detected by the current backend."
                title="No devices"
              />
            )}
          </div>
        </Panel>

        <Panel
          eyebrow="Runtime"
          title="Probe status"
          subtitle="This is the state the Rust backend is actually exposing."
        >
          <div className="space-y-3">
            <CapabilityRow label="Active HID backend" value={props.bootstrap.platformCapabilities.activeHidBackend} />
            <CapabilityRow label="Active hook backend" value={props.bootstrap.platformCapabilities.activeHookBackend} />
            <CapabilityRow label="Active focus backend" value={props.bootstrap.platformCapabilities.activeFocusBackend} />
            <CapabilityRow label="hidapi" value={props.bootstrap.platformCapabilities.hidapiAvailable ? "Ready" : "Unavailable"} />
            <CapabilityRow label="iokit" value={props.bootstrap.platformCapabilities.iokitAvailable ? "Ready" : "Not ported"} />
          </div>
        </Panel>
      </div>
    </div>
  );
}

function ProfilesView(props: {
  profiles: Profile[];
  profile: Profile;
  knownApps: BootstrapPayload["knownApps"];
  setSelectedProfileId: (profileId: string | null) => void;
  updateSelectedProfile: (mutateProfile: (profile: Profile) => void) => void;
  deleteProfile: (profileId: string) => void;
}) {
  return (
    <div className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_380px]">
      <Panel
        eyebrow="Profiles"
        title="Application routing"
        subtitle="Each profile can target one or more executables."
      >
        <div className="space-y-3">
          {props.profiles.map((profile) => {
            const profileApp = profile.appMatchers[0]?.value
              ? props.knownApps.find((app) => app.executable === profile.appMatchers[0]?.value) ?? null
              : null;
            return (
              <button
                className={[
                  "flex w-full items-center justify-between gap-4 rounded-[24px] border px-4 py-4 text-left transition",
                  profile.id === props.profile.id
                    ? "border-[#93c5fd] bg-white shadow-[0_12px_28px_rgba(37,99,235,0.08)]"
                    : "border-[#e7eaf2] bg-white hover:border-[#d4d9ea]",
                ].join(" ")}
                key={profile.id}
                onClick={() => props.setSelectedProfileId(profile.id)}
                type="button"
              >
                <div className="min-w-0">
                  <p className="truncate text-sm font-semibold text-[#10131a]">{profile.label}</p>
                  <p className="mt-1 truncate text-xs text-[#717787]">
                    {profile.appMatchers.map((matcher) => matcher.value).join(", ") || "All applications"}
                  </p>
                </div>
                {profileApp?.iconAsset ? (
                  <img
                    alt={profileApp.label}
                    className="h-11 w-11 rounded-2xl border border-[#e7eaf2] bg-white object-cover"
                    src={profileApp.iconAsset}
                  />
                ) : (
                  <StatusPill tone="neutral" value={profile.id} />
                )}
              </button>
            );
          })}
        </div>
      </Panel>

      <Panel
        eyebrow="Editor"
        title="Selected profile"
        subtitle="Edit the label, executable routes, and lifecycle of the active profile."
      >
        <div className="space-y-4">
          <Field label="Label">
            <input
              className="field-input"
              data-testid="profile-label-input"
              value={props.profile.label}
              onChange={(event) =>
                props.updateSelectedProfile((nextProfile) => {
                  nextProfile.label = event.currentTarget.value;
                })
              }
            />
          </Field>

          <Field label="Executable matchers">
            <textarea
              className="field-input min-h-[180px] resize-y py-3"
              rows={6}
              value={props.profile.appMatchers.map((matcher) => matcher.value).join("\n")}
              onChange={(event) =>
                props.updateSelectedProfile((nextProfile) => {
                  nextProfile.appMatchers = event.currentTarget.value
                    .split("\n")
                    .map((value) => value.trim())
                    .filter(Boolean)
                    .map((value) => ({ kind: "executable", value }));
                })
              }
            />
          </Field>

          <div className="rounded-[24px] border border-[#e7eaf2] bg-white px-4 py-4">
            <p className="text-[11px] font-semibold uppercase tracking-[0.24em] text-[#8a8fa0]">
              Current selection
            </p>
            <p
              className="mt-3 text-sm font-semibold text-[#10131a]"
              data-testid="profile-label-display"
            >
              {props.profile.label}
            </p>
          </div>

          <button
            className="flex w-full items-center justify-center rounded-2xl border border-[#efc7c7] bg-white px-4 py-3 text-sm font-semibold text-[#8d3a3a] transition hover:border-[#e3b1b1] disabled:cursor-not-allowed disabled:opacity-40"
            disabled={props.profile.id === "default"}
            onClick={() => props.deleteProfile(props.profile.id)}
            type="button"
          >
            Delete profile
          </button>
        </div>
      </Panel>
    </div>
  );
}

function SettingsView(props: {
  config: AppConfig;
  activeDevice: DeviceInfo | null;
  platformCapabilities: BootstrapPayload["platformCapabilities"];
  saveSettings: (mutateConfig: (nextConfig: AppConfig) => void) => void;
}) {
  return (
    <div className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_340px]">
      <Panel
        eyebrow="App"
        title="Global preferences"
        subtitle="Startup behavior, appearance, and gesture tuning live here."
      >
        <div className="grid gap-4 md:grid-cols-2">
          <SwitchRow
            checked={props.config.settings.startMinimized}
            label="Start minimized"
            onChange={(value) =>
              props.saveSettings((nextConfig) => {
                nextConfig.settings.startMinimized = value;
              })
            }
          />
          <SwitchRow
            checked={props.config.settings.startAtLogin}
            label="Start at login"
            onChange={(value) =>
              props.saveSettings((nextConfig) => {
                nextConfig.settings.startAtLogin = value;
              })
            }
          />
          <SwitchRow
            checked={props.config.settings.debugMode}
            label="Enable debug mode"
            onChange={(value) =>
              props.saveSettings((nextConfig) => {
                nextConfig.settings.debugMode = value;
              })
            }
          />
          <Field label="Appearance mode">
            <select
              className="field-input"
              value={props.config.settings.appearanceMode}
              onChange={(event) =>
                props.saveSettings((nextConfig) => {
                  nextConfig.settings.appearanceMode =
                    event.currentTarget.value as AppConfig["settings"]["appearanceMode"];
                })
              }
            >
              <option value="system">System</option>
              <option value="light">Light</option>
              <option value="dark">Dark</option>
            </select>
          </Field>
          <Field label="Gesture timeout (ms)">
            <input
              className="field-input"
              type="number"
              value={props.config.settings.gestureTimeoutMs}
              onChange={(event) =>
                props.saveSettings((nextConfig) => {
                  nextConfig.settings.gestureTimeoutMs = Number(event.currentTarget.value);
                })
              }
            />
          </Field>
          <Field label="Gesture cooldown (ms)">
            <input
              className="field-input"
              type="number"
              value={props.config.settings.gestureCooldownMs}
              onChange={(event) =>
                props.saveSettings((nextConfig) => {
                  nextConfig.settings.gestureCooldownMs = Number(event.currentTarget.value);
                })
              }
            />
          </Field>
        </div>
      </Panel>

      <Panel
        eyebrow="Readiness"
        title="Runtime status"
        subtitle="The backend needs both HID and a live hook path for full parity."
      >
        <div className="space-y-3">
          <CapabilityRow label="Platform" value={props.platformCapabilities.platform} />
          <CapabilityRow label="Selected DPI" value={`${props.config.settings.dpi}`} />
          <CapabilityRow label="Live HID" value={props.platformCapabilities.liveHidAvailable ? "Ready" : "Fallback"} />
          <CapabilityRow label="Live remapping" value={props.platformCapabilities.mappingEngineReady ? "Ready" : "Not yet"} />
          <CapabilityRow label="Selected device" value={props.activeDevice?.displayName ?? "None"} />
          <CapabilityRow label="Tray" value={props.platformCapabilities.trayReady ? "Ready" : "Pending"} />
        </div>
      </Panel>
    </div>
  );
}

function DebugView(props: {
  config: AppConfig;
  platformCapabilities: BootstrapPayload["platformCapabilities"];
  debugEvents: DebugEventRecord[];
  importDraft: string;
  importSourcePath: string;
  importWarnings: string[];
  isClearing: boolean;
  saveSettings: (mutateConfig: (nextConfig: AppConfig) => void) => void;
  clearDebugLog: () => void;
  onImport: () => void;
  setImportDraft: (value: string) => void;
  setImportSourcePath: (value: string) => void;
}) {
  return (
    <div className="grid gap-6 xl:grid-cols-[minmax(0,1.2fr)_360px]">
      <Panel
        eyebrow="Diagnostics"
        title="Runtime log"
        subtitle="Use this to confirm what the Rust runtime is actually doing on macOS."
      >
        <div className="flex flex-wrap items-center gap-3">
          <StatusPill
            tone={props.config.settings.debugMode ? "accent" : "neutral"}
            value={props.config.settings.debugMode ? "Debug on" : "Debug off"}
          />
          <button
            className="rounded-full border border-[#d8dded] bg-white px-4 py-2 text-sm font-semibold text-[#171b24] transition hover:border-[#c8cee0] disabled:cursor-not-allowed disabled:opacity-50"
            disabled={props.isClearing}
            onClick={props.clearDebugLog}
            type="button"
          >
            Clear log
          </button>
        </div>

        <div className="mt-5 grid gap-3 md:grid-cols-2">
          <CapabilityRow label="Active HID backend" value={props.platformCapabilities.activeHidBackend} />
          <CapabilityRow label="Active hook backend" value={props.platformCapabilities.activeHookBackend} />
          <CapabilityRow label="Active focus backend" value={props.platformCapabilities.activeFocusBackend} />
          <CapabilityRow label="iokit backend" value={props.platformCapabilities.iokitAvailable ? "Ready" : "Not ported"} />
        </div>

        <div className="mt-5 rounded-[28px] border border-[#e7eaf2] bg-white p-3">
          <div className="max-h-[560px] space-y-3 overflow-y-auto pr-1">
            {props.debugEvents.length > 0 ? (
              props.debugEvents.map((event) => (
                <LogEntry event={event} key={`${event.timestampMs}-${event.message}`} />
              ))
            ) : (
              <EmptyState
                body="Enable debug mode, then interact with the app to collect backend events."
                title="No debug events"
              />
            )}
          </div>
        </div>
      </Panel>

      <div className="space-y-6">
        <Panel
          eyebrow="Controls"
          title="Debug mode"
          subtitle="This mirrors the Python app: enable verbose runtime reporting when you need it."
        >
          <SwitchRow
            checked={props.config.settings.debugMode}
            label="Enable debug mode"
            onChange={(value) =>
              props.saveSettings((nextConfig) => {
                nextConfig.settings.debugMode = value;
              })
            }
          />
        </Panel>

        <Panel
          eyebrow="Importer"
          title="Legacy config"
          subtitle="Hydrate the Rust config from the existing Python Mouser JSON."
        >
          <div className="space-y-4">
            <Field label="Optional source path">
              <input
                className="field-input"
                placeholder="~/Library/Application Support/Mouser/config.json"
                value={props.importSourcePath}
                onChange={(event) => props.setImportSourcePath(event.currentTarget.value)}
              />
            </Field>
            <Field label="Legacy Mouser JSON">
              <textarea
                className="field-input min-h-[280px] resize-y py-3 font-mono text-xs leading-6"
                data-testid="legacy-import-input"
                rows={12}
                value={props.importDraft}
                onChange={(event) => props.setImportDraft(event.currentTarget.value)}
              />
            </Field>
            <button
              className="flex w-full items-center justify-center rounded-2xl bg-[#2563eb] px-4 py-3 text-sm font-semibold text-white transition hover:bg-[#1d4ed8]"
              data-testid="legacy-import-button"
              onClick={props.onImport}
              type="button"
            >
              Import legacy config
            </button>
            {props.importWarnings.length > 0 && (
              <ul className="space-y-2 rounded-[24px] border border-[#efd8af] bg-white p-4 text-sm text-[#8b5f1b]">
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
  onClick: () => void;
  icon: (props: SVGProps<SVGSVGElement>) => ReactNode;
}) {
  const Icon = props.icon;

  return (
    <button
      aria-label={props.label}
      className={[
        "flex w-full items-center gap-3 rounded-[18px] px-4 py-3 text-left transition",
        props.active
          ? "bg-[#2563eb] text-white shadow-[0_16px_32px_rgba(37,99,235,0.22)]"
          : "text-[#1f232c] hover:bg-[#f4f5f8]",
      ].join(" ")}
      onClick={props.onClick}
      type="button"
    >
      <Icon className="h-5 w-5" />
      <span className="text-sm font-semibold">{props.label}</span>
    </button>
  );
}

function Panel(props: {
  eyebrow: string;
  title: string;
  subtitle: string;
  children: ReactNode;
}) {
  return (
    <section className="rounded-[26px] border border-[#eceef4] bg-white p-6">
      <p className="text-[11px] font-semibold uppercase tracking-[0.24em] text-[#8a8fa0]">
        {props.eyebrow}
      </p>
      <div className="mt-3 border-b border-[#eef1f6] pb-4">
        <h3 className="text-[24px] font-semibold tracking-[-0.04em] text-[#10131a]">
          {props.title}
        </h3>
        <p className="mt-2 text-sm text-[#656c7d]">{props.subtitle}</p>
      </div>
      <div className="pt-5">{props.children}</div>
    </section>
  );
}

function StagePanel(props: {
  eyebrow: string;
  title: string;
  subtitle: string;
  children: ReactNode;
}) {
  return (
    <section className="rounded-[26px] border border-[#eceef4] bg-white p-6">
      <p className="text-[11px] font-semibold uppercase tracking-[0.24em] text-[#8a8fa0]">
        {props.eyebrow}
      </p>
      <div className="mt-3 border-b border-[#eef1f6] pb-4">
        <h3 className="text-[26px] font-semibold tracking-[-0.05em] text-[#10131a]">
          {props.title}
        </h3>
        <p className="mt-2 text-sm text-[#656c7d]">{props.subtitle}</p>
      </div>
      <div className="pt-5">{props.children}</div>
    </section>
  );
}

function Field(props: { label: string; children: ReactNode }) {
  return (
    <label className="block">
      <span className="mb-2 block text-sm font-medium text-[#2f3441]">{props.label}</span>
      {props.children}
    </label>
  );
}

function SwitchRow(props: {
  label: string;
  checked: boolean;
  onChange: (value: boolean) => void;
}) {
  return (
    <label className="flex items-center justify-between rounded-[22px] border border-[#e7eaf2] bg-white px-4 py-4 text-sm font-medium text-[#171b24]">
      <span>{props.label}</span>
      <input
        checked={props.checked}
        className="h-4 w-4 accent-[#2563eb]"
        onChange={(event) => props.onChange(event.currentTarget.checked)}
        type="checkbox"
      />
    </label>
  );
}

function ButtonsWorkbench(props: {
  activeDevice: DeviceInfo;
  layout: DeviceLayout;
  profile: Profile;
  actionLookup: Map<string, ActionDefinition>;
  selectedControl: LogicalControl | null;
  onSelectControl: (control: LogicalControl) => void;
}) {
  return (
    <section className="relative flex min-h-[760px] items-center justify-center bg-white">
      <div className="relative flex min-h-[720px] w-full items-center justify-center px-6 py-10 xl:px-10">
        <div
          className="relative"
          style={{ width: props.layout.imageWidth, height: props.layout.imageHeight }}
        >
          <img
            alt={props.layout.label}
            className="absolute inset-0 h-full w-full object-contain drop-shadow-[0_28px_44px_rgba(15,23,42,0.14)]"
            data-testid="device-layout-image"
            src={props.layout.imageAsset}
          />

          {props.layout.hotspots.map((hotspot) => {
            const isSelected = props.selectedControl === hotspot.control;
            const summary = summarizeHotspot(props.profile, hotspot.control, props.actionLookup);
            const labelX = hotspot.normX * props.layout.imageWidth + hotspot.labelOffX;
            const labelY = hotspot.normY * props.layout.imageHeight + hotspot.labelOffY;

            return (
              <div key={hotspot.control}>
                <span
                  className={[
                    "absolute z-10 h-4 w-4 -translate-x-1/2 -translate-y-1/2 rounded-full border-[3px] bg-[#111318] transition",
                    isSelected
                      ? "border-[#bfdbfe] bg-[#2563eb] shadow-[0_0_0_10px_rgba(37,99,235,0.14)]"
                      : "border-white shadow-[0_10px_24px_rgba(15,23,42,0.14)]",
                  ].join(" ")}
                  style={{
                    left: hotspot.normX * props.layout.imageWidth,
                    top: hotspot.normY * props.layout.imageHeight,
                  }}
                  title={hotspot.label}
                />
                <button
                  aria-label={hotspot.label}
                  aria-pressed={isSelected}
                  className={[
                    "absolute z-20 max-w-[260px] rounded-[26px] border px-8 py-6 text-left transition",
                    isSelected
                      ? "border-[#2563eb] bg-white shadow-[0_20px_36px_rgba(37,99,235,0.16)]"
                      : "border-[#eceef4] bg-white shadow-[0_18px_32px_rgba(15,23,42,0.08)] hover:border-[#d6dbee]",
                  ].join(" ")}
                  data-testid={`hotspot-card-${hotspot.control}`}
                  onClick={() => props.onSelectControl(hotspot.control)}
                  style={{ left: labelX, top: labelY }}
                  type="button"
                >
                  <p className="text-[17px] font-semibold tracking-[-0.03em] text-[#111318]">
                    {hotspot.label}
                  </p>
                  <p className="mt-3 text-[15px] leading-8 text-[#677084]">{summary}</p>
                </button>
              </div>
            );
          })}
        </div>
      </div>
    </section>
  );
}

function ButtonsControlSheet(props: {
  control: LogicalControl;
  profile: Profile;
  actionLookup: Map<string, ActionDefinition>;
  groupedActions: Array<[string, ActionDefinition[]]>;
  frontmostApp: string | null | undefined;
  mappingEngineReady: boolean;
  platformCapabilities: BootstrapPayload["platformCapabilities"];
  setBinding: (control: LogicalControl, actionId: string) => void;
  onClose: () => void;
}) {
  const controls = editorControlsFor(props.control);
  const title = editorTitleFor(props.control);
  const description = editorDescriptionFor(props.control);
  const note =
    !props.mappingEngineReady
      ? "Live remapping is unavailable because the macOS event tap did not start."
      : props.control === "gesture_press" && !props.platformCapabilities.iokitAvailable
        ? "Gesture-button swipe diversion still depends on the native IOKit listener."
        : null;

  return (
    <section
      className="flex h-full min-h-[520px] flex-col rounded-[34px] border border-[#eceef4] bg-white p-6"
      data-testid="buttons-editor-sheet"
    >
      <div className="flex items-start justify-between gap-4 border-b border-[#eceef4] pb-5">
        <div>
          <p className="text-[11px] font-semibold uppercase tracking-[0.24em] text-[#8a8fa0]">
            Assignment
          </p>
          <h3 className="mt-3 text-[34px] font-semibold tracking-[-0.05em] text-[#10131a]">
            {title}
          </h3>
          <p className="mt-3 max-w-sm text-sm leading-7 text-[#677084]">{description}</p>
        </div>

        <button
          aria-label="Close button editor"
          className="flex h-11 w-11 items-center justify-center rounded-full border border-[#e1e5ef] text-[#353c49] transition hover:border-[#cfd5e4]"
          onClick={props.onClose}
          type="button"
        >
          <X className="h-4 w-4" />
        </button>
      </div>

      <div className="mt-5 flex flex-wrap gap-2">
        <InfoPill label="Profile" value={props.profile.label} />
        <InfoPill label="App" value={props.frontmostApp ?? "All applications"} />
        <StatusPill
          tone={props.mappingEngineReady ? "success" : "warning"}
          value={props.mappingEngineReady ? "Live" : props.platformCapabilities.activeHookBackend}
        />
      </div>

      <div className="mt-6 rounded-[28px] border border-[#eceef4] bg-white px-5 py-5">
        <p className="text-[11px] font-semibold uppercase tracking-[0.24em] text-[#8a8fa0]">
          Current mapping
        </p>
        <p className="mt-3 text-base leading-8 text-[#171b24]">
          {summarizeHotspot(props.profile, props.control, props.actionLookup)}
        </p>
      </div>

      <div className="mt-6 flex-1 space-y-4 overflow-y-auto pr-1">
        {controls.map((control) => (
          <SheetActionField
            actionLookup={props.actionLookup}
            control={control}
            groupedActions={props.groupedActions}
            key={control}
            label={sheetFieldLabelFor(control)}
            onChange={(actionId) => props.setBinding(control, actionId)}
            profile={props.profile}
          />
        ))}

        {note && <InlineNotice tone={props.mappingEngineReady ? "accent" : "warning"}>{note}</InlineNotice>}
      </div>
    </section>
  );
}

function SheetActionField(props: {
  control: LogicalControl;
  label: string;
  profile: Profile;
  actionLookup: Map<string, ActionDefinition>;
  groupedActions: Array<[string, ActionDefinition[]]>;
  onChange: (actionId: string) => void;
}) {
  const currentBinding = bindingFor(props.profile, props.control);

  return (
    <div className="rounded-[24px] border border-[#e7eaf2] bg-white p-4">
      <div className="flex items-start justify-between gap-3">
        <div>
          <p className="text-sm font-semibold text-[#10131a]">{props.label}</p>
          <p className="mt-1 text-xs uppercase tracking-[0.18em] text-[#8a8fa0]">
            {props.control.replace(/_/g, " ")}
          </p>
        </div>
        <span className="rounded-full border border-[#e2e6f0] px-3 py-1 text-xs font-semibold text-[#596173]">
          {actionFor(props.profile, props.control, props.actionLookup)}
        </span>
      </div>
      <select
        aria-label={props.label}
        className="field-input mt-4"
        value={currentBinding.actionId}
        onChange={(event) => props.onChange(event.currentTarget.value)}
      >
        {props.groupedActions.map(([category, actions]) => (
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
}

function DeviceCanvas(props: {
  device: DeviceInfo;
  layout: DeviceLayout;
  profile: Profile;
  actionLookup: Map<string, ActionDefinition>;
}) {
  return (
    <div
      className="rounded-[28px] border border-[#eef0f5] bg-white p-6"
      data-testid="device-layout-card"
    >
      <div className="relative mx-auto min-h-[500px] w-full max-w-[920px]">
        <div
          className="relative mx-auto"
          style={{ width: props.layout.imageWidth, height: props.layout.imageHeight }}
        >
          <img
            alt={props.layout.label}
            className="absolute inset-0 h-full w-full object-contain drop-shadow-[0_24px_36px_rgba(15,23,42,0.12)]"
            data-testid="device-layout-image"
            src={props.layout.imageAsset}
          />
          {props.layout.hotspots.map((hotspot) => {
            const summary = summarizeHotspot(props.profile, hotspot.control, props.actionLookup);
            const labelX = hotspot.normX * props.layout.imageWidth + hotspot.labelOffX;
            const labelY = hotspot.normY * props.layout.imageHeight + hotspot.labelOffY;
            return (
              <div key={hotspot.control}>
                <span
                  className="absolute z-10 h-4 w-4 -translate-x-1/2 -translate-y-1/2 rounded-full border-[3px] border-white bg-[#111318] shadow-[0_10px_24px_rgba(15,23,42,0.14)]"
                  style={{
                    left: hotspot.normX * props.layout.imageWidth,
                    top: hotspot.normY * props.layout.imageHeight,
                  }}
                  title={hotspot.label}
                />
                <div
                  className="absolute z-20 max-w-[240px] rounded-[22px] border border-[#e7eaf2] bg-white px-4 py-3 shadow-[0_12px_24px_rgba(15,23,42,0.08)]"
                  style={{ left: labelX, top: labelY }}
                >
                  <p className="text-sm font-semibold text-[#10131a]">{hotspot.label}</p>
                  <p className="mt-1 text-xs leading-5 text-[#656c7d]">{summary}</p>
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

function InfoPill(props: { label: string; value: string }) {
  return (
    <span className="rounded-full border border-[#dfe4f0] bg-white px-3 py-1.5 text-xs font-semibold text-[#515869]">
      <span className="text-[#9096a8]">{props.label}</span> {props.value}
    </span>
  );
}

function StatusPill(props: {
  tone: "success" | "accent" | "neutral" | "warning";
  value: string;
}) {
  const toneClass =
    props.tone === "success"
      ? "border-[#cfe9da] bg-white text-[#177a4d]"
      : props.tone === "accent"
        ? "border-[#bfdbfe] bg-white text-[#2563eb]"
        : props.tone === "warning"
          ? "border-[#f3dfbe] bg-white text-[#92611f]"
          : "border-[#e3e7f0] bg-white text-[#596071]";

  return (
    <span className={`rounded-full border px-3 py-1.5 text-xs font-semibold ${toneClass}`}>
      {props.value}
    </span>
  );
}

function CapabilityRow(props: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between rounded-[20px] border border-[#e7eaf2] bg-white px-4 py-3 text-sm">
      <span className="font-medium text-[#2f3441]">{props.label}</span>
      <span className="text-[#10131a]">{props.value}</span>
    </div>
  );
}

function LogEntry(props: { event: DebugEventRecord }) {
  const accent =
    props.event.kind === "warning"
      ? "border-[#f2dfc0] bg-white text-[#8b5e1a]"
      : props.event.kind === "gesture"
        ? "border-[#bfdbfe] bg-white text-[#1d4ed8]"
        : "border-[#e3e7f0] bg-white text-[#485062]";

  return (
    <article className={`rounded-[24px] border px-4 py-4 ${accent}`}>
      <div className="flex items-center justify-between gap-4">
        <strong className="text-[11px] font-semibold uppercase tracking-[0.22em]">
          {props.event.kind}
        </strong>
        <span className="text-xs">
          {new Date(props.event.timestampMs).toLocaleTimeString()}
        </span>
      </div>
      <p className="mt-3 text-sm leading-6">{props.event.message}</p>
    </article>
  );
}

function InlineNotice(props: {
  tone: "warning" | "accent";
  children: ReactNode;
}) {
  const toneClass =
    props.tone === "warning"
      ? "border-[#f2dfc0] bg-white text-[#8b5e1a]"
      : "border-[#bfdbfe] bg-white text-[#1d4ed8]";

  return <div className={`rounded-[24px] border px-4 py-4 text-sm font-medium ${toneClass}`}>{props.children}</div>;
}

function EmptyState(props: { title: string; body: string }) {
  return (
    <div className="rounded-[28px] border border-dashed border-[#d5dbea] bg-white p-8 text-center">
      <p className="text-base font-semibold text-[#10131a]">{props.title}</p>
      <p className="mx-auto mt-3 max-w-lg text-sm leading-6 text-[#656c7d]">{props.body}</p>
    </div>
  );
}

function EmptyStage(props: { title: string; body: string }) {
  return (
    <div className="flex min-h-[520px] items-center justify-center rounded-[32px] border border-dashed border-[#d5dbea] bg-white p-8">
      <EmptyState body={props.body} title={props.title} />
    </div>
  );
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

  return "Adjust the action for this control on the active profile. Changes save immediately and apply to the currently selected app profile.";
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
) {
  if (control === "hscroll_left") {
    const left = actionFor(profile, "hscroll_left", actionLookup);
    const right = actionFor(profile, "hscroll_right", actionLookup);
    return `Left: ${left} | Right: ${right}`;
  }

  if (control === "gesture_press") {
    const tap = actionFor(profile, "gesture_press", actionLookup);
    const swipeConfigured =
      ["gesture_left", "gesture_right", "gesture_up", "gesture_down"].some(
        (gestureControl) =>
          actionFor(profile, gestureControl as LogicalControl, actionLookup) !==
          "Do Nothing (Pass-through)",
      );
    return swipeConfigured ? `Tap: ${tap} | Swipes configured` : `Tap: ${tap}`;
  }

  return actionFor(profile, control, actionLookup);
}

function actionFor(
  profile: Profile,
  control: LogicalControl,
  actionLookup: Map<string, ActionDefinition>,
) {
  const actionId = bindingFor(profile, control).actionId;
  return actionLookup.get(actionId)?.label ?? "Do Nothing (Pass-through)";
}

function bindingFor(profile: Profile, control: LogicalControl): Binding {
  return (
    profile.bindings.find((binding) => binding.control === control) ??
    ({ control, actionId: "none" } satisfies Binding)
  );
}

function upsertBinding(profile: Profile, control: LogicalControl, actionId: string) {
  const target = profile.bindings.find((binding) => binding.control === control);
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
    profiles: config.profiles.map(cloneProfile),
    settings: {
      ...config.settings,
      deviceLayoutOverrides: { ...config.settings.deviceLayoutOverrides },
    },
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
    return layouts.find((layout) => layout.key === "generic_mouse") ?? layouts[0];
  }

  const overrideKey = config.settings.deviceLayoutOverrides[device.key];
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

export default App;
