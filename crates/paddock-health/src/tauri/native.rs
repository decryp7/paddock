//! The OS port over `tauri-plugin-notification` (feature `os-notify`; the app registers the plugin). Native glue only:
//! every decision (policy, limits, buttons, routing) is in `notify::os` and tested there and with a fake toaster; this file
//! turns a toast into the plugin's builder and the plugin's click report into a [`Click`]. Tests here never show a
//! toast: they build the pieces and read a report from JSON.

use crate::notify::centre::NoticeId;
use crate::notify::os::{
    Click, ClickSink, OsToast, PAYLOAD_KEY, Permission, Target, ToastError, Toaster,
};
use ::tauri::plugin::PermissionState;
use ::tauri::{AppHandle, Runtime};
use tauri_plugin_notification::{Action, ActionPerformed, ActionType, NotificationExt};

/// Shows toasts through the notification plugin.
pub struct PluginToaster<R: Runtime>(AppHandle<R>);

impl<R: Runtime> PluginToaster<R> {
    /// The toaster of this app.
    pub fn new(app: AppHandle<R>) -> PluginToaster<R> {
        PluginToaster(app)
    }
}

/// The plugin's action type of a toast's buttons (registered before the toast is shown; registering again is harmless).
pub fn action_type_of(toast: &OsToast) -> Option<ActionType> {
    let id = toast.action_type()?;
    let actions = toast
        .buttons
        .iter()
        .map(|b| Action::builder(b.slot.id(), &b.label).build())
        .collect();
    Some(ActionType::builder(id).actions(actions).build())
}

/// The click a report from the plugin describes: the message id comes from the toast's payload and the button from its
/// id; anything else (another app's toast, a damaged payload) is nothing.
pub fn click_of(performed: &ActionPerformed) -> Option<Click> {
    let id = performed
        .notification()?
        .extra()
        .get(PAYLOAD_KEY)?
        .as_u64()?;
    Some(Click {
        notice: NoticeId(id),
        target: Target::from_id(performed.action_id())?,
    })
}

impl<R: Runtime> Toaster for PluginToaster<R> {
    fn show(&self, toast: &OsToast) -> Result<(), ToastError> {
        let n = self.0.notification();
        let mut builder = n
            .builder()
            .title(&toast.title)
            .body(&toast.body)
            .extra(PAYLOAD_KEY, toast.notice.0);
        if let Some(kind) = action_type_of(toast) {
            let id = kind.id().to_string();
            n.register_action_types(vec![kind])
                .map_err(|e| ToastError(e.to_string()))?;
            builder = builder.action_type_id(id);
        }
        builder.show().map_err(|e| ToastError(e.to_string()))
    }

    fn subscribe(&self, sink: ClickSink) {
        let registered = self.0.notification().on_action(move |performed| {
            if let Some(click) = click_of(performed) {
                sink(click);
            }
        });
        if let Err(e) = registered {
            log::warn!(target: "paddock_health", "Clicks on notifications are not reported: {e}");
        }
    }

    fn permission(&self) -> Permission {
        permission_of(self.0.notification().permission_state())
    }

    fn request_permission(&self) -> Permission {
        permission_of(self.0.notification().request_permission())
    }

    fn open_system_settings(&self) -> Result<(), ToastError> {
        open_settings_with(std::env::consts::OS, &spawn_detached)
    }
}

/// The permission a plugin answer means (an error or a prompt state is "not known").
pub fn permission_of(state: ::tauri_plugin_notification::Result<PermissionState>) -> Permission {
    match state {
        Ok(PermissionState::Granted) => Permission::Granted,
        Ok(PermissionState::Denied) => Permission::Denied,
        _ => Permission::Unknown,
    }
}

