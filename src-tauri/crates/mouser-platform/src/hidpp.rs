use std::time::{Duration, Instant};

use crate::PlatformError;
use mouser_core::{DeviceBatteryInfo, DeviceBatteryKind};

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
use hidapi::HidDevice;

pub(crate) const LONG_ID: u8 = 0x11;
pub(crate) const LONG_LEN: usize = 20;
pub(crate) const BT_DEV_IDX: u8 = 0xFF;
pub(crate) const FEAT_ADJ_DPI: u16 = 0x2201;
pub(crate) const FEAT_UNIFIED_BATT: u16 = 0x1004;
pub(crate) const FEAT_BATTERY_STATUS: u16 = 0x1000;
pub(crate) const MY_SW: u8 = 0x0A;
pub(crate) const HIDPP_GET_SENSOR_DPI_FN: u8 = 0x02;
pub(crate) const HIDPP_SET_SENSOR_DPI_FN: u8 = 0x03;
const BATTERY_CAPABILITY_MILEAGE_ENABLED: u8 = 0x02;
const UNIFIED_BATTERY_FLAG_STATE_OF_CHARGE: u8 = 0x02;
const UNIFIED_BATTERY_LEVEL_CRITICAL: u8 = 0x01;
const UNIFIED_BATTERY_LEVEL_LOW: u8 = 0x02;
const UNIFIED_BATTERY_LEVEL_GOOD: u8 = 0x04;
const UNIFIED_BATTERY_LEVEL_FULL: u8 = 0x08;

pub(crate) type HidppMessage = (u8, u8, u8, u8, Vec<u8>);

pub(crate) trait HidppIo {
    fn write_packet(&self, packet: &[u8]) -> Result<(), PlatformError>;
    fn read_packet(&self, timeout_ms: i32) -> Result<Vec<u8>, PlatformError>;
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
impl HidppIo for HidDevice {
    fn write_packet(&self, packet: &[u8]) -> Result<(), PlatformError> {
        self.write(packet)
            .map_err(|error| PlatformError::Message(error.to_string()))?;
        Ok(())
    }

