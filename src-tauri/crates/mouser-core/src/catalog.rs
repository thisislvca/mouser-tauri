use std::sync::OnceLock;

use serde::Deserialize;

use crate::{
    ActionDefinition, DeviceBatteryInfo, DeviceFingerprint, DeviceInfo, DeviceLayout,
    DeviceSupportLevel, DeviceSupportMatrix, KnownDeviceSpec, LogicalControl, ManagedDevice,
};

pub fn known_device_specs() -> Vec<KnownDeviceSpec> {
    known_device_specs_ref().to_vec()
}

pub fn known_device_specs_ref() -> &'static [KnownDeviceSpec] {
    static KNOWN_DEVICE_SPECS: OnceLock<Vec<KnownDeviceSpec>> = OnceLock::new();
    KNOWN_DEVICE_SPECS.get_or_init(|| generated_logitech_mouse_catalog().devices.clone())
}

pub fn known_device_spec_by_key(model_key: &str) -> Option<KnownDeviceSpec> {
    known_device_specs_ref()
        .iter()
        .find(|spec| spec.key == model_key)
        .cloned()
}

pub fn default_layouts() -> Vec<DeviceLayout> {
    default_layouts_ref().to_vec()
}

pub fn default_layouts_ref() -> &'static [DeviceLayout] {
    static LAYOUTS: OnceLock<Vec<DeviceLayout>> = OnceLock::new();
    LAYOUTS.get_or_init(|| {
        let mut layouts = generated_logitech_mouse_catalog().layouts.clone();
        layouts.push(DeviceLayout {
            key: "generic_mouse".to_string(),
            label: "Generic mouse".to_string(),
            image_asset: "/assets/icons/mouse-simple.svg".to_string(),
            image_width: 220,
            image_height: 220,
            interactive: false,
            manual_selectable: false,
            note: "This device is detected and the backend can still probe HID++ features, but Mouser does not have a dedicated visual overlay for it yet."
                .to_string(),
            hotspots: Vec::new(),
        });
        layouts
    })
}