/// The program and arguments that open the notification settings of `os` (`std::env::consts::OS`), or `None` where the
/// system has no page the app can reach (Linux desktops differ; GNOME's settings are the one that is the same everywhere).
pub fn settings_command(os: &str) -> Option<(&'static str, &'static [&'static str])> {
    match os {
        "macos" => Some((
            "open",
            &["x-apple.systempreferences:com.apple.Notifications-Settings.extension"],
        )),
        "windows" => Some(("explorer.exe", &["ms-settings:notifications"])),
        "linux" => Some(("gnome-control-center", &["notifications"])),
        _ => None,
    }
}

/// Starts a program with arguments (injected so tests start nothing).
pub type Spawn<'a> = dyn Fn(&str, &[&str]) -> Result<(), String> + 'a;

/// Opens the notification settings of `os` through `spawn` (program, arguments).
pub fn open_settings_with(
    os: &str,
    spawn: &Spawn<'_>,
) -> Result<(), ToastError> {
    let Some((program, args)) = settings_command(os) else {
        return Err(ToastError(
            "Open your system's notification settings and allow notifications for this app.".into(),
        ));
    };
    spawn(program, args).map_err(|e| {
        ToastError(format!(
            "The system's notification settings could not be opened ({e}). Open them by hand and allow notifications for this app."
        ))
    })
}

/// Starts a program without waiting for it. Platform glue.
fn spawn_detached(program: &str, args: &[&str]) -> Result<(), String> {
    paddock_proc::command(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::os::{Slot, ToastButton};
    use serde_json::json;

    fn toast(buttons: Vec<ToastButton>) -> OsToast {
        OsToast {
            notice: NoticeId(4),
            cause: "c".into(),
            title: "T".into(),
            body: "B".into(),
            buttons,
        }
    }

    #[test]
    fn the_buttons_become_one_action_type_with_the_slots_as_ids() {
        let t = toast(vec![
            ToastButton {
                slot: Slot::Primary,
                label: "Repair".into(),
            },
            ToastButton {
                slot: Slot::Tertiary,
                label: "Open settings".into(),
            },
        ]);
        let kind = action_type_of(&t).unwrap();
        assert_eq!(kind.id(), t.action_type().unwrap());
        let ids: Vec<_> = kind.actions().iter().map(|a| (a.id(), a.title())).collect();
        assert_eq!(ids, [("primary", "Repair"), ("tertiary", "Open settings")]);
        assert!(action_type_of(&toast(vec![])).is_none());
    }

    fn report(action: &str, extra: serde_json::Value) -> ActionPerformed {
        serde_json::from_value(
            json!({ "actionId": action, "notification": { "id": 1, "extra": extra } }),
        )
        .unwrap()
    }

    #[test]
    fn a_report_names_the_message_and_the_button_and_nothing_else_is_believed() {
        let ok = report("secondary", json!({ "paddockNotice": 12 }));
        assert_eq!(
            click_of(&ok),
            Some(Click {
                notice: NoticeId(12),
                target: Target::Button(Slot::Secondary)
            })
        );
        assert_eq!(
            click_of(&report("tap", json!({ "paddockNotice": 3 }))).map(|c| c.target),
            Some(Target::Toast)
        );
        assert_eq!(click_of(&report("tap", json!({}))), None, "not one of ours");
        assert_eq!(
            click_of(&report("tap", json!({ "paddockNotice": "7" }))),
            None,
            "a string is not an id"
        );
        assert_eq!(
            click_of(&report("tap", json!({ "paddockNotice": -1 }))),
            None
        );
        assert_eq!(
            click_of(&report("rm -rf", json!({ "paddockNotice": 1 }))),
            None,
            "an unknown button"
        );
        let no_notification: ActionPerformed =
            serde_json::from_value(json!({ "actionId": "tap" })).unwrap();
        assert_eq!(click_of(&no_notification), None);
    }

    #[test]
    fn the_plugin_toaster_reports_permission_and_accepts_a_click_subscription_without_showing_anything()
     {
        use ::tauri::test::{mock_builder, mock_context, noop_assets};
        let app = mock_builder()
            .plugin(tauri_plugin_notification::init())
            .build(mock_context(noop_assets()))
            .unwrap();
        let toaster = PluginToaster::new(app.handle().clone());
        // Desktop systems do not ask: always granted.
        assert_eq!(toaster.permission(), Permission::Granted);
        toaster.subscribe(std::sync::Arc::new(|_| {}));
        assert_eq!(toaster.request_permission(), Permission::Granted);
    }

    #[test]
    fn the_permission_follows_the_plugins_answer() {
        assert_eq!(
            permission_of(Ok(PermissionState::Granted)),
            Permission::Granted
        );
        assert_eq!(
            permission_of(Ok(PermissionState::Denied)),
            Permission::Denied
        );
        assert_eq!(
            permission_of(Ok(PermissionState::Prompt)),
            Permission::Unknown
        );
        let failed = Err(::tauri_plugin_notification::Error::from(
            std::io::Error::other("x"),
        ));
        assert_eq!(permission_of(failed), Permission::Unknown);
    }

    #[test]
    fn each_os_opens_its_own_notification_settings_and_a_failure_says_what_to_do() {
        use std::cell::RefCell;
        let calls = RefCell::new(Vec::new());
        let record = |p: &str, a: &[&str]| {
            calls.borrow_mut().push((p.to_string(), a.join(" ")));
            Ok(())
        };
        for os in ["macos", "windows", "linux"] {
            open_settings_with(os, &record).unwrap();
        }
        let calls = calls.into_inner();
        assert_eq!(calls[0].0, "open");
        assert!(calls[0].1.contains("Notifications"));
        assert_eq!(
            calls[1],
            ("explorer.exe".into(), "ms-settings:notifications".into())
        );
        assert_eq!(calls[2].0, "gnome-control-center");
        let unknown = open_settings_with("haiku", &|_, _| Ok(())).unwrap_err();
        assert!(unknown.0.contains("notification settings"));
        let failed =
            open_settings_with("macos", &|_, _| Err("no such program".into())).unwrap_err();
        assert!(failed.0.contains("no such program") && failed.0.contains("by hand"));
    }
}