    fn read_packet(&self, timeout_ms: i32) -> Result<Vec<u8>, PlatformError> {
        let mut buffer = [0u8; 64];
        let size = self
            .read_timeout(&mut buffer, timeout_ms)
            .map_err(|error| PlatformError::Message(error.to_string()))?;
        Ok(buffer[..size].to_vec())
    }
}

pub(crate) fn set_sensor_dpi<T: HidppIo + ?Sized>(
    device: &T,
    dev_idx: u8,
    dpi: u16,
    timeout_ms: i32,
) -> Result<bool, PlatformError> {
    let Some(feature_index) = find_feature(device, dev_idx, FEAT_ADJ_DPI, timeout_ms)? else {
        return Ok(false);
    };

    let hi = ((dpi >> 8) & 0xFF) as u8;
    let lo = (dpi & 0xFF) as u8;
    Ok(request(
        device,
        dev_idx,
        feature_index,
        HIDPP_SET_SENSOR_DPI_FN,
        &[0, hi, lo],
        timeout_ms,
    )?
    .is_some())
}

pub(crate) fn read_sensor_dpi<T: HidppIo + ?Sized>(
    device: &T,
    dev_idx: u8,
    timeout_ms: i32,
) -> Result<Option<u16>, PlatformError> {
    let Some(feature_index) = find_feature(device, dev_idx, FEAT_ADJ_DPI, timeout_ms)? else {
        return Ok(None);
    };

    let Some((_dev_idx, _feature, _function, _sw, response)) = request(
        device,
        dev_idx,
        feature_index,
        HIDPP_GET_SENSOR_DPI_FN,
        &[0],
        timeout_ms,
    )?
    else {
        return Ok(None);
    };
    Ok(parse_sensor_dpi_response(&response))
}

pub(crate) fn parse_sensor_dpi_response(response: &[u8]) -> Option<u16> {
    if response.len() < 3 {
        return None;
    }

    Some(u16::from(response[1]) << 8 | u16::from(response[2]))
}

pub(crate) fn read_battery_info<T: HidppIo + ?Sized>(
    device: &T,
    dev_idx: u8,
    timeout_ms: i32,
) -> Result<Option<DeviceBatteryInfo>, PlatformError> {
    if let Some(feature_index) = find_feature(device, dev_idx, FEAT_UNIFIED_BATT, timeout_ms)? {
        let capabilities = request(device, dev_idx, feature_index, 0, &[], timeout_ms)?
            .map(|(_dev_idx, _feature, _function, _sw, response)| response);
        let status = request(device, dev_idx, feature_index, 1, &[], timeout_ms)?
            .map(|(_dev_idx, _feature, _function, _sw, response)| response);
        if let (Some(capabilities), Some(status)) = (capabilities, status) {
            if let Some(battery) = parse_unified_battery_info(&capabilities, &status) {
                return Ok(Some(battery));
            }
        }
    }

    if let Some(feature_index) = find_feature(device, dev_idx, FEAT_BATTERY_STATUS, timeout_ms)? {
        let capabilities = request(device, dev_idx, feature_index, 1, &[], timeout_ms)?
            .map(|(_dev_idx, _feature, _function, _sw, response)| response);
        let status = request(device, dev_idx, feature_index, 0, &[], timeout_ms)?
            .map(|(_dev_idx, _feature, _function, _sw, response)| response);
        if let (Some(capabilities), Some(status)) = (capabilities, status) {
            if let Some(battery) = parse_legacy_battery_info(&capabilities, &status) {
                return Ok(Some(battery));
            }
        }
    }

    Ok(None)
}

fn parse_legacy_battery_info(capabilities: &[u8], status: &[u8]) -> Option<DeviceBatteryInfo> {
    let raw_level = status.first().copied()?;
    let charge_status = status.get(2).copied().unwrap_or(u8::MAX);
    let level_count = capabilities.first().copied().unwrap_or_default();
    let flags = capabilities.get(1).copied().unwrap_or_default();
    let has_percentage = level_count >= 10 && (flags & BATTERY_CAPABILITY_MILEAGE_ENABLED) != 0;

    if has_percentage && (1..=100).contains(&raw_level) {
        return Some(DeviceBatteryInfo {
            kind: DeviceBatteryKind::Percentage,
            percentage: Some(raw_level),
            label: percentage_label(raw_level, legacy_charge_label(charge_status)),
            source_feature: Some("battery_status".to_string()),
            raw_capabilities: capabilities.to_vec(),
            raw_status: status.to_vec(),
        });
    }

    Some(DeviceBatteryInfo {
        kind: DeviceBatteryKind::Status,
        percentage: None,
        label: status_label(
            legacy_level_label(raw_level),
            legacy_charge_label(charge_status),
        ),
        source_feature: Some("battery_status".to_string()),
        raw_capabilities: capabilities.to_vec(),
        raw_status: status.to_vec(),
    })
}

fn parse_unified_battery_info(capabilities: &[u8], status: &[u8]) -> Option<DeviceBatteryInfo> {
    let state_of_charge = status.first().copied()?;
    let level_flags = status.get(1).copied().unwrap_or_default();
    let charging_status = status.get(2).copied().unwrap_or(u8::MAX);
    let flags = capabilities.get(1).copied().unwrap_or_default();
    let has_percentage = (flags & UNIFIED_BATTERY_FLAG_STATE_OF_CHARGE) != 0;

    if has_percentage && (1..=100).contains(&state_of_charge) {
        return Some(DeviceBatteryInfo {
            kind: DeviceBatteryKind::Percentage,
            percentage: Some(state_of_charge),
            label: percentage_label(state_of_charge, unified_charge_label(charging_status)),
            source_feature: Some("unified_battery".to_string()),
            raw_capabilities: capabilities.to_vec(),
            raw_status: status.to_vec(),
        });
    }

    Some(DeviceBatteryInfo {
        kind: DeviceBatteryKind::Status,
        percentage: None,
        label: status_label(
            unified_level_label(level_flags),
            unified_charge_label(charging_status),
        ),
        source_feature: Some("unified_battery".to_string()),
        raw_capabilities: capabilities.to_vec(),
        raw_status: status.to_vec(),
    })
}

fn percentage_label(percentage: u8, charge_label: Option<&'static str>) -> String {
    match charge_label {
        Some("Charging") => format!("Charging ({percentage}%)"),
        Some("Charging slowly") => format!("Charging slowly ({percentage}%)"),
        Some("Charged") => "Charged".to_string(),
        Some("Battery error") => "Battery error".to_string(),
        Some("Thermal error") => "Thermal error".to_string(),
        Some("Invalid battery") => "Invalid battery".to_string(),
        Some(label) => label.to_string(),
        None => format!("{percentage}%"),
    }
}

fn status_label(level_label: Option<&'static str>, charge_label: Option<&'static str>) -> String {
    match charge_label {
        Some("Charging") => "Charging".to_string(),
        Some("Charging slowly") => "Charging slowly".to_string(),
        Some("Charged") => "Charged".to_string(),
        Some("Battery error") => "Battery error".to_string(),
        Some("Thermal error") => "Thermal error".to_string(),
        Some("Invalid battery") => "Invalid battery".to_string(),
        Some(label) => label.to_string(),
        None => level_label.unwrap_or("Status unavailable").to_string(),
    }
}

fn legacy_level_label(raw_level: u8) -> Option<&'static str> {
    match raw_level {
        0 => None,
        1..=10 => Some("Critical"),
        11..=30 => Some("Low"),
        31..=80 => Some("Good"),
        81..=100 => Some("Full"),
        _ => None,
    }
}