pub fn default_device_catalog() -> Vec<DeviceInfo> {
    let mx_master_3s = known_device_spec_by_key("mx_master_3s");
    let mx_anywhere_3 = known_device_spec_by_key("mx_anywhere_3");

    vec![
        DeviceInfo {
            key: "mx_master_3s".to_string(),
            model_key: "mx_master_3s".to_string(),
            display_name: "MX Master 3S".to_string(),
            nickname: None,
            product_id: Some(0xB034),
            product_name: Some("MX Master 3S".to_string()),
            transport: Some("Bluetooth Low Energy".to_string()),
            source: Some("mock-catalog".to_string()),
            ui_layout: mx_master_3s
                .as_ref()
                .map(|spec| spec.ui_layout.clone())
                .unwrap_or_else(|| "generic_mouse".to_string()),
            image_asset: mx_master_3s
                .as_ref()
                .map(|spec| spec.image_asset.clone())
                .unwrap_or_else(|| "/assets/icons/mouse-simple.svg".to_string()),
            supported_controls: mx_master_3s
                .as_ref()
                .map(|spec| spec.supported_controls.clone())
                .unwrap_or_else(LogicalControl::all),
            controls: mx_master_3s
                .as_ref()
                .map(|spec| spec.controls.clone())
                .unwrap_or_default(),
            support: mx_master_3s
                .as_ref()
                .map(|spec| spec.support.clone())
                .unwrap_or_else(generic_logitech_support),
            gesture_cids: mx_master_3s
                .as_ref()
                .map(|spec| spec.gesture_cids.clone())
                .unwrap_or_default(),
            dpi_min: mx_master_3s
                .as_ref()
                .map(|spec| spec.dpi_min)
                .unwrap_or(200),
            dpi_max: mx_master_3s
                .as_ref()
                .map(|spec| spec.dpi_max)
                .unwrap_or(8000),
            dpi_inferred: mx_master_3s
                .as_ref()
                .map(|spec| spec.dpi_inferred)
                .unwrap_or(false),
            dpi_source_kind: mx_master_3s
                .as_ref()
                .and_then(|spec| spec.dpi_source_kind.clone()),
            connected: true,
            battery: Some(DeviceBatteryInfo {
                kind: crate::DeviceBatteryKind::Percentage,
                percentage: Some(84),
                label: "84%".to_string(),
                source_feature: None,
                raw_capabilities: Vec::new(),
                raw_status: Vec::new(),
            }),
            battery_level: Some(84),
            current_dpi: 1200,
            fingerprint: DeviceFingerprint::default(),
        },
        DeviceInfo {
            key: "mx_anywhere_3".to_string(),
            model_key: "mx_anywhere_3".to_string(),
            display_name: "MX Anywhere 3".to_string(),
            nickname: None,
            product_id: Some(0xB025),
            product_name: Some("MX Anywhere 3".to_string()),
            transport: Some("Bolt Receiver".to_string()),
            source: Some("mock-catalog".to_string()),
            ui_layout: mx_anywhere_3
                .as_ref()
                .map(|spec| spec.ui_layout.clone())
                .unwrap_or_else(|| "generic_mouse".to_string()),
            image_asset: mx_anywhere_3
                .as_ref()
                .map(|spec| spec.image_asset.clone())
                .unwrap_or_else(|| "/assets/icons/mouse-simple.svg".to_string()),
            supported_controls: mx_anywhere_3
                .as_ref()
                .map(|spec| spec.supported_controls.clone())
                .unwrap_or_else(LogicalControl::all),
            controls: mx_anywhere_3
                .as_ref()
                .map(|spec| spec.controls.clone())
                .unwrap_or_default(),
            support: mx_anywhere_3
                .as_ref()
                .map(|spec| spec.support.clone())
                .unwrap_or_else(generic_logitech_support),
            gesture_cids: mx_anywhere_3
                .as_ref()
                .map(|spec| spec.gesture_cids.clone())
                .unwrap_or_default(),
            dpi_min: mx_anywhere_3
                .as_ref()
                .map(|spec| spec.dpi_min)
                .unwrap_or(200),
            dpi_max: mx_anywhere_3
                .as_ref()
                .map(|spec| spec.dpi_max)
                .unwrap_or(8000),
            dpi_inferred: mx_anywhere_3
                .as_ref()
                .map(|spec| spec.dpi_inferred)
                .unwrap_or(false),
            dpi_source_kind: mx_anywhere_3
                .as_ref()
                .and_then(|spec| spec.dpi_source_kind.clone()),
            connected: false,
            battery: Some(DeviceBatteryInfo {
                kind: crate::DeviceBatteryKind::Percentage,
                percentage: Some(62),
                label: "62%".to_string(),
                source_feature: None,
                raw_capabilities: Vec::new(),
                raw_status: Vec::new(),
            }),
            battery_level: Some(62),
            current_dpi: 1000,
            fingerprint: DeviceFingerprint::default(),
        },
        DeviceInfo {
            key: "mystery_logitech_mouse".to_string(),
            model_key: "mystery_logitech_mouse".to_string(),
            display_name: "Mystery Logitech Mouse".to_string(),
            nickname: None,
            product_id: Some(0xB999),
            product_name: Some("Mystery Logitech Mouse".to_string()),
            transport: Some("USB".to_string()),
            source: Some("mock-catalog".to_string()),
            ui_layout: "generic_mouse".to_string(),
            image_asset: "/assets/icons/mouse-simple.svg".to_string(),
            supported_controls: Vec::new(),
            controls: Vec::new(),
            support: generic_logitech_support(),
            gesture_cids: vec![0x00C3, 0x00D7],
            dpi_min: 200,
            dpi_max: 8000,
            dpi_inferred: false,
            dpi_source_kind: None,
            connected: false,
            battery: None,
            battery_level: None,
            current_dpi: 900,
            fingerprint: DeviceFingerprint::default(),
        },
    ]
}

pub fn resolve_known_device(
    product_id: Option<u16>,
    product_name: Option<&str>,
) -> Option<KnownDeviceSpec> {
    let normalized_name = crate::types::normalize_name(product_name.unwrap_or_default());
    known_device_specs_ref()
        .iter()
        .find(|spec| {
            product_id
                .map(|product_id| spec.product_ids.contains(&product_id))
                .unwrap_or(false)
                || (!normalized_name.is_empty()
                    && std::iter::once(spec.display_name.as_str())
                        .chain(spec.aliases.iter().map(String::as_str))
                        .any(|candidate| {
                            crate::types::normalize_name(candidate) == normalized_name
                        }))
        })
        .cloned()
}

