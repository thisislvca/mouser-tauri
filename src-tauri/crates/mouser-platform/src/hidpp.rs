use std::time::{Duration, Instant};

use crate::PlatformError;

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

pub(crate) fn read_battery_level<T: HidppIo + ?Sized>(
    device: &T,
    dev_idx: u8,
    timeout_ms: i32,
) -> Result<Option<u8>, PlatformError> {
    if let Some(feature_index) = find_feature(device, dev_idx, FEAT_UNIFIED_BATT, timeout_ms)? {
        if let Some((_dev_idx, _feature, _function, _sw, response)) =
            request(device, dev_idx, feature_index, 1, &[], timeout_ms)?
        {
            return Ok(response.first().copied());
        }
    }

    if let Some(feature_index) = find_feature(device, dev_idx, FEAT_BATTERY_STATUS, timeout_ms)? {
        if let Some((_dev_idx, _feature, _function, _sw, response)) =
            request(device, dev_idx, feature_index, 0, &[], timeout_ms)?
        {
            return Ok(response.first().copied());
        }
    }

    Ok(None)
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
