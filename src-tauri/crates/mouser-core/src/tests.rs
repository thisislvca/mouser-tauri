use crate::*;

fn test_app_identity(executable: &str) -> AppIdentity {
    AppIdentity {
        label: Some(executable.to_string()),
        executable: Some(executable.to_string()),
        ..AppIdentity::default()
    }
}

#[test]
fn config_round_trip_preserves_defaults() {
    let config = default_config();
    let json = serde_json::to_string_pretty(&config).unwrap();
    let decoded: AppConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, config);
}

#[test]
fn profile_resolution_prefers_matching_app() {
    let mut config = default_config();
    config.upsert_profile(Profile {
        id: "vscode".to_string(),
        label: "VS Code".to_string(),
        app_matchers: vec![AppMatcher {
            kind: AppMatcherKind::Executable,
            value: "Code.exe".to_string(),
        }],
        bindings: default_profile_bindings(),
    });

    assert!(config.sync_active_profile_for_app(Some(&test_app_identity("code.exe"))));
    assert_eq!(config.active_profile_id, "vscode");
    assert!(config.sync_active_profile_for_app(Some(&test_app_identity("Finder"))));
    assert_eq!(config.active_profile_id, "default");
}

#[test]
fn binding_lookup_uses_control_identity() {
    let profile = default_profile();
    let binding = profile.binding_for(LogicalControl::Back).unwrap();
    assert_eq!(binding.action_id, "browser_back");
}

#[test]
fn ensure_invariants_migrates_legacy_default_profile_bindings_once() {
    let mut config = default_config();
    config.version = 3;
    config.profiles[0].bindings = legacy_default_profile_bindings_v3();

    config.ensure_invariants();

    assert_eq!(config.version, 4);
    assert_eq!(
        config.profiles[0]
            .binding_for(LogicalControl::Back)
            .map(|binding| binding.action_id.as_str()),
        Some("browser_back")
    );
    assert_eq!(
        config.profiles[0]
            .binding_for(LogicalControl::Forward)
            .map(|binding| binding.action_id.as_str()),
        Some("browser_forward")
    );
}

#[test]
fn device_metadata_and_dpi_clamp_match_catalog() {
    let catalog = default_device_catalog();
    let device = catalog
        .iter()
        .find(|device| device.key == "mx_master_3s")
        .unwrap();
    assert_eq!(device.ui_layout, "mx_master_3s");
    assert_eq!(clamp_dpi(Some(device), 9000), 8000);
    assert_eq!(clamp_dpi(Some(device), 100), 200);
}

#[test]
fn connected_device_info_prefers_live_product_name_for_display() {
    let device = build_connected_device_info(
        Some(0xB023),
        Some("MX Master 3 Mac"),
        Some("Bluetooth Low Energy"),
        Some("iokit"),
        Some(DeviceBatteryInfo {
            kind: DeviceBatteryKind::Percentage,
            percentage: Some(100),
            label: "100%".to_string(),
            source_feature: None,
            raw_capabilities: Vec::new(),
            raw_status: Vec::new(),
        }),
        1000,
        DeviceFingerprint::default(),
    );

    assert_eq!(device.model_key, "mx_master_3");
    assert_eq!(device.display_name, "MX Master 3 Mac");
    assert_eq!(device.product_name.as_deref(), Some("MX Master 3 Mac"));
}

#[test]
fn connected_device_info_uses_experimental_matrix_for_unknown_models() {
    let device = build_connected_device_info(
        Some(0xBEEF),
        Some("Mystery Logitech Device"),
        Some("USB"),
        Some("hidapi"),
        None,
        1000,
        DeviceFingerprint::default(),
    );

    assert_eq!(device.support.level, DeviceSupportLevel::Experimental);
    assert!(device.supported_controls.is_empty());
    assert!(!device.support.supports_dpi_configuration);
}