pub fn build_connected_device_info(
    product_id: Option<u16>,
    product_name: Option<&str>,
    transport: Option<&str>,
    source: Option<&str>,
    battery: Option<DeviceBatteryInfo>,
    current_dpi: u16,
    mut fingerprint: DeviceFingerprint,
) -> DeviceInfo {
    hydrate_identity_key(product_id, &mut fingerprint);
    let product_name = crate::types::non_empty_name(product_name);
    if let Some(spec) = resolve_known_device(product_id, product_name.as_deref()) {
        let model_key = spec.key.clone();
        let display_name = product_name
            .clone()
            .unwrap_or_else(|| spec.display_name.clone());
        let key = live_device_key(&model_key, &fingerprint);
        return DeviceInfo {
            key,
            model_key,
            display_name,
            nickname: None,
            product_id,
            product_name,
            transport: transport.map(str::to_string),
            source: source.map(str::to_string),
            ui_layout: spec.ui_layout,
            image_asset: spec.image_asset,
            supported_controls: spec.supported_controls,
            controls: spec.controls,
            support: support_with_runtime_battery(spec.support, battery.as_ref()),
            gesture_cids: spec.gesture_cids,
            dpi_min: spec.dpi_min,
            dpi_max: spec.dpi_max,
            dpi_inferred: spec.dpi_inferred,
            dpi_source_kind: spec.dpi_source_kind,
            connected: true,
            battery_level: battery.as_ref().and_then(|value| value.percentage),
            battery,
            current_dpi,
            fingerprint,
        };
    }

    let display_name = product_name
        .or_else(|| product_id.map(|product_id| format!("Logitech PID 0x{product_id:04X}")))
        .unwrap_or_else(|| "Logitech mouse".to_string());
    let key = crate::types::normalize_name(&display_name).replace(' ', "_");
    let fallback_key = if key.is_empty() {
        "logitech_mouse".to_string()
    } else {
        key
    };
    let key = live_device_key(&fallback_key, &fingerprint);

    DeviceInfo {
        key,
        model_key: fallback_key,
        display_name: display_name.clone(),
        nickname: None,
        product_id,
        product_name: Some(display_name),
        transport: transport.map(str::to_string),
        source: source.map(str::to_string),
        ui_layout: "generic_mouse".to_string(),
        image_asset: "/assets/icons/mouse-simple.svg".to_string(),
        supported_controls: Vec::new(),
        controls: Vec::new(),
        support: support_with_runtime_battery(generic_logitech_support(), battery.as_ref()),
        gesture_cids: vec![0x00C3, 0x00D7],
        dpi_min: 200,
        dpi_max: 8000,
        dpi_inferred: false,
        dpi_source_kind: None,
        connected: true,
        battery_level: battery.as_ref().and_then(|value| value.percentage),
        battery,
        current_dpi,
        fingerprint,
    }
}

pub fn build_managed_device_info(managed: &ManagedDevice, live: Option<&DeviceInfo>) -> DeviceInfo {
    let effective_current_dpi = live
        .map(|device| device.current_dpi)
        .unwrap_or(managed.settings.dpi);
    let live_product_name =
        live.and_then(|device| crate::types::non_empty_name(device.product_name.as_deref()));
    let display_name =
        crate::types::managed_device_display_name(managed, live_product_name.as_deref());
    let connected = live.is_some();
    let battery = live.and_then(|device| device.battery.clone());
    let battery_level = live.and_then(|device| device.battery_level);
    let transport = crate::types::managed_device_transport(managed, live);
    let source = crate::types::managed_device_source(live);
    let fingerprint = crate::types::managed_device_fingerprint(managed, live);

    if let Some(spec) = known_device_spec_by_key(&managed.model_key) {
        return DeviceInfo {
            key: managed.id.clone(),
            model_key: managed.model_key.clone(),
            display_name,
            nickname: managed.nickname.clone(),
            product_id: live
                .and_then(|device| device.product_id)
                .or_else(|| spec.product_ids.first().copied()),
            product_name: live_product_name.or_else(|| Some(spec.display_name.clone())),
            transport,
            source,
            ui_layout: spec.ui_layout,
            image_asset: spec.image_asset,
            supported_controls: spec.supported_controls,
            controls: spec.controls,
            support: support_with_runtime_battery(spec.support, battery.as_ref()),
            gesture_cids: spec.gesture_cids,
            dpi_min: spec.dpi_min,
            dpi_max: spec.dpi_max,
            dpi_inferred: spec.dpi_inferred,
            dpi_source_kind: spec.dpi_source_kind,
            connected,
            battery,
            battery_level,
            current_dpi: effective_current_dpi.max(spec.dpi_min).min(spec.dpi_max),
            fingerprint,
        };
    }

    DeviceInfo {
        key: managed.id.clone(),
        model_key: managed.model_key.clone(),
        display_name,
        nickname: managed.nickname.clone(),
        product_id: live.and_then(|device| device.product_id),
        product_name: live_product_name.or_else(|| Some(managed.display_name.clone())),
        transport,
        source,
        ui_layout: "generic_mouse".to_string(),
        image_asset: "/assets/icons/mouse-simple.svg".to_string(),
        supported_controls: Vec::new(),
        controls: Vec::new(),
        support: support_with_runtime_battery(generic_logitech_support(), battery.as_ref()),
        gesture_cids: vec![0x00C3, 0x00D7],
        dpi_min: 200,
        dpi_max: 8000,
        dpi_inferred: false,
        dpi_source_kind: None,
        connected,
        battery,
        battery_level,
        current_dpi: effective_current_dpi.clamp(200, 8000),
        fingerprint,
    }
}

