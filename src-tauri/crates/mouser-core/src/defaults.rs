use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

use crate::{
    ActionDefinition, AppConfig, AppDiscoverySnapshot, AppMatcherKind, Binding, CatalogApp,
    DebugLogGroups, DeviceSettings, KnownApp, LayoutChoice, LogicalControl, Profile, Settings,
};

pub fn default_settings() -> Settings {
    Settings {
        start_minimized: true,
        start_at_login: false,
        appearance_mode: crate::AppearanceMode::System,
        debug_mode: false,
        debug_log_groups: default_debug_log_groups(),
    }
}

pub fn default_debug_log_groups() -> DebugLogGroups {
    DebugLogGroups {
        runtime: true,
        hook_routing: false,
        gestures: false,
        thumb_wheel: false,
        hid: false,
    }
}

pub fn default_macos_thumb_wheel_trackpad_hold_timeout_ms() -> u32 {
    500
}

pub fn default_device_settings() -> DeviceSettings {
    DeviceSettings {
        dpi: 1000,
        invert_horizontal_scroll: false,
        invert_vertical_scroll: false,
        macos_thumb_wheel_simulate_trackpad: false,
        macos_thumb_wheel_trackpad_hold_timeout_ms:
            default_macos_thumb_wheel_trackpad_hold_timeout_ms(),
        gesture_threshold: 50,
        gesture_deadzone: 40,
        gesture_timeout_ms: 3000,
        gesture_cooldown_ms: 500,
        manual_layout_override: None,
    }
}

pub fn default_app_discovery_snapshot() -> AppDiscoverySnapshot {
    AppDiscoverySnapshot {
        suggested_apps: Vec::new(),
        browse_apps: Vec::new(),
        last_scan_at_ms: None,
        scanning: false,
    }
}

pub fn default_profile_bindings() -> Vec<Binding> {
    normalize_bindings(vec![
        Binding {
            control: LogicalControl::Back,
            action_id: "browser_back".to_string(),
        },
        Binding {
            control: LogicalControl::Forward,
            action_id: "browser_forward".to_string(),
        },
        Binding {
            control: LogicalControl::HscrollLeft,
            action_id: "browser_back".to_string(),
        },
        Binding {
            control: LogicalControl::HscrollRight,
            action_id: "browser_forward".to_string(),
        },
    ])
}

pub fn legacy_default_profile_bindings_v3() -> Vec<Binding> {
    normalize_bindings(vec![
        Binding {
            control: LogicalControl::Back,
            action_id: "alt_tab".to_string(),
        },
        Binding {
            control: LogicalControl::Forward,
            action_id: "alt_tab".to_string(),
        },
        Binding {
            control: LogicalControl::HscrollLeft,
            action_id: "browser_back".to_string(),
        },
        Binding {
            control: LogicalControl::HscrollRight,
            action_id: "browser_forward".to_string(),
        },
    ])
}

pub fn default_profile() -> Profile {
    Profile {
        id: "default".to_string(),
        label: "Default (All Apps)".to_string(),
        app_matchers: Vec::new(),
        bindings: default_profile_bindings(),
    }
}

pub fn default_config() -> AppConfig {
    AppConfig {
        version: 4,
        active_profile_id: "default".to_string(),
        profiles: vec![default_profile()],
        managed_devices: Vec::new(),
        settings: default_settings(),
        device_defaults: default_device_settings(),
    }
}

pub fn normalize_bindings(bindings: Vec<Binding>) -> Vec<Binding> {
    let mut map = BTreeMap::new();
    for binding in bindings {
        map.insert(binding.control, binding.action_id);
    }

    LogicalControl::all()
        .into_iter()
        .map(|control| Binding {
            control,
            action_id: map.remove(&control).unwrap_or_else(|| "none".to_string()),
        })
        .collect()
}

pub fn profile_for_supported_controls(
    profile: &Profile,
    supported_controls: &[LogicalControl],
) -> Profile {
    let supported = supported_controls.iter().copied().collect::<BTreeSet<_>>();
    let bindings = normalize_bindings(
        profile
            .bindings
            .iter()
            .filter(|binding| supported.contains(&binding.control))
            .cloned()
            .collect(),
    );

    Profile {
        id: profile.id.clone(),
        label: profile.label.clone(),
        app_matchers: profile.app_matchers.clone(),
        bindings,
    }
}

pub fn default_action_catalog() -> Vec<ActionDefinition> {
    default_action_catalog_ref().to_vec()
}

pub fn default_action_catalog_ref() -> &'static [ActionDefinition] {
    static ACTIONS: OnceLock<Vec<ActionDefinition>> = OnceLock::new();
    ACTIONS.get_or_init(|| {
        let mut actions = vec![
            action("alt_tab", "Alt + Tab (Switch Windows)", "Navigation"),
            action(
                "alt_shift_tab",
                "Alt + Shift + Tab (Switch Windows Reverse)",
                "Navigation",
            ),
            action(
                "show_desktop",
                "Show Desktop (Win + D / Mission Control)",
                "Navigation",
            ),
            action("task_view", "Task View / App Expose", "Navigation"),
            action("mission_control", "Mission Control", "Navigation"),
            action("app_expose", "App Expose", "Navigation"),
            action("launchpad", "Launchpad", "Navigation"),
            action("space_left", "Previous Space", "Navigation"),
            action("space_right", "Next Space", "Navigation"),
            action("browser_back", "Browser Back", "Browser"),
            action("browser_forward", "Browser Forward", "Browser"),
            action("close_tab", "Close Tab (Ctrl/Cmd + W)", "Browser"),
            action("new_tab", "New Tab (Ctrl/Cmd + T)", "Browser"),
            action("copy", "Copy (Ctrl/Cmd + C)", "Editing"),
            action("paste", "Paste (Ctrl/Cmd + V)", "Editing"),
            action("cut", "Cut (Ctrl/Cmd + X)", "Editing"),
            action("undo", "Undo (Ctrl/Cmd + Z)", "Editing"),
            action("redo", "Redo (Ctrl/Cmd + Shift + Z)", "Editing"),
            action("select_all", "Select All (Ctrl/Cmd + A)", "Editing"),
            action("save", "Save (Ctrl/Cmd + S)", "Editing"),
            action("find", "Find (Ctrl/Cmd + F)", "Editing"),
            action("screen_capture", "Screen Capture", "Navigation"),
            action("emoji_picker", "Emoji Picker", "Other"),
            action("volume_up", "Volume Up", "Media"),
            action("volume_down", "Volume Down", "Media"),
            action("volume_mute", "Volume Mute", "Media"),
            action("play_pause", "Play / Pause", "Media"),
            action("next_track", "Next Track", "Media"),
            action("prev_track", "Previous Track", "Media"),
            action("none", "Do Nothing (Pass-through)", "Other"),
        ];
        actions.extend(
            crate::catalog::generated_logitech_mouse_catalog()
                .extra_actions
                .clone(),
        );
        actions
    })
}