fn legacy_charge_label(charge_status: u8) -> Option<&'static str> {
    match charge_status {
        0 => None,
        1 | 2 => Some("Charging"),
        3 => Some("Charged"),
        4 => Some("Charging slowly"),
        5 | 7 => Some("Battery error"),
        6 => Some("Thermal error"),
        _ => None,
    }
}

fn unified_level_label(level_flags: u8) -> Option<&'static str> {
    if (level_flags & UNIFIED_BATTERY_LEVEL_FULL) != 0 {
        Some("Full")
    } else if (level_flags & UNIFIED_BATTERY_LEVEL_GOOD) != 0 {
        Some("Good")
    } else if (level_flags & UNIFIED_BATTERY_LEVEL_LOW) != 0 {
        Some("Low")
    } else if (level_flags & UNIFIED_BATTERY_LEVEL_CRITICAL) != 0 {
        Some("Critical")
    } else {
        None
    }
}

fn unified_charge_label(charging_status: u8) -> Option<&'static str> {
    match charging_status {
        0 => None,
        1 => Some("Charging"),
        2 => Some("Charging slowly"),
        3 => Some("Charged"),
        4 => Some("Battery error"),
        _ => None,
    }
}

pub(crate) fn find_feature<T: HidppIo + ?Sized>(
    device: &T,
    dev_idx: u8,
    feature_id: u16,
    timeout_ms: i32,
) -> Result<Option<u8>, PlatformError> {
    let feature_hi = ((feature_id >> 8) & 0xFF) as u8;
    let feature_lo = (feature_id & 0xFF) as u8;
    let Some((_dev_idx, _feature, _function, _sw, params)) = request(
        device,
        dev_idx,
        0x00,
        0,
        &[feature_hi, feature_lo, 0x00],
        timeout_ms,
    )?
    else {
        return Ok(None);
    };

    Ok(params
        .first()
        .copied()
        .filter(|feature_index| *feature_index != 0))
}

pub(crate) fn request<T: HidppIo + ?Sized>(
    device: &T,
    dev_idx: u8,
    feature_idx: u8,
    function: u8,
    params: &[u8],
    timeout_ms: i32,
) -> Result<Option<HidppMessage>, PlatformError> {
    write_request(device, dev_idx, feature_idx, function, params)?;
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.max(50) as u64);
    let expected_reply_functions = [function, (function + 1) & 0x0F];

    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let packet =
            device.read_packet(remaining.min(Duration::from_millis(80)).as_millis() as i32)?;
        if packet.is_empty() {
            continue;
        }

        let Some((
            response_dev_idx,
            response_feature,
            response_function,
            response_sw,
            response_params,
        )) = parse_message(&packet)
        else {
            continue;
        };

        if response_feature == 0xFF {
            return Ok(None);
        }

        if response_dev_idx == dev_idx
            && response_feature == feature_idx
            && response_sw == MY_SW
            && expected_reply_functions.contains(&response_function)
        {
            return Ok(Some((
                response_dev_idx,
                response_feature,
                response_function,
                response_sw,
                response_params,
            )));
        }
    }

    Ok(None)
}

