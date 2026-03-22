import { commands, events, type Result } from "./bindings";
import type {
  DebugEvent,
  ImportLegacyConfigRequest,
  AppConfig,
} from "./bindings";

function unwrap<T, E>(result: Result<T, E>): T {
  if (result.status === "ok") {
    return result.data;
  }

  throw new Error(String(result.error));
}

export { events };
export type { DebugEvent };

export async function bootstrapLoad() {
  return unwrap(await commands.bootstrapLoad());
}

export async function configGet() {
  return unwrap(await commands.configGet());
}

export async function configSave(config: AppConfig) {
  return unwrap(await commands.configSave(config));
}

export async function appDiscoveryRefresh() {
  return unwrap(await commands.appDiscoveryRefresh());
}

export async function appIconLoad(sourcePath: string) {
  return unwrap(await commands.appIconLoad(sourcePath));
}

export async function devicesList() {
  return unwrap(await commands.devicesList());
}

export async function devicesAdd(modelKey: string) {
  return unwrap(await commands.devicesAdd(modelKey));
}

export async function devicesResetToFactory(deviceKey: string) {
  return unwrap(await commands.devicesResetToFactory(deviceKey));
}

export async function devicesRemove(deviceKey: string) {
  return unwrap(await commands.devicesRemove(deviceKey));
}

export async function devicesSelect(deviceKey: string) {
  return unwrap(await commands.devicesSelect(deviceKey));
}

export async function devicesSelectMock(deviceKey: string) {
  return unwrap(await commands.devicesSelectMock(deviceKey));
}

export async function importLegacyConfig(request: ImportLegacyConfigRequest) {
  return unwrap(await commands.importLegacyConfig(request));
}

export async function debugClearLog() {
  return unwrap(await commands.debugClearLog());
}
