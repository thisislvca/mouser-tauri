import { useEffect, useRef, useState, type ReactNode, type SVGProps } from "react";
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
    eyebrow: string;
    blurb: string;
    icon: (props: SVGProps<SVGSVGElement>) => ReactNode;
  }
> = {
  buttons: {
    label: "Buttons",
    eyebrow: "Assignments",
    blurb: "Map click, gesture, and thumb-wheel controls.",
    icon: ButtonsIcon,
  },
  devices: {
    label: "Point + Scroll",
    eyebrow: "Device",
    blurb: "Tune DPI, scroll direction, and layout detection.",
    icon: PointScrollIcon,
  },
  profiles: {
    label: "Profiles",
    eyebrow: "Routing",
    blurb: "Switch bindings when the frontmost app changes.",
    icon: ProfilesIcon,
  },
  settings: {
    label: "Settings",
    eyebrow: "Preferences",
    blurb: "Adjust startup, appearance, and runtime behavior.",
    icon: SettingsIcon,
  },
  debug: {
    label: "Debug",
    eyebrow: "Diagnostics",
    blurb: "Inspect backend state, logs, and legacy imports.",
    icon: DebugIcon,
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
      <main className="flex min-h-screen items-center justify-center bg-[radial-gradient(circle_at_top_left,_#ffffff,_#eef2f8_58%,_#e5ebf4)] px-8 text-[#13141b]">
        <div className="rounded-[28px] border border-white/70 bg-white/85 px-6 py-4 text-sm font-medium shadow-[0_28px_80px_rgba(15,23,42,0.12)]">
          Loading Mouser...
        </div>
      </main>
    );
  }

  if (bootstrapQuery.isError || !bootstrap) {
    return (
      <main className="flex min-h-screen items-center justify-center bg-[radial-gradient(circle_at_top_left,_#ffffff,_#eef2f8_58%,_#e5ebf4)] px-8 text-[#13141b]">
        <div className="max-w-xl rounded-[30px] border border-[#e4e7ef] bg-white/90 p-6 shadow-[0_28px_80px_rgba(15,23,42,0.12)]">
          <p className="text-sm font-semibold">Failed to load Mouser.</p>
          <pre className="mt-4 overflow-auto rounded-3xl bg-[#f5f7fb] p-4 text-xs text-[#5d6472]">
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
  const activeSectionMeta = SECTION_META[activeSection];

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

  return (
    <main className="min-h-screen bg-[radial-gradient(circle_at_top_left,_#ffffff,_#eef2f8_58%,_#e5ebf4)] px-4 py-4 text-[#11121a] antialiased sm:px-6 lg:px-8">
      <div className="mx-auto flex min-h-[calc(100vh-2rem)] max-w-[1720px] overflow-hidden rounded-[36px] border border-white/70 bg-[#fcfcfe]/92 shadow-[0_32px_120px_rgba(15,23,42,0.14)] backdrop-blur-xl">
        <aside className="flex w-[124px] shrink-0 flex-col justify-between border-r border-[#ececf4] bg-[#f6f7fb] px-4 py-5">
          <div className="space-y-6">
            <div className="flex h-16 w-16 items-center justify-center rounded-[26px] bg-[#111318] text-2xl font-semibold text-white shadow-[inset_0_1px_0_rgba(255,255,255,0.12)]">
              M
            </div>
            <nav className="space-y-2">
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
          </div>

          <div className="rounded-[28px] border border-[#e6e8f1] bg-white/90 p-3 shadow-[0_16px_34px_rgba(15,23,42,0.08)]">
            <div className="text-[10px] font-semibold uppercase tracking-[0.24em] text-[#7c8190]">
              Debug
            </div>
            <button
              className={[
                "mt-3 flex w-full items-center justify-center rounded-2xl px-3 py-2 text-sm font-semibold transition",
                config.settings.debugMode
                  ? "bg-[#5b4af4] text-white shadow-[0_14px_28px_rgba(91,74,244,0.28)]"
                  : "border border-[#e3e6ef] bg-[#f7f8fc] text-[#434958] hover:border-[#cfd4e2]",
              ].join(" ")}
              onClick={() =>
                saveSettings((nextConfig) => {
                  nextConfig.settings.debugMode = !config.settings.debugMode;
                })
              }
              type="button"
            >
              {config.settings.debugMode ? "On" : "Off"}
            </button>
          </div>
        </aside>

        <aside className="flex w-[340px] shrink-0 flex-col border-r border-[#ececf4] bg-white/72">
          <div className="border-b border-[#ececf4] px-6 py-6">
            <div className="flex items-center gap-3">
              <img alt="Mouser logo" className="h-11 w-11 rounded-2xl" src="/assets/logo_icon.png" />
              <div>
                <p className="text-[11px] font-semibold uppercase tracking-[0.24em] text-[#8a8fa0]">
                  Mouser
                </p>
                <h1 className="text-xl font-semibold tracking-[-0.03em] text-[#10131a]">
                  {activeDevice?.displayName ?? "No device"}
                </h1>
              </div>
            </div>

            <div className="mt-5 rounded-[30px] border border-[#ebeef4] bg-[#f8f9fc] p-4 shadow-[0_20px_40px_rgba(15,23,42,0.05)]">
              <div className="flex items-start justify-between gap-4">
                <div>
                  <p className="text-sm font-semibold text-[#151923]">
                    {selectedProfile.label}
                  </p>
                  <p className="mt-1 text-xs text-[#717787]">
                    {engineSnapshot.engineStatus.frontmostApp ?? "All applications"}
                  </p>
                </div>
                <StatusPill
                  tone={engineSnapshot.engineStatus.connected ? "success" : "neutral"}
                  value={engineSnapshot.engineStatus.connected ? "Connected" : "Waiting"}
                />
              </div>
              <div className="mt-4 flex flex-wrap gap-2">
                <CompactMetric
                  label="Battery"
                  value={activeDevice?.batteryLevel != null ? `${activeDevice.batteryLevel}%` : "N/A"}
                />
                <CompactMetric label="DPI" value={`${config.settings.dpi}`} />
                <CompactMetric
                  label="Transport"
                  value={activeDevice?.transport ?? platformCapabilities.activeHidBackend}
                />
              </div>
            </div>
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto px-5 py-5">
            <div className="flex items-center justify-between">
              <p className="text-[11px] font-semibold uppercase tracking-[0.24em] text-[#8a8fa0]">
                Application Profiles
              </p>
              <StatusPill
                tone={selectedProfile.id === config.activeProfileId ? "accent" : "neutral"}
                value={selectedProfile.id === config.activeProfileId ? "Active" : "Selected"}
              />
            </div>

            <div className="mt-4 space-y-3">
              {config.profiles.map((profile) => {
                const profileApp = profile.appMatchers[0]?.value
                  ? knownApps.find((app) => app.executable === profile.appMatchers[0]?.value) ?? null
                  : null;
                return (
                  <button
                    className={[
                      "w-full rounded-[26px] border px-4 py-4 text-left transition",
                      profile.id === selectedProfile.id
                        ? "border-[#d8dded] bg-white shadow-[0_16px_34px_rgba(15,23,42,0.08)]"
                        : "border-transparent bg-transparent hover:border-[#e8ebf4] hover:bg-white/80",
                    ].join(" ")}
                    key={profile.id}
                    onClick={() => setSelectedProfileId(profile.id)}
                    type="button"
                  >
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0">
                        <p className="truncate text-sm font-semibold text-[#10131a]">
                          {profile.label}
                        </p>
                        <p className="mt-1 truncate text-xs text-[#717787]">
                          {profile.appMatchers.map((matcher) => matcher.value).join(", ") || "All applications"}
                        </p>
                      </div>
                      {profileApp?.iconAsset ? (
                        <img
                          alt={profileApp.label}
                          className="h-10 w-10 rounded-2xl border border-[#e7eaf2] bg-white object-cover"
                          src={profileApp.iconAsset}
                        />
                      ) : (
                        <div className="flex h-10 w-10 items-center justify-center rounded-2xl border border-[#e7eaf2] bg-white text-xs font-semibold text-[#636a7a]">
                          {profile.label.slice(0, 1).toUpperCase()}
                        </div>
                      )}
                    </div>
                  </button>
                );
              })}
            </div>
          </div>

          <div className="border-t border-[#ececf4] p-5">
            <div className="rounded-[28px] border border-[#ebeef4] bg-[#f8f9fc] p-4 shadow-[0_18px_34px_rgba(15,23,42,0.05)]">
              <div className="flex items-center justify-between">
                <p className="text-[11px] font-semibold uppercase tracking-[0.24em] text-[#8a8fa0]">
                  Add Application
                </p>
                <button
                  className="rounded-full border border-[#d9def0] bg-white px-3 py-1 text-[11px] font-semibold text-[#4f5567] transition hover:border-[#c7cee2]"
                  onClick={() => setActiveSection("profiles")}
                  type="button"
                >
                  Open
                </button>
              </div>
              <input
                className="mt-4 w-full rounded-2xl border border-[#dfe3ef] bg-white px-4 py-3 text-sm text-[#121620] outline-none transition placeholder:text-[#9097a8] focus:border-[#5b4af4]"
                list="known-apps"
                placeholder="Known app executable"
                value={newProfileApp}
                onChange={(event) => setNewProfileApp(event.currentTarget.value)}
              />
              <datalist id="known-apps">
                {knownApps.map((app) => (
                  <option key={app.executable} value={app.executable}>
                    {app.label}
                  </option>
                ))}
              </datalist>
              <input
                className="mt-3 w-full rounded-2xl border border-[#dfe3ef] bg-white px-4 py-3 text-sm text-[#121620] outline-none transition placeholder:text-[#9097a8] focus:border-[#5b4af4]"
                placeholder="Optional custom label"
                value={newProfileLabel}
                onChange={(event) => setNewProfileLabel(event.currentTarget.value)}
              />
              <button
                className="mt-3 flex w-full items-center justify-center rounded-2xl bg-[#111318] px-4 py-3 text-sm font-semibold text-white transition hover:bg-[#07090d]"
                onClick={createProfileFromDraft}
                type="button"
              >
                Create profile
              </button>
            </div>
          </div>
        </aside>

        <section className="min-w-0 flex-1 overflow-y-auto bg-[linear-gradient(180deg,#ffffff_0%,#f6f7fb_100%)]">
          <header className="border-b border-[#ececf4] px-8 py-6">
            <div className="flex flex-col gap-5 xl:flex-row xl:items-end xl:justify-between">
              <div>
                <p className="text-[11px] font-semibold uppercase tracking-[0.26em] text-[#8a8fa0]">
                  {activeSectionMeta.eyebrow}
                </p>
                <h2 className="mt-3 text-[38px] font-semibold tracking-[-0.05em] text-[#10131a]">
                  {activeDevice?.displayName ?? "Mouser"}
                </h2>
                <p className="mt-2 text-sm text-[#62697a]">{activeSectionMeta.blurb}</p>
                <div className="mt-4 flex flex-wrap gap-2">
                  <InfoPill label="Profile" value={selectedProfile.label} />
                  <InfoPill
                    label="Frontmost"
                    value={engineSnapshot.engineStatus.frontmostApp ?? "None"}
                  />
                  <InfoPill label="HID" value={platformCapabilities.activeHidBackend} />
                  <InfoPill
                    label="Remapping"
                    value={platformCapabilities.mappingEngineReady ? "Live" : "Stub"}
                  />
                </div>
              </div>

              <div className="flex flex-wrap items-center gap-3">
                {isMutating && <StatusPill tone="accent" value="Applying" />}
                <button
                  className="rounded-full border border-[#d8dded] bg-white px-5 py-3 text-sm font-semibold text-[#171b24] transition hover:border-[#c8cee0]"
                  onClick={() => setActiveSection("profiles")}
                  type="button"
                >
                  + Add Application
                </button>
              </div>
            </div>
          </header>

          <div className="px-8 py-6">
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
              <SettingsView
                activeDevice={activeDevice}
                config={config}
                platformCapabilities={platformCapabilities}
                saveSettings={saveSettings}
              />
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
          </div>
        </section>
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
  return (
    <div className="space-y-6">
      {!props.mappingEngineReady && (
        <InlineNotice tone="warning">
          Live remapping is not wired into the Rust runtime yet. Mapping edits save correctly, but
          button interception is still stubbed.
        </InlineNotice>
      )}

      <div className="grid gap-6 xl:grid-cols-[minmax(0,1.1fr)_360px]">
        <StagePanel
          eyebrow="Device"
          title={props.activeLayout.label}
          subtitle={props.activeLayout.note || "Live layout with Logitech-style callouts."}
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
              body="Connect a supported mouse to inspect mapped controls."
              title="No device detected"
            />
          )}
        </StagePanel>

        <div className="space-y-4">
          <SummaryCard label="Selected profile" value={props.profile.label} />
          <SummaryCard
            label="Frontmost app"
            value={props.frontmostApp ?? "All applications"}
          />
          <SummaryCard
            label="HID backend"
            value={props.platformCapabilities.activeHidBackend}
          />
          <SummaryCard
            label="Mapping engine"
            value={props.mappingEngineReady ? "Live" : props.platformCapabilities.activeHookBackend}
          />
          <SummaryCard
            label="Battery"
            value={props.activeDevice?.batteryLevel != null ? `${props.activeDevice.batteryLevel}%` : "Unavailable"}
          />
        </div>
      </div>

      <Panel
        eyebrow="Assignments"
        title="Control mappings"
        subtitle="Each control stores a per-profile action. Gesture tap and swipe directions are independent."
      >
        <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
          {CONTROL_ORDER.map((control) => {
            const currentBinding =
              props.profile.bindings.find((binding) => binding.control === control) ??
              ({ control, actionId: "none" } satisfies Binding);

            return (
              <BindingCard
                control={control}
                currentBinding={currentBinding}
                groupedActions={props.groupedActions}
                key={control}
                onChange={(actionId) =>
                  props.updateSelectedProfile((nextProfile) => {
                    const target = nextProfile.bindings.find(
                      (binding) => binding.control === control,
                    );
                    if (target) {
                      target.actionId = actionId;
                    }
                  })
                }
              />
            );
          })}
        </div>
      </Panel>
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
                      ? "border-[#cfd5e6] bg-[#111318] text-white shadow-[0_18px_36px_rgba(15,23,42,0.16)]"
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
                            ? "text-white/70"
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
                    ? "border-[#d5dbeb] bg-white shadow-[0_16px_32px_rgba(15,23,42,0.08)]"
                    : "border-[#e7eaf2] bg-[#fbfbfd] hover:border-[#d4d9ea]",
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

          <div className="rounded-[24px] border border-[#e7eaf2] bg-[#f8f9fc] px-4 py-4">
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

        <div className="mt-5 rounded-[28px] border border-[#e7eaf2] bg-[#f8f9fc] p-3">
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
              className="flex w-full items-center justify-center rounded-2xl bg-[#111318] px-4 py-3 text-sm font-semibold text-white transition hover:bg-[#07090d]"
              data-testid="legacy-import-button"
              onClick={props.onImport}
              type="button"
            >
              Import legacy config
            </button>
            {props.importWarnings.length > 0 && (
              <ul className="space-y-2 rounded-[24px] border border-[#efd8af] bg-[#fff7e7] p-4 text-sm text-[#8b5f1b]">
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
        "flex w-full flex-col items-center justify-center gap-2 rounded-[24px] px-3 py-3 text-center transition",
        props.active
          ? "bg-white text-[#11131a] shadow-[0_16px_32px_rgba(15,23,42,0.12)]"
          : "text-[#6a7181] hover:bg-white/80 hover:text-[#11131a]",
      ].join(" ")}
      onClick={props.onClick}
      type="button"
    >
      <Icon className="h-5 w-5" />
      <span className="text-[11px] font-semibold leading-4">{props.label}</span>
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
    <section className="rounded-[32px] border border-[#e7eaf2] bg-white/88 p-6 shadow-[0_20px_48px_rgba(15,23,42,0.06)]">
      <p className="text-[11px] font-semibold uppercase tracking-[0.24em] text-[#8a8fa0]">
        {props.eyebrow}
      </p>
      <div className="mt-3 border-b border-[#eef1f6] pb-5">
        <h3 className="text-[28px] font-semibold tracking-[-0.04em] text-[#10131a]">
          {props.title}
        </h3>
        <p className="mt-2 text-sm text-[#656c7d]">{props.subtitle}</p>
      </div>
      <div className="pt-6">{props.children}</div>
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
    <section className="rounded-[34px] border border-[#e7eaf2] bg-white/92 p-6 shadow-[0_20px_48px_rgba(15,23,42,0.06)]">
      <p className="text-[11px] font-semibold uppercase tracking-[0.24em] text-[#8a8fa0]">
        {props.eyebrow}
      </p>
      <div className="mt-3 border-b border-[#eef1f6] pb-5">
        <h3 className="text-[30px] font-semibold tracking-[-0.05em] text-[#10131a]">
          {props.title}
        </h3>
        <p className="mt-2 text-sm text-[#656c7d]">{props.subtitle}</p>
      </div>
      <div className="pt-6">{props.children}</div>
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
    <label className="flex items-center justify-between rounded-[22px] border border-[#e7eaf2] bg-[#f8f9fc] px-4 py-4 text-sm font-medium text-[#171b24]">
      <span>{props.label}</span>
      <input
        checked={props.checked}
        className="h-4 w-4 accent-[#5b4af4]"
        onChange={(event) => props.onChange(event.currentTarget.checked)}
        type="checkbox"
      />
    </label>
  );
}