pub(crate) fn write_request<T: HidppIo + ?Sized>(
    device: &T,
    dev_idx: u8,
    feature_idx: u8,
    function: u8,
    params: &[u8],
) -> Result<(), PlatformError> {
    let mut packet = [0u8; LONG_LEN];
    packet[0] = LONG_ID;
    packet[1] = dev_idx;
    packet[2] = feature_idx;
    packet[3] = ((function & 0x0F) << 4) | (MY_SW & 0x0F);
    for (offset, byte) in params.iter().copied().enumerate() {
        if 4 + offset < LONG_LEN {
            packet[4 + offset] = byte;
        }
    }

    device.write_packet(&packet)
}

pub(crate) fn parse_message(raw: &[u8]) -> Option<HidppMessage> {
    if raw.len() < 4 {
        return None;
    }

    let offset = usize::from(matches!(raw.first(), Some(0x10) | Some(0x11)));
    if raw.len() < offset + 4 {
        return None;
    }

    let dev_idx = raw[offset];
    let feature = raw[offset + 1];
    let function_and_sw = raw[offset + 2];
    let function = (function_and_sw >> 4) & 0x0F;
    let sw = function_and_sw & 0x0F;
    let params = raw[offset + 3..].to_vec();

    Some((dev_idx, feature, function, sw, params))
}

#[cfg(test)]
mod tests {
    use super::{
        parse_legacy_battery_info, parse_sensor_dpi_response, parse_unified_battery_info,
    };
    use mouser_core::DeviceBatteryKind;

    #[test]
    fn sensor_dpi_response_parses_big_endian_payload() {
        assert_eq!(parse_sensor_dpi_response(&[0, 0x03, 0xE8]), Some(1000));
    }

    #[test]
    fn legacy_battery_uses_percentage_when_mileage_is_enabled() {
        let battery = parse_legacy_battery_info(&[100, 0x02], &[84, 0, 0]).unwrap();
        assert_eq!(battery.kind, DeviceBatteryKind::Percentage);
        assert_eq!(battery.percentage, Some(84));
        assert_eq!(battery.label, "84%");
        assert_eq!(battery.source_feature.as_deref(), Some("battery_status"));
        assert_eq!(battery.raw_capabilities, vec![100, 0x02]);
        assert_eq!(battery.raw_status, vec![84, 0, 0]);
    }

    #[test]
    fn legacy_battery_maps_coarse_levels_when_mileage_is_disabled() {
        let battery = parse_legacy_battery_info(&[4, 0x00], &[50, 0, 0]).unwrap();
        assert_eq!(battery.kind, DeviceBatteryKind::Status);
        assert_eq!(battery.percentage, None);
        assert_eq!(battery.label, "Good");
        assert_eq!(battery.source_feature.as_deref(), Some("battery_status"));
    }

    #[test]
    fn unified_battery_uses_percentage_when_state_of_charge_is_available() {
        let battery = parse_unified_battery_info(&[0, 0x02], &[73, 0, 1, 0]).unwrap();
        assert_eq!(battery.kind, DeviceBatteryKind::Percentage);
        assert_eq!(battery.percentage, Some(73));
        assert_eq!(battery.label, "Charging (73%)");
        assert_eq!(battery.source_feature.as_deref(), Some("unified_battery"));
        assert_eq!(battery.raw_capabilities, vec![0, 0x02]);
        assert_eq!(battery.raw_status, vec![73, 0, 1, 0]);
    }

    #[test]
    fn unified_battery_maps_level_flags_when_only_coarse_status_exists() {
        let battery = parse_unified_battery_info(&[0x0F, 0x00], &[0, 0x04, 0, 0]).unwrap();
        assert_eq!(battery.kind, DeviceBatteryKind::Status);
        assert_eq!(battery.percentage, None);
        assert_eq!(battery.label, "Good");
        assert_eq!(battery.source_feature.as_deref(), Some("unified_battery"));
    }
}
