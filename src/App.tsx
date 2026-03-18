import {
  useEffect,
  useRef,
  useState,
  type ComponentProps,
  type ReactNode,
  type RefObject,
  type SVGProps,
} from "react";
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
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "./components/ui/card";
import { Input } from "./components/ui/input";
import { ScrollArea } from "./components/ui/scroll-area";
import { Select, type SelectOption } from "./components/ui/select";
import { Switch } from "./components/ui/switch";
import { Textarea } from "./components/ui/textarea";
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
import { cn } from "./lib/utils";
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
      <main className="flex min-h-screen items-center justify-center bg-[var(--app-bg)] px-8 text-[var(--foreground)]">
        <Card className="px-6 py-4">
          Loading Mouser...
        </Card>
      </main>
    );
  }

  if (bootstrapQuery.isError || !bootstrap) {
    return (
      <main className="flex min-h-screen items-center justify-center bg-[var(--app-bg)] px-8 text-[var(--foreground)]">
        <Card className="max-w-xl p-6">
          <p className="text-sm font-semibold">Failed to load Mouser.</p>
          <pre className="mt-4 overflow-auto rounded-3xl border border-[var(--border)] bg-white p-4 text-xs text-[var(--muted-foreground)]">
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
  const batteryLabel =
    activeDevice?.batteryLevel != null ? `${activeDevice.batteryLevel}%` : "N/A";

  return (
    <main className="min-h-screen bg-[var(--app-bg)] text-[var(--foreground)] antialiased">
      <div className="min-h-screen lg:pl-[18rem]">
        <aside className="hidden lg:fixed lg:inset-y-0 lg:left-0 lg:flex lg:w-[18rem] lg:flex-col lg:bg-[var(--sidebar)] lg:px-5 lg:py-6">
          <div className="px-3 pb-6 pt-3">
            <p className="text-[13px] font-semibold uppercase tracking-[0.28em] text-[var(--sidebar-foreground)]">
              Mouser
            </p>
          </div>

          <nav className="space-y-1.5">
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
            <Card className="bg-[var(--sidebar-surface)]">
              <CardContent className="flex items-center justify-between px-4 py-4">
                <div>
                  <p className="text-xs font-semibold uppercase tracking-[0.22em] text-[var(--muted-foreground)]">
                    Battery
                  </p>
                  <p className="mt-1 text-sm font-semibold text-[var(--foreground)]">{batteryLabel}</p>
                </div>
                <StatusPill tone="success" value="Connected" />
              </CardContent>
            </Card>
          </div>
        </aside>

        <div className="min-h-screen p-3 sm:p-4 lg:p-5">
          <div className="flex min-h-[calc(100vh-1.5rem)] flex-col overflow-hidden rounded-[32px] bg-[var(--surface)] shadow-[0_36px_120px_rgba(15,23,42,0.10)] ring-1 ring-[var(--border-soft)] sm:min-h-[calc(100vh-2rem)]">
            <header className="border-b border-[var(--border)] px-5 py-5 sm:px-8">
              <div className="flex flex-wrap items-center justify-between gap-4">
              <div className="flex min-w-0 items-center gap-3 sm:gap-4">
                <Button aria-label="Back" size="icon" variant="ghost">
                  <CaretLeft className="h-5 w-5" />
                </Button>
                <h2 className="truncate text-[24px] font-semibold tracking-[-0.05em] text-[var(--foreground)]">
                  {shellTitle}
                </h2>
              </div>

                <div className="flex shrink-0 items-center gap-3">
                  {isMutating && <StatusPill tone="accent" value="Applying" />}
                  <Button onClick={() => setActiveSection("profiles")} variant="outline">
                    + Add application
                  </Button>
                </div>
              </div>

              <div className="mt-5 lg:hidden">
                <ScrollArea className="w-full whitespace-nowrap">
                  <div className="flex gap-2 pb-1">
                    {SECTION_ORDER.map((section) => (
                      <SectionNavButton
                        active={activeSection === section}
                        compact
                        icon={SECTION_META[section].icon}
                        key={section}
                        label={SECTION_META[section].label}
                        onClick={() => setActiveSection(section)}
                      />
                    ))}
                  </div>
                </ScrollArea>
              </div>
            </header>

            <div className="min-h-0 flex-1">
              <ScrollArea className="h-full">
                <section className="min-w-0 px-5 py-6 sm:px-8 sm:py-8">
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
                      <Card>
                        <CardHeader className="grid gap-4 pb-0 md:grid-cols-[minmax(0,1fr)_220px] md:items-end">
                          <div>
                            <CardTitle className="text-[26px]">New Profile</CardTitle>
                          </div>
                          <div className="md:justify-self-end">
                            <Button className="w-full md:w-auto" onClick={createProfileFromDraft}>
                              Create profile
                            </Button>
                          </div>
                        </CardHeader>
                        <CardContent className="pt-6">
                          <div className="grid gap-3 md:grid-cols-2">
                            <Input
                              list="known-apps"
                              placeholder="Known app executable"
                              value={newProfileApp}
                              onChange={(event) => setNewProfileApp(event.currentTarget.value)}
                            />
                            <Input
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
                        </CardContent>
                      </Card>

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
              </ScrollArea>
            </div>
          </div>
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
        <div className="grid min-h-[760px] gap-6 xl:grid-cols-[minmax(0,1fr)_380px] xl:gap-8">
          <motion.div layout className="min-w-0" transition={BUTTONS_SHEET_TRANSITION}>
            <StagePanel title={props.activeDevice.displayName}>
              <ButtonsWorkbench
                actionLookup={props.actionLookup}
                activeDevice={props.activeDevice}
                layout={props.activeLayout}
                profile={props.profile}
                selectedControl={selectedControl}
                onSelectControl={setSelectedControl}
              />
            </StagePanel>
          </motion.div>

          <AnimatePresence initial={false}>
            {selectedHotspot ? (
              <motion.aside
                animate={{ opacity: 1, x: 0 }}
                className="min-w-0 xl:border-l xl:border-[var(--border)] xl:pl-8"
                exit={{ opacity: 0, x: 24 }}
                initial={{ opacity: 0, x: 24 }}
                key={selectedHotspot.control}
                transition={{ duration: 0.22, ease: [0.22, 1, 0.36, 1] }}
              >
                <ButtonsControlSheet
                  actionLookup={props.actionLookup}
                  control={selectedHotspot.control}
                  groupedActions={props.groupedActions}
                  mappingEngineReady={props.mappingEngineReady}
                  platformCapabilities={props.platformCapabilities}
                  profile={props.profile}
                  setBinding={setBinding}
                  onClose={() => setSelectedControl(null)}
                />
              </motion.aside>
            ) : (
              <motion.aside
                animate={{ opacity: 1 }}
                className="hidden xl:block xl:border-l xl:border-[var(--border)] xl:pl-8"
                initial={{ opacity: 0 }}
              >
                <Card className="h-full bg-[var(--card-muted)]">
                  <CardHeader className="flex h-full items-center justify-center">
                    <CardTitle className="text-[22px]">Select a control</CardTitle>
                  </CardHeader>
                </Card>
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
  const layoutOptions = props.layoutChoices.map(
    (choice) =>
      ({
        label: choice.label,
        value: choice.key,
      }) satisfies SelectOption,
  );

  return (
    <div className="space-y-6">
      <div className="grid gap-6 xl:grid-cols-[minmax(0,1.1fr)_380px]">
        <StagePanel
          title={props.activeDevice?.displayName ?? "Point and scroll"}
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
          title="Tuning"
        >
          <div className="space-y-4">
            <Field label="DPI">
              <Input
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
              <Select
                ariaLabel="Manual layout override"
                options={layoutOptions}
                value={props.activeDevice ? props.config.settings.deviceLayoutOverrides[props.activeDevice.key] ?? "" : ""}
                onValueChange={(value) =>
                  props.saveSettings((nextConfig) => {
                    if (!props.activeDevice) {
                      return;
                    }

                    if (value) {
                      nextConfig.settings.deviceLayoutOverrides[props.activeDevice.key] =
                        value;
                    } else {
                      delete nextConfig.settings.deviceLayoutOverrides[props.activeDevice.key];
                    }
                  })
                }
              />
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
              <Input
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
              <Input
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
          title="Devices"
        >
          <div className="space-y-3">
            {props.engineSnapshot.devices.length > 0 ? (
              props.engineSnapshot.devices.map((device) => (
                <button
                  className={[
                    "w-full rounded-[24px] px-4 py-4 text-left transition ring-1",
                    device.key === props.engineSnapshot.activeDeviceKey
                      ? "bg-[var(--card)] text-[var(--foreground)] ring-[#c3d8fb] shadow-[0_16px_34px_rgba(37,99,235,0.10)]"
                      : "bg-[var(--card-muted)] ring-[var(--border)] hover:bg-[var(--card)]",
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
          title="Runtime"
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
        title="Profiles"
      >
        <div className="space-y-3">
          {props.profiles.map((profile) => {
            const profileApp = profile.appMatchers[0]?.value
              ? props.knownApps.find((app) => app.executable === profile.appMatchers[0]?.value) ?? null
              : null;
            return (
              <button
                className={[
                  "flex w-full items-center justify-between gap-4 rounded-[24px] px-4 py-4 text-left transition ring-1",
                  profile.id === props.profile.id
                    ? "bg-[var(--card)] shadow-[0_16px_34px_rgba(37,99,235,0.10)] ring-[#c3d8fb]"
                    : "bg-[var(--card-muted)] ring-[var(--border)] hover:bg-[var(--card)]",
                ].join(" ")}
                key={profile.id}
                onClick={() => props.setSelectedProfileId(profile.id)}
                type="button"
              >
                <div className="min-w-0">
                  <p className="truncate text-sm font-semibold text-[var(--foreground)]">
                    {profile.label}
                  </p>
                  <p className="mt-1 truncate text-xs text-[var(--muted-foreground)]">
                    {profile.appMatchers.map((matcher) => matcher.value).join(", ") || "All applications"}
                  </p>
                </div>
                {profileApp?.iconAsset ? (
                  <img
                    alt={profileApp.label}
                    className="h-11 w-11 rounded-2xl border border-[var(--border)] bg-white object-cover"
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
        title="Profile"
      >
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

          <Field label="Executable matchers">
            <Textarea
              className="min-h-[180px] resize-y"
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

          <Card className="bg-[var(--card-muted)] shadow-none ring-1 ring-[var(--border)]">
            <CardContent className="px-4 py-4">
              <p className="text-[11px] font-semibold uppercase tracking-[0.24em] text-[var(--muted-foreground)]">
              Current selection
              </p>
              <p
                className="mt-3 text-sm font-semibold text-[var(--foreground)]"
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

function SettingsView(props: {
  config: AppConfig;
  activeDevice: DeviceInfo | null;
  platformCapabilities: BootstrapPayload["platformCapabilities"];
  saveSettings: (mutateConfig: (nextConfig: AppConfig) => void) => void;
}) {
  const appearanceOptions = [
    { label: "System", value: "system" },
    { label: "Light", value: "light" },
    { label: "Dark", value: "dark" },
  ] satisfies SelectOption[];

  return (
    <div className="grid gap-6 xl:grid-cols-[minmax(0,1fr)_340px]">
      <Panel
        title="Settings"
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
            <Select
              ariaLabel="Appearance mode"
              options={appearanceOptions}
              value={props.config.settings.appearanceMode}
              onValueChange={(value) =>
                props.saveSettings((nextConfig) => {
                  nextConfig.settings.appearanceMode =
                    value as AppConfig["settings"]["appearanceMode"];
                })
              }
            />
          </Field>
          <Field label="Gesture timeout (ms)">
            <Input
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
            <Input
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
        title="Status"
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
        title="Log"
      >
        <div className="flex flex-wrap items-center gap-3">
          <StatusPill
            tone={props.config.settings.debugMode ? "accent" : "neutral"}
            value={props.config.settings.debugMode ? "Debug on" : "Debug off"}
          />
          <Button
            disabled={props.isClearing}
            onClick={props.clearDebugLog}
            variant="outline"
          >
            Clear log
          </Button>
        </div>

        <div className="mt-5 grid gap-3 md:grid-cols-2">
          <CapabilityRow label="Active HID backend" value={props.platformCapabilities.activeHidBackend} />
          <CapabilityRow label="Active hook backend" value={props.platformCapabilities.activeHookBackend} />
          <CapabilityRow label="Active focus backend" value={props.platformCapabilities.activeFocusBackend} />
          <CapabilityRow label="iokit backend" value={props.platformCapabilities.iokitAvailable ? "Ready" : "Not ported"} />
        </div>

        <div className="mt-5 rounded-[28px] bg-[var(--card-muted)] p-3 ring-1 ring-[var(--border)]">
          <ScrollArea className="max-h-[560px] pr-1">
            <div className="space-y-3">
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
          </ScrollArea>
        </div>
      </Panel>

      <div className="space-y-6">
        <Panel
          title="Debug"
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
          title="Import"
        >
          <div className="space-y-4">
            <Field label="Optional source path">
              <Input
                placeholder="~/Library/Application Support/Mouser/config.json"
                value={props.importSourcePath}
                onChange={(event) => props.setImportSourcePath(event.currentTarget.value)}
              />
            </Field>
            <Field label="Legacy Mouser JSON">
              <Textarea
                className="min-h-[280px] resize-y font-mono text-xs leading-6"
                data-testid="legacy-import-input"
                rows={12}
                value={props.importDraft}
                onChange={(event) => props.setImportDraft(event.currentTarget.value)}
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
      aria-label={props.compact ? `${props.label} quick nav` : props.label}
      className={cn(
        "flex items-center gap-3 rounded-[20px] px-4 py-3 text-left transition-all duration-200",
        props.compact ? "shrink-0 whitespace-nowrap" : "w-full",
        props.active
          ? "bg-[var(--accent)] text-white shadow-[0_18px_36px_rgba(37,99,235,0.22)]"
          : "bg-transparent text-[var(--sidebar-foreground)] hover:bg-[var(--sidebar-surface)]",
      )}
      onClick={props.onClick}
      type="button"
    >
      <Icon className="h-5 w-5" />
      <span className="text-sm font-semibold">{props.label}</span>
    </button>
  );
}

function Panel(props: { title: string; subtitle?: string; children: ReactNode }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle>{props.title}</CardTitle>
        {props.subtitle ? <CardDescription>{props.subtitle}</CardDescription> : null}
      </CardHeader>
      <CardContent>{props.children}</CardContent>
    </Card>
  );
}

function StagePanel(props: { title: string; subtitle?: string; children: ReactNode }) {
  return (
    <Card className="overflow-hidden bg-[var(--card-muted)]">
      <CardHeader className="pb-0">
        <CardTitle className="text-[28px]">{props.title}</CardTitle>
        {props.subtitle ? <CardDescription>{props.subtitle}</CardDescription> : null}
      </CardHeader>
      <CardContent className="pt-6">{props.children}</CardContent>
    </Card>
  );
}

function Field(props: { label: string; children: ReactNode }) {
  return (
    <label className="block">
      <span className="mb-2.5 block text-sm font-medium text-[var(--foreground)]">{props.label}</span>
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
    <label className="flex items-center justify-between rounded-[24px] bg-[var(--card-muted)] px-4 py-4 text-sm font-medium text-[var(--foreground)] ring-1 ring-[var(--border)]">
      <span>{props.label}</span>
      <Switch checked={props.checked} onCheckedChange={props.onChange} />
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
  const workbenchRef = useRef<HTMLDivElement | null>(null);
  const workbenchSize = useElementSize(workbenchRef);
  const minCanvasWidth = props.layout.imageWidth + 220;
  const maxCanvasWidth = props.layout.imageWidth + 520;
  const canvasWidth = clamp(
    workbenchSize.width || props.layout.imageWidth + 360,
    minCanvasWidth,
    maxCanvasWidth,
  );
  const canvasHeight = props.layout.imageHeight + 320;
  const imageLeft = (canvasWidth - props.layout.imageWidth) / 2;
  const imageTop = 124;

  return (
    <section className="relative rounded-[30px] bg-white/90 p-3 shadow-[inset_0_1px_0_rgba(255,255,255,0.65)] ring-1 ring-[var(--border)]">
      <div
        className="relative overflow-x-auto rounded-[26px] bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.95),rgba(241,244,250,0.88)_42%,rgba(236,240,246,0.74))]"
        ref={workbenchRef}
      >
        <div className="relative mx-auto min-h-[720px] min-w-full px-4 py-6 sm:px-8">
          <div className="relative mx-auto" style={{ width: canvasWidth, height: canvasHeight }}>
            <div
              className="absolute inset-x-8 inset-y-10 rounded-[40px] border border-white/70 bg-white/50 blur-0"
              aria-hidden="true"
            />
            <img
              alt={props.layout.label}
              className="absolute object-contain drop-shadow-[0_28px_44px_rgba(15,23,42,0.16)]"
              data-testid="device-layout-image"
              src={props.layout.imageAsset}
              style={{
                height: props.layout.imageHeight,
                left: imageLeft,
                top: imageTop,
                width: props.layout.imageWidth,
              }}
            />

            {props.layout.hotspots.map((hotspot) => {
              const isSelected = props.selectedControl === hotspot.control;
              const summary = stageHotspotSummary(props.profile, hotspot.control, props.actionLookup);
              const pointX = imageLeft + hotspot.normX * props.layout.imageWidth;
              const pointY = imageTop + hotspot.normY * props.layout.imageHeight;
              const cardMetrics = stageCardMetrics(hotspot.control);
              const rawLabelX = pointX + hotspot.labelOffX;
              const rawLabelY = pointY + hotspot.labelOffY;
              const labelX = clamp(rawLabelX, 20, canvasWidth - cardMetrics.width - 20);
              const labelY = clamp(rawLabelY, 28, canvasHeight - cardMetrics.height - 28);
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
                        ? "border-[#d7e6ff] bg-[var(--accent)] shadow-[0_0_0_10px_rgba(37,99,235,0.14)]"
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
                      "absolute z-20 rounded-[24px] px-5 py-4 text-left transition ring-1",
                      isSelected
                        ? "bg-white shadow-[0_18px_30px_rgba(37,99,235,0.14)] ring-[var(--accent)]"
                        : "bg-white/90 shadow-[0_14px_24px_rgba(15,23,42,0.08)] ring-[var(--border)] hover:bg-white hover:ring-[var(--border-strong)]",
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
                    <p className="text-[14px] font-semibold tracking-[-0.02em] text-[var(--foreground)]">
                      {hotspot.label}
                    </p>
                    <p className="mt-2 text-[12px] leading-5 text-[var(--muted-foreground)]">
                      {summary}
                    </p>
                  </button>
                </div>
              );
            })}
          </div>
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
  mappingEngineReady: boolean;
  platformCapabilities: BootstrapPayload["platformCapabilities"];
  setBinding: (control: LogicalControl, actionId: string) => void;
  onClose: () => void;
}) {
  const controls = editorControlsFor(props.control);
  const title = editorTitleFor(props.control);
  const description = editorDescriptionFor(props.control);
  const gestureControl = props.control.startsWith("gesture_");
  const note =
    gestureControl && !props.platformCapabilities.gestureDiversionAvailable
      ? props.platformCapabilities.platform === "macos"
        ? "Gesture remapping will appear when the Logitech gesture channel connects."
        : "Gesture remapping is unavailable on this platform."
      : !props.mappingEngineReady
        ? "Live remapping is unavailable because the macOS event tap did not start."
        : null;

  return (
    <section
      className="flex h-full min-h-[520px] flex-col pt-2"
      data-testid="buttons-editor-sheet"
    >
      <div className="flex items-start justify-between gap-4 pb-3">
        <div>
          <h3 className="text-[34px] font-semibold tracking-[-0.05em] text-[var(--foreground)]">
            {title}
          </h3>
          {note ? (
            <p className="mt-3 max-w-sm text-sm leading-7 text-[var(--muted-foreground)]">{note}</p>
          ) : (
            <p className="mt-3 text-sm leading-7 text-[var(--muted-foreground)]">{description}</p>
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

      <Card className="mt-2 bg-[var(--card-muted)] shadow-none ring-1 ring-[var(--border)]">
        <CardContent className="px-5 py-4">
          <p className="text-base leading-8 text-[var(--foreground)]">
            {summarizeHotspot(props.profile, props.control, props.actionLookup)}
          </p>
        </CardContent>
      </Card>

      <ScrollArea className="mt-5 flex-1 pr-1">
        <div className="space-y-4">
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
  groupedActions: Array<[string, ActionDefinition[]]>;
  onChange: (actionId: string) => void;
}) {
  const currentBinding = bindingFor(props.profile, props.control);
  const actionOptions = props.groupedActions.flatMap(([group, actions]) =>
    actions.map(
      (action) =>
        ({
          group,
          label: action.label,
          value: action.id,
        }) satisfies SelectOption,
    ),
  );

  return (
    <Card className="bg-[var(--card)]">
      <CardContent className="p-4">
        <div className="flex items-start justify-between gap-3">
          <p className="text-sm font-semibold text-[var(--foreground)]">{props.label}</p>
          <Badge variant="default">
            {actionFor(props.profile, props.control, props.actionLookup)}
          </Badge>
        </div>
        <Select
          ariaLabel={props.label}
          className="mt-4"
          onValueChange={props.onChange}
          options={actionOptions}
          value={currentBinding.actionId}
        />
      </CardContent>
    </Card>
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
      className="rounded-[28px] bg-white p-6 ring-1 ring-[var(--border)]"
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
                  className="absolute z-20 max-w-[240px] rounded-[22px] bg-white px-4 py-3 shadow-[0_12px_24px_rgba(15,23,42,0.08)] ring-1 ring-[var(--border)]"
                  style={{ left: labelX, top: labelY }}
                >
                  <p className="text-sm font-semibold text-[var(--foreground)]">{hotspot.label}</p>
                  <p className="mt-1 text-xs leading-5 text-[var(--muted-foreground)]">{summary}</p>
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

function StatusPill(props: {
  tone: "success" | "accent" | "neutral" | "warning";
  value: string;
}) {
  const toneClass: ComponentProps<typeof Badge>["variant"] =
    props.tone === "success"
      ? "success"
      : props.tone === "accent"
        ? "accent"
        : props.tone === "warning"
          ? "warning"
          : "default";

  return <Badge variant={toneClass}>{props.value}</Badge>;
}

function CapabilityRow(props: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between rounded-[20px] bg-[var(--card-muted)] px-4 py-3 text-sm ring-1 ring-[var(--border)]">
      <span className="font-medium text-[var(--foreground)]">{props.label}</span>
      <span className="text-[var(--foreground)]">{props.value}</span>
    </div>
  );
}

function LogEntry(props: { event: DebugEventRecord }) {
  const accent =
    props.event.kind === "warning"
      ? "border-[#f2dfc0] bg-[#fff9ef] text-[#8b5e1a]"
      : props.event.kind === "gesture"
        ? "border-[#bfdbfe] bg-[#f2f7ff] text-[#1d4ed8]"
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

function EmptyState(props: { title: string; body: string }) {
  return (
    <div className="rounded-[28px] border border-dashed border-[var(--border-strong)] bg-[var(--card-muted)] p-8 text-center">
      <p className="text-base font-semibold text-[var(--foreground)]">{props.title}</p>
      <p className="mx-auto mt-3 max-w-lg text-sm leading-6 text-[var(--muted-foreground)]">{props.body}</p>
    </div>
  );
}

function EmptyStage(props: { title: string; body: string }) {
  return (
    <div className="flex min-h-[520px] items-center justify-center rounded-[32px] border border-dashed border-[var(--border-strong)] bg-[var(--card)] p-8">
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

function clamp(value: number, min: number, max: number) {
  return Math.min(Math.max(value, min), max);
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

  return "Adjust the action for this control on the active profile. Changes save immediately and apply to the currently selected app profile.";
}

function stageHotspotSummary(
  profile: Profile,
  control: LogicalControl,
  actionLookup: Map<string, ActionDefinition>,
) {
  if (control === "hscroll_left") {
    const left = compactStageActionLabel(actionFor(profile, "hscroll_left", actionLookup));
    const right = compactStageActionLabel(actionFor(profile, "hscroll_right", actionLookup));
    return `L ${left} / R ${right}`;
  }

  if (control === "gesture_press") {
    const tap = compactStageActionLabel(actionFor(profile, "gesture_press", actionLookup));
    const swipeConfigured =
      ["gesture_left", "gesture_right", "gesture_up", "gesture_down"].some(
        (gestureControl) =>
          actionFor(profile, gestureControl as LogicalControl, actionLookup) !==
          "Do Nothing (Pass-through)",
      );
    return swipeConfigured ? `Tap ${tap} + swipes` : `Tap ${tap}`;
  }

  return compactStageActionLabel(actionFor(profile, control, actionLookup));
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
