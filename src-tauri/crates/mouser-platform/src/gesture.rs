use std::collections::BTreeSet;

use mouser_core::LogicalControl;

pub(crate) fn ordered_gesture_candidates(
    gesture_cids: &[u16],
    default_gesture_cids: &[u16],
) -> Vec<u16> {
    let mut ordered = Vec::new();
    let mut seen = BTreeSet::new();
    for cid in gesture_cids
        .iter()
        .copied()
        .chain(default_gesture_cids.iter().copied())
    {
        if seen.insert(cid) {
            ordered.push(cid);
        }
    }
    ordered
}

pub(crate) fn detect_gesture_control(
    delta_x: f64,
    delta_y: f64,
    threshold: f64,
    deadzone: f64,
) -> Option<LogicalControl> {
    let abs_x = delta_x.abs();
    let abs_y = delta_y.abs();
    let dominant = abs_x.max(abs_y);
    if dominant < threshold.max(5.0) {
        return None;
    }

    let cross_limit = deadzone.max(dominant * 0.35);
    if abs_x > abs_y {
        if abs_y > cross_limit {
            return None;
        }
        if delta_x < 0.0 {
            Some(LogicalControl::GestureLeft)
        } else {
            Some(LogicalControl::GestureRight)
        }
    } else {
        if abs_x > cross_limit {
            return None;
        }
        if delta_y < 0.0 {
            Some(LogicalControl::GestureUp)
        } else {
            Some(LogicalControl::GestureDown)
        }
    }
}