#[test]
fn managed_device_info_prefers_live_product_name_without_nickname() {
    let managed = ManagedDevice {
        id: "mx_master_3-1".to_string(),
        model_key: "mx_master_3".to_string(),
        display_name: "MX Master 3".to_string(),
        nickname: None,
        profile_id: None,
        identity_key: None,
        settings: default_device_settings(),
        created_at_ms: 1,
        last_seen_at_ms: None,
        last_seen_transport: Some("Bluetooth Low Energy".to_string()),
    };
    let live = build_connected_device_info(
        Some(0xB023),
        Some("MX Master 3 Mac"),
        Some("Bluetooth Low Energy"),
        Some("iokit"),
        Some(DeviceBatteryInfo {
            kind: DeviceBatteryKind::Percentage,
            percentage: Some(100),
            label: "100%".to_string(),
            source_feature: None,
            raw_capabilities: Vec::new(),
            raw_status: Vec::new(),
        }),
        1000,
        DeviceFingerprint::default(),
    );

    let merged = build_managed_device_info(&managed, Some(&live));

    assert_eq!(merged.display_name, "MX Master 3 Mac");
    assert_eq!(merged.product_name.as_deref(), Some("MX Master 3 Mac"));
}

#[test]
fn profile_filter_for_supported_controls_disables_unsupported_bindings() {
    let profile = default_profile();
    let filtered = profile_for_supported_controls(
        &profile,
        &[
            LogicalControl::Middle,
            LogicalControl::Back,
            LogicalControl::Forward,
        ],
    );

    assert_eq!(
        filtered
            .binding_for(LogicalControl::Back)
            .map(|binding| binding.action_id.as_str()),
        Some("browser_back")
    );
    assert_eq!(
        filtered
            .binding_for(LogicalControl::GestureLeft)
            .map(|binding| binding.action_id.as_str()),
        Some("none")
    );
    assert_eq!(
        filtered
            .binding_for(LogicalControl::HscrollLeft)
            .map(|binding| binding.action_id.as_str()),
        Some("none")
    );
}

#[test]
fn profile_resolution_prefers_device_assignment() {
    let mut config = default_config();
    config.upsert_profile(Profile {
        id: "vscode".to_string(),
        label: "VS Code".to_string(),
        app_matchers: vec![AppMatcher {
            kind: AppMatcherKind::Executable,
            value: "Code.exe".to_string(),
        }],
        bindings: default_profile_bindings(),
    });
    config.managed_devices.push(ManagedDevice {
        id: "mx_master_3-1".to_string(),
        model_key: "mx_master_3".to_string(),
        display_name: "MX Master 3 Mac".to_string(),
        nickname: None,
        profile_id: Some("default".to_string()),
        identity_key: None,
        settings: default_device_settings(),
        created_at_ms: 1,
        last_seen_at_ms: None,
        last_seen_transport: None,
    });

    assert!(!config.sync_active_profile(Some("default"), Some(&test_app_identity("Code.exe")),));
    assert_eq!(config.active_profile_id, "default");
}

#[test]
fn deleting_profile_clears_managed_device_assignment() {
    let mut config = default_config();
    config.upsert_profile(Profile {
        id: "vscode".to_string(),
        label: "VS Code".to_string(),
        app_matchers: vec![AppMatcher {
            kind: AppMatcherKind::Executable,
            value: "Code.exe".to_string(),
        }],
        bindings: default_profile_bindings(),
    });
    config.managed_devices.push(ManagedDevice {
        id: "mx_master_3-1".to_string(),
        model_key: "mx_master_3".to_string(),
        display_name: "MX Master 3 Mac".to_string(),
        nickname: None,
        profile_id: Some("vscode".to_string()),
        identity_key: None,
        settings: default_device_settings(),
        created_at_ms: 1,
        last_seen_at_ms: None,
        last_seen_transport: None,
    });

    assert!(config.delete_profile("vscode"));
    assert_eq!(config.managed_devices[0].profile_id, None);
}
