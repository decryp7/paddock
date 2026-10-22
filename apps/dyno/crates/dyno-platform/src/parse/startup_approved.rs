//! `…\Explorer\StartupApproved\{Run,Run32,StartupFolder}` values: 12 bytes, the first says enabled (even: 0x02,
//! 0x06) or disabled (odd: 0x03, 0x01, 0x07); a disabled value carries the FILETIME it was disabled at in bytes 4–11.

use dyno_core::startup::StartupState;

/// The state a StartupApproved value records (`None` for an empty value: treat as enabled).
pub fn approved_state(bytes: &[u8]) -> Option<StartupState> {
    let first = *bytes.first()?;
    Some(if first & 1 == 1 {
        StartupState::Disabled
    } else {
        StartupState::Enabled
    })
}

/// The value Task Manager writes: `0x02` and zeros to enable, `0x03` and the FILETIME of the change to disable.
pub fn approved_value(enabled: bool, now_filetime: u64) -> [u8; 12] {
    let mut v = [0u8; 12];
    if enabled {
        v[0] = 0x02;
    } else {
        v[0] = 0x03;
        v[4..12].copy_from_slice(&now_filetime.to_le_bytes());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_task_manager_values() {
        assert_eq!(approved_state(&[0x02, 0, 0, 0]), Some(StartupState::Enabled));
        assert_eq!(approved_state(&[0x06]), Some(StartupState::Enabled));
        assert_eq!(approved_state(&[0x03, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8]), Some(StartupState::Disabled));
        assert_eq!(approved_state(&[0x07]), Some(StartupState::Disabled));
        assert_eq!(approved_state(&[]), None);
    }

    #[test]
    fn writes_task_manager_values() {
        assert_eq!(approved_value(true, 99), [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let v = approved_value(false, 0x0102_0304_0506_0708);
        assert_eq!(v[0], 3);
        assert_eq!(&v[1..4], &[0, 0, 0]);
        assert_eq!(&v[4..], &[8, 7, 6, 5, 4, 3, 2, 1]);
        assert_eq!(approved_state(&v), Some(StartupState::Disabled));
    }
}