pub fn default_app_catalog() -> Vec<CatalogApp> {
    vec![
        catalog_app(
            "microsoft_edge",
            "Microsoft Edge",
            None,
            &[
                (AppMatcherKind::Executable, "msedge.exe"),
                (AppMatcherKind::Executable, "Microsoft Edge"),
                (AppMatcherKind::BundleId, "com.microsoft.edgemac"),
            ],
        ),
        catalog_app(
            "google_chrome",
            "Google Chrome",
            None,
            &[
                (AppMatcherKind::Executable, "chrome.exe"),
                (AppMatcherKind::Executable, "Google Chrome"),
                (AppMatcherKind::BundleId, "com.google.Chrome"),
            ],
        ),
        catalog_app(
            "safari",
            "Safari",
            None,
            &[
                (AppMatcherKind::Executable, "Safari"),
                (AppMatcherKind::BundleId, "com.apple.Safari"),
            ],
        ),
        catalog_app(
            "vscode",
            "Visual Studio Code",
            None,
            &[
                (AppMatcherKind::Executable, "Code.exe"),
                (AppMatcherKind::Executable, "Code"),
                (AppMatcherKind::BundleId, "com.microsoft.VSCode"),
            ],
        ),
        catalog_app(
            "vlc",
            "VLC Media Player",
            None,
            &[
                (AppMatcherKind::Executable, "vlc.exe"),
                (AppMatcherKind::Executable, "VLC"),
                (AppMatcherKind::BundleId, "org.videolan.vlc"),
            ],
        ),
        catalog_app(
            "windows_media_player",
            "Windows Media Player",
            None,
            &[
                (AppMatcherKind::Executable, "Microsoft.Media.Player.exe"),
                (AppMatcherKind::Executable, "wmplayer.exe"),
            ],
        ),
        catalog_app(
            "finder",
            "Finder",
            None,
            &[
                (AppMatcherKind::Executable, "Finder"),
                (AppMatcherKind::BundleId, "com.apple.finder"),
            ],
        ),
    ]
}

pub fn default_known_apps() -> Vec<KnownApp> {
    default_known_apps_ref().to_vec()
}

pub fn default_known_apps_ref() -> &'static [KnownApp] {
    static KNOWN_APPS: OnceLock<Vec<KnownApp>> = OnceLock::new();
    KNOWN_APPS.get_or_init(build_default_known_apps)
}

pub(crate) fn build_default_known_apps() -> Vec<KnownApp> {
    let mut apps = Vec::new();
    for catalog in default_app_catalog() {
        for matcher in catalog.matchers {
            if matcher.kind != AppMatcherKind::Executable {
                continue;
            }

            if apps
                .iter()
                .any(|app: &KnownApp| app.executable.eq_ignore_ascii_case(&matcher.value))
            {
                continue;
            }

            apps.push(known_app(
                &matcher.value,
                &catalog.label,
                catalog.icon_asset.as_deref(),
            ));
        }
    }
    apps
}

pub(crate) fn default_action_supported() -> bool {
    true
}

fn action(id: &str, label: &str, category: &str) -> ActionDefinition {
    ActionDefinition {
        id: id.to_string(),
        label: label.to_string(),
        category: category.to_string(),
        supported: true,
    }
}

fn catalog_app(
    id: &str,
    label: &str,
    icon_asset: Option<&str>,
    matchers: &[(AppMatcherKind, &str)],
) -> CatalogApp {
    let mut app_matchers = Vec::new();
    for (kind, value) in matchers {
        crate::types::push_unique_matcher(&mut app_matchers, *kind, value);
    }

    CatalogApp {
        id: id.to_string(),
        label: label.to_string(),
        icon_asset: icon_asset.map(str::to_string),
        matchers: app_matchers,
    }
}

fn known_app(executable: &str, label: &str, icon_asset: Option<&str>) -> KnownApp {
    KnownApp {
        executable: executable.to_string(),
        label: label.to_string(),
        icon_asset: icon_asset.map(str::to_string),
    }
}

pub fn manual_layout_choices(layouts: &[crate::DeviceLayout]) -> Vec<LayoutChoice> {
    let mut choices = vec![LayoutChoice {
        key: String::new(),
        label: "Auto-detect".to_string(),
    }];
    choices.extend(
        layouts
            .iter()
            .filter(|layout| layout.manual_selectable)
            .map(|layout| LayoutChoice {
                key: layout.key.clone(),
                label: layout.label.clone(),
            }),
    );
    choices
}