function BindingCard(props: {
  control: LogicalControl;
  currentBinding: Binding;
  groupedActions: Array<[string, ActionDefinition[]]>;
  onChange: (actionId: string) => void;
}) {
  return (
    <div className="rounded-[24px] border border-[#e7eaf2] bg-[#f8f9fc] p-4">
      <div className="mb-4">
        <p className="text-sm font-semibold text-[#10131a]">{CONTROL_LABELS[props.control]}</p>
        <p className="mt-1 text-xs uppercase tracking-[0.18em] text-[#8a8fa0]">
          {props.control.replace(/_/g, " ")}
        </p>
      </div>
      <select
        className="field-input"
        value={props.currentBinding.actionId}
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
      className="rounded-[32px] border border-[#edf0f6] bg-[radial-gradient(circle_at_top,_#ffffff,_#f7f8fc_60%,_#eef2f8)] p-6"
      data-testid="device-layout-card"
    >
      <div className="relative mx-auto min-h-[520px] w-full max-w-[900px]">
        <div
          className="relative mx-auto"
          style={{ width: props.layout.imageWidth, height: props.layout.imageHeight }}
        >
          <img
            alt={props.layout.label}
            className="absolute inset-0 h-full w-full object-contain drop-shadow-[0_42px_60px_rgba(15,23,42,0.18)]"
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
                  className="absolute z-20 max-w-[240px] rounded-[22px] border border-white/80 bg-white/92 px-4 py-3 shadow-[0_20px_36px_rgba(15,23,42,0.1)] backdrop-blur-md"
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

function SummaryCard(props: { label: string; value: string }) {
  return (
    <div className="rounded-[24px] border border-[#e7eaf2] bg-white/88 px-4 py-4 shadow-[0_12px_30px_rgba(15,23,42,0.05)]">
      <p className="text-[11px] font-semibold uppercase tracking-[0.22em] text-[#8a8fa0]">
        {props.label}
      </p>
      <p className="mt-2 text-sm font-semibold text-[#10131a]">{props.value}</p>
    </div>
  );
}

function CompactMetric(props: { label: string; value: string }) {
  return (
    <span className="rounded-full border border-[#e3e7f0] bg-white px-3 py-1.5 text-[11px] font-semibold text-[#4e5565]">
      <span className="text-[#959bad]">{props.label}</span> {props.value}
    </span>
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
      ? "border-[#cfe9da] bg-[#effaf3] text-[#177a4d]"
      : props.tone === "accent"
        ? "border-[#d9d4ff] bg-[#f1eeff] text-[#5b4af4]"
        : props.tone === "warning"
          ? "border-[#f3dfbe] bg-[#fff7e8] text-[#92611f]"
          : "border-[#e3e7f0] bg-white text-[#596071]";

  return (
    <span className={`rounded-full border px-3 py-1.5 text-xs font-semibold ${toneClass}`}>
      {props.value}
    </span>
  );
}

function CapabilityRow(props: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between rounded-[20px] border border-[#e7eaf2] bg-[#f8f9fc] px-4 py-3 text-sm">
      <span className="font-medium text-[#2f3441]">{props.label}</span>
      <span className="text-[#10131a]">{props.value}</span>
    </div>
  );
}

function LogEntry(props: { event: DebugEventRecord }) {
  const accent =
    props.event.kind === "warning"
      ? "border-[#f2dfc0] bg-[#fff8ea] text-[#8b5e1a]"
      : props.event.kind === "gesture"
        ? "border-[#ddd7ff] bg-[#f5f2ff] text-[#5849d2]"
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
      ? "border-[#f2dfc0] bg-[#fff8ea] text-[#8b5e1a]"
      : "border-[#ddd7ff] bg-[#f5f2ff] text-[#5849d2]";

  return <div className={`rounded-[24px] border px-4 py-4 text-sm font-medium ${toneClass}`}>{props.children}</div>;
}

function EmptyState(props: { title: string; body: string }) {
  return (
    <div className="rounded-[28px] border border-dashed border-[#d5dbea] bg-white/70 p-8 text-center">
      <p className="text-base font-semibold text-[#10131a]">{props.title}</p>
      <p className="mx-auto mt-3 max-w-lg text-sm leading-6 text-[#656c7d]">{props.body}</p>
    </div>
  );
}

function EmptyStage(props: { title: string; body: string }) {
  return (
    <div className="flex min-h-[520px] items-center justify-center rounded-[32px] border border-dashed border-[#d5dbea] bg-[radial-gradient(circle_at_top,_#ffffff,_#f5f7fb_68%,_#eef2f8)] p-8">
      <EmptyState body={props.body} title={props.title} />
    </div>
  );
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
  const actionId =
    profile.bindings.find((binding) => binding.control === control)?.actionId ?? "none";
  return actionLookup.get(actionId)?.label ?? "Do Nothing (Pass-through)";
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

function iconStrokeProps() {
  return {
    fill: "none",
    stroke: "currentColor",
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    strokeWidth: 1.8,
  };
}

function ButtonsIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg viewBox="0 0 24 24" {...props}>
      <path d="M7 5.5c0-1.9 1.6-3.5 3.5-3.5h3C15.4 2 17 3.6 17 5.5V18c0 2.2-1.8 4-4 4h-2c-2.2 0-4-1.8-4-4V5.5Z" {...iconStrokeProps()} />
      <path d="M12 2v7" {...iconStrokeProps()} />
    </svg>
  );
}

function PointScrollIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg viewBox="0 0 24 24" {...props}>
      <path d="M12 3v18" {...iconStrokeProps()} />
      <path d="M8 7.5c0-2.5 1.7-4.5 4-4.5s4 2 4 4.5V16c0 2.8-1.8 5-4 5s-4-2.2-4-5V7.5Z" {...iconStrokeProps()} />
      <path d="M4 9.5h2.5M17.5 14.5H20" {...iconStrokeProps()} />
    </svg>
  );
}

function ProfilesIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg viewBox="0 0 24 24" {...props}>
      <path d="M8 12a3 3 0 1 0 0-6 3 3 0 0 0 0 6Zm8 2a3 3 0 1 0 0-6 3 3 0 0 0 0 6Z" {...iconStrokeProps()} />
      <path d="M3 19.5c1.2-2.2 3.2-3.5 5.5-3.5s4.3 1.3 5.5 3.5M11 19.5c.9-1.4 2.3-2.3 4-2.3 1.8 0 3.4.9 4.5 2.3" {...iconStrokeProps()} />
    </svg>
  );
}

function SettingsIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg viewBox="0 0 24 24" {...props}>
      <path d="M12 8.5a3.5 3.5 0 1 0 0 7 3.5 3.5 0 0 0 0-7Z" {...iconStrokeProps()} />
      <path d="M19.4 15.1 21 16l-1.6 2.8-1.8-.6a7.9 7.9 0 0 1-1.7 1l-.3 1.9h-3.2l-.3-1.9a7.9 7.9 0 0 1-1.7-1l-1.8.6L3 16l1.6-.9a7.7 7.7 0 0 1 0-2.2L3 12l1.6-2.8 1.8.6c.5-.4 1.1-.7 1.7-1l.3-1.9h3.2l.3 1.9c.6.3 1.2.6 1.7 1l1.8-.6L21 12l-1.6.9c.2.7.2 1.5 0 2.2Z" {...iconStrokeProps()} />
    </svg>
  );
}

function DebugIcon(props: SVGProps<SVGSVGElement>) {
  return (
    <svg viewBox="0 0 24 24" {...props}>
      <path d="M12 4.5a6.5 6.5 0 0 0-6.5 6.5v2.5A4 4 0 0 0 9.5 17.5h5a4 4 0 0 0 4-4V11A6.5 6.5 0 0 0 12 4.5Z" {...iconStrokeProps()} />
      <path d="M9.5 4 8 2M14.5 4 16 2M4 11H2M22 11h-2M10 12.5h.01M14 12.5h.01M9.5 17.5 8 21h8l-1.5-3.5" {...iconStrokeProps()} />
    </svg>
  );
}

export default App;