pub fn clamp_dpi(device: Option<&DeviceInfo>, value: u16) -> u16 {
    let min = device.map(|info| info.dpi_min).unwrap_or(200);
    let max = device.map(|info| info.dpi_max).unwrap_or(8000);
    value.max(min).min(max)
}

pub fn normalize_device_settings(model_key: Option<&str>, settings: &mut crate::DeviceSettings) {
    let (dpi_min, dpi_max) = known_device_spec_by_key(model_key.unwrap_or_default())
        .map(|spec| (spec.dpi_min, spec.dpi_max))
        .unwrap_or((200, 8000));
    settings.dpi = settings.dpi.max(dpi_min).min(dpi_max);

    if settings
        .manual_layout_override
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        settings.manual_layout_override = None;
    }
}

pub fn layout_by_key<'a>(
    layouts: &'a [DeviceLayout],
    layout_key: &str,
) -> Option<&'a DeviceLayout> {
    layouts.iter().find(|layout| layout.key == layout_key)
}

pub fn effective_layout_key(manual_override: Option<&str>, default_layout_key: &str) -> String {
    manual_override
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_layout_key)
        .to_string()
}

pub fn active_device_with_layout(
    mut device: DeviceInfo,
    manual_override: Option<&str>,
    layouts: &[DeviceLayout],
) -> DeviceInfo {
    let layout_key = effective_layout_key(manual_override, &device.ui_layout);
    device.ui_layout = layout_key.clone();
    if let Some(layout) = layout_by_key(layouts, &layout_key) {
        device.image_asset = layout.image_asset.clone();
    }
    device
}

pub fn hydrate_identity_key(product_id: Option<u16>, fingerprint: &mut DeviceFingerprint) {
    if fingerprint.identity_key.is_some() {
        return;
    }

    fingerprint.identity_key = fingerprint
        .serial_number
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|serial| format!("serial:{:04x}:{}", product_id.unwrap_or_default(), serial,))
        .or_else(|| {
            fingerprint.location_id.map(|location_id| {
                format!(
                    "location:{:04x}:{location_id:08x}:{:04x}:{:04x}",
                    product_id.unwrap_or_default(),
                    fingerprint.usage_page.unwrap_or_default(),
                    fingerprint.usage.unwrap_or_default(),
                )
            })
        })
        .or_else(|| {
            fingerprint
                .hid_path
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|path| format!("path:{path}"))
        })
        .or_else(|| {
            match (
                fingerprint.interface_number,
                fingerprint.usage_page,
                fingerprint.usage,
            ) {
                (Some(interface_number), Some(usage_page), Some(usage)) => Some(format!(
                    "interface:{:04x}:{interface_number}:{usage_page:04x}:{usage:04x}",
                    product_id.unwrap_or_default(),
                )),
                _ => None,
            }
        });
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GeneratedLogitechMouseCatalog {
    pub devices: Vec<KnownDeviceSpec>,
    pub layouts: Vec<DeviceLayout>,
    #[serde(default)]
    pub extra_actions: Vec<ActionDefinition>,
}

pub(crate) fn generated_logitech_mouse_catalog() -> &'static GeneratedLogitechMouseCatalog {
    static GENERATED: OnceLock<GeneratedLogitechMouseCatalog> = OnceLock::new();
    GENERATED.get_or_init(|| {
        serde_json::from_str(include_str!("../generated/logitech-mouse-catalog.json"))
            .expect("valid generated Logitech mouse catalog")
    })
}

fn generic_logitech_support() -> DeviceSupportMatrix {
    DeviceSupportMatrix {
        level: DeviceSupportLevel::Experimental,
        supports_battery_status: false,
        supports_dpi_configuration: false,
        has_interactive_layout: false,
        notes: vec![
            "The backend detected this Logitech device, but Mouser does not have a verified support entry for it yet.".to_string(),
            "Add a catalog entry before exposing remapping or tuning controls for this model.".to_string(),
        ],
    }
}

fn support_with_runtime_battery(
    mut support: DeviceSupportMatrix,
    battery: Option<&DeviceBatteryInfo>,
) -> DeviceSupportMatrix {
    if battery.is_some() {
        support.supports_battery_status = true;
    }
    support
}

fn live_device_key(model_key: &str, fingerprint: &DeviceFingerprint) -> String {
    fingerprint
        .identity_key
        .clone()
        .unwrap_or_else(|| model_key.to_string())
}
