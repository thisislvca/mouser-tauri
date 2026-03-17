import { invoke } from "@tauri-apps/api/core";
import type {
  AppConfig,
  BootstrapPayload,
  DeviceInfo,
  EngineSnapshot,
  ImportLegacyRequest,
  LegacyImportReport,
  Profile,
} from "./types";

export function bootstrapLoad() {
  return invoke<BootstrapPayload>("bootstrap_load");
}

export function configGet() {
  return invoke<AppConfig>("config_get");
}

export function configSave(config: AppConfig) {
  return invoke<BootstrapPayload>("config_save", { config });
}

export function profilesCreate(profile: Profile) {
  return invoke<BootstrapPayload>("profiles_create", { profile });
}

export function profilesUpdate(profile: Profile) {
  return invoke<BootstrapPayload>("profiles_update", { profile });
}

export function profilesDelete(profileId: string) {
  return invoke<BootstrapPayload>("profiles_delete", { profile_id: profileId });
}

export function devicesList() {
  return invoke<DeviceInfo[]>("devices_list");
}

export function devicesSelectMock(deviceKey: string) {
  return invoke<EngineSnapshot>("devices_select_mock", { device_key: deviceKey });
}

export function importLegacyConfig(request: ImportLegacyRequest) {
  return invoke<LegacyImportReport>("import_legacy_config", { request });
}
