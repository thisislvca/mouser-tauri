use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use mouser_core::{normalize_bindings, AppConfig, Binding, KnownDeviceSpec, Profile};
use mouser_platform::{
    linux::{LinuxAppDiscoveryBackend, LinuxAppFocusBackend, LinuxHidBackend, LinuxHookBackend},
    macos::{MacOsAppDiscoveryBackend, MacOsAppFocusBackend, MacOsHidBackend, MacOsHookBackend},
    windows::{
        WindowsAppDiscoveryBackend, WindowsAppFocusBackend, WindowsHidBackend, WindowsHookBackend,
    },
    AppDiscoveryBackend, AppFocusBackend, ConfigStore, HidBackend, HookBackend,
};

pub(super) fn current_hid_backend() -> Arc<dyn HidBackend> {
    if cfg!(target_os = "macos") {
        Arc::new(MacOsHidBackend::new())
    } else if cfg!(target_os = "linux") {
        Arc::new(LinuxHidBackend::new())
    } else {
        Arc::new(WindowsHidBackend::new())
    }
}

pub(super) fn current_hook_backend() -> Arc<dyn HookBackend> {
    if cfg!(target_os = "macos") {
        Arc::new(MacOsHookBackend::new())
    } else if cfg!(target_os = "linux") {
        Arc::new(LinuxHookBackend::new())
    } else {
        Arc::new(WindowsHookBackend::new())
    }
}

pub(super) fn current_app_focus_backend() -> Arc<dyn AppFocusBackend> {
    if cfg!(target_os = "macos") {
        Arc::new(MacOsAppFocusBackend)
    } else if cfg!(target_os = "linux") {
        Arc::new(LinuxAppFocusBackend)
    } else {
        Arc::new(WindowsAppFocusBackend)
    }
}

pub(super) fn current_app_discovery_backend() -> Box<dyn AppDiscoveryBackend> {
    if cfg!(target_os = "macos") {
        Box::new(MacOsAppDiscoveryBackend)
    } else if cfg!(target_os = "linux") {
        Box::new(LinuxAppDiscoveryBackend)
    } else {
        Box::new(WindowsAppDiscoveryBackend)
    }
}

pub(super) fn load_config_with_recovery(
    config_store: &dyn ConfigStore,
) -> (AppConfig, Option<String>) {
    config_store.load_or_recover()
}

pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

pub(super) fn model_default_profile_id(model_key: &str) -> String {
    format!("device_{model_key}")
}

pub(super) fn model_default_profile(spec: &KnownDeviceSpec) -> Profile {
    let bindings = normalize_bindings(
        spec.controls
            .iter()
            .map(|control| Binding {
                control: control.control,
                action_id: control.default_action_id.clone(),
            })
            .collect(),
    );

    Profile {
        id: model_default_profile_id(&spec.key),
        label: format!("{} Defaults", spec.display_name),
        app_matchers: Vec::new(),
        bindings,
    }
}
