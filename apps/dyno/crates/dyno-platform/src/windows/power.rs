//! Sleep, hibernate and shut down on Windows by the normal path, and the full-screen state. `ExitWindowsEx` is called without
//! `EWX_FORCE`: every app gets `WM_QUERYENDSESSION` and one with unsaved work can hold the shutdown until the person acts.
#![cfg_attr(
    not(test),
    deny(clippy::let_underscore_must_use, clippy::let_underscore_future, clippy::let_underscore_untyped)
)]

use dyno_core::metrics::Reading;
use dyno_core::ports::PlatformError;
use dyno_core::power::idle::IdleAction;
use windows::core::w;
use windows::core::Interface;
use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID};
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::{
    eConsole, eRender, AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Shutdown::{ExitWindowsEx, EWX_POWEROFF, EWX_SHUTDOWN, SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MAJOR_OTHER};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN};

/// Asks Windows for `action` the normal way.
pub fn request(action: IdleAction) -> Result<(), PlatformError> {
    match action {
        IdleAction::Sleep => suspend(false),
        IdleAction::Hibernate => suspend(true),
        IdleAction::ShutDown => shut_down(),
    }
}

fn suspend(hibernate: bool) -> Result<(), PlatformError> {
    // SAFETY: three booleans; `force` is false, so apps are asked and a hibernate needs hibernation to be on.
    let ok = unsafe { SetSuspendState(hibernate, false, false) };
    if ok {
        Ok(())
    } else {
        Err(PlatformError::Failed(if hibernate {
            "Windows did not hibernate (hibernation may be off: powercfg /hibernate on).".into()
        } else {
            "Windows did not go to sleep.".into()
        }))
    }
}

fn shut_down() -> Result<(), PlatformError> {
    enable_shutdown_privilege()?;
    // SAFETY: plain flags; no EWX_FORCE, so apps with unsaved work can hold the shutdown (the planned reason is logged by Windows).
    unsafe { ExitWindowsEx(EWX_SHUTDOWN | EWX_POWEROFF, SHTDN_REASON_MAJOR_OTHER | SHTDN_REASON_FLAG_PLANNED) }
        .map_err(|e| PlatformError::Failed(format!("Windows did not take the shutdown request: {e}")))
}

fn enable_shutdown_privilege() -> Result<(), PlatformError> {
    let mut token = HANDLE::default();
    // SAFETY: the current process pseudo handle and a valid out pointer.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES, &mut token) }
        .map_err(|e| PlatformError::AccessDenied(format!("Windows would not open this program's token: {e}")))?;
    let mut luid = LUID::default();
    // SAFETY: a valid privilege name and out pointer.
    let looked_up = unsafe { LookupPrivilegeValueW(None, w!("SeShutdownPrivilege"), &mut luid) };
    let result = looked_up
        .map_err(|e| PlatformError::AccessDenied(format!("Windows has no shutdown privilege for this user: {e}")))
        .and_then(|()| {
            let privileges = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES {
                    Luid: luid,
                    Attributes: SE_PRIVILEGE_ENABLED,
                }],
            };
            // SAFETY: `token` is a live token with TOKEN_ADJUST_PRIVILEGES and `privileges` lives across the call.
            unsafe { AdjustTokenPrivileges(token, false, Some(&privileges), 0, None, None) }
                .map_err(|e| PlatformError::AccessDenied(format!("Windows would not enable the shutdown privilege: {e}")))
        });
    // SAFETY: the token was opened above and is closed once.
    paddock_proc::cleanup::logged("closing the process token", unsafe { CloseHandle(token) });
    result
}

/// Whether a full-screen app, a presentation or a busy state shows (`SHQueryUserNotificationState`).
pub fn full_screen() -> Reading<bool> {
    // SAFETY: no arguments; the call returns the state or an error.
    match unsafe { SHQueryUserNotificationState() } {
        Ok(state) => Reading::Available(state == QUNS_BUSY || state == QUNS_RUNNING_D3D_FULL_SCREEN || state == QUNS_PRESENTATION_MODE),
        Err(e) => Reading::Unavailable(format!("Windows would not say whether an app is full screen: {e}")),
    }
}

/// The loudest an active session may peak and still count as silence (a stream that is open but plays nothing).
const SILENCE_PEAK: f32 = 0.000_1;

/// Whether any audio session on the default playback device is active and making sound, read in user mode (WASAPI
/// `IAudioSessionManager2` and `IAudioMeterInformation`; no administrator rights). System sounds are not counted.
pub fn audio_playing() -> Reading<bool> {
    match read_audio() {
        Ok(playing) => Reading::Available(playing),
        // No playback device at all (E_NOTFOUND, measured in the test VM): nothing can be playing.
        Err(e) if e.code() == windows::core::HRESULT(0x8007_0490_u32 as i32) => Reading::Available(false),
        Err(e) => Reading::Unavailable(format!("Windows would not list the audio sessions: {e}")),
    }
}

fn read_audio() -> windows::core::Result<bool> {
    // SAFETY: COM calls on this thread only; every interface is released when it drops; a repeated initialisation answers S_FALSE.
    unsafe {
        // S_FALSE (already initialised) is success; a different apartment model (RPC_E_CHANGED_MODE) is logged and the calls below say if COM works.
        paddock_proc::cleanup::logged(
            "initialising COM for the audio reading",
            CoInitializeEx(None, COINIT_MULTITHREADED).ok(),
        );
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
        let sessions = manager.GetSessionEnumerator()?;
        for i in 0..sessions.GetCount()? {
            let control = sessions.GetSession(i)?;
            if control.GetState()? != AudioSessionStateActive {
                continue;
            }
            let control2: IAudioSessionControl2 = control.cast()?;
            // S_OK from IsSystemSoundsSession means it is the system sounds session.
            if control2.IsSystemSoundsSession().is_ok() {
                continue;
            }
            let meter: IAudioMeterInformation = control.cast()?;
            if meter.GetPeakValue()? > SILENCE_PEAK {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
