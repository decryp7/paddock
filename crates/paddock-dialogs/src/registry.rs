//! Which dialog windows are open: the bookkeeping behind the Tauri adapter, as a pure value with its clock passed in,
//! so the close-request, required-choice and unresponsive-page rules are tested without windows or sleeps.
//!
//! A dialog belongs to a host window (`main` or `w-<n>` of the shell's label scheme, `paddock_commands::labels`): its
//! label is `d-<host>-<n>`, it never outlives the host, and at most [`MAX_OPEN_PER_HOST`] are open per host. One window
//! exists per dialog id, opener and instance key: opening it again focuses the existing one.

use crate::contract::{Contracts, Grant, Kind, Modality};
use crate::placement::Bounds;
use crate::window::{DEFAULT_ROUTE, DialogError, DialogErrorCode, MAX_OPEN_PER_HOST};
use paddock_commands::labels::{DIALOG_PREFIX, Label};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// A dialog that did not answer a close request this long is closed on the next request (a hung webview must not trap
/// the user).
pub const CLOSE_GRACE: Duration = Duration::from_millis(1500);

/// One open dialog window.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenDialog {
    /// The top-level window it belongs to.
    pub host: String,
    /// The window that opened it and receives its answer (the host, a pop-out or another dialog).
    pub parent: String,
    pub dialog_id: String,
    pub instance: Option<String>,
    pub title: String,
    pub kind: Kind,
    pub modality: Modality,
    pub required_choice: bool,
    /// Handed to its webview by `dialog_take_input` (again after a reload).
    pub input: Option<Value>,
    /// Where it is (logical px of its monitor), from its last move or resize.
    pub bounds: Option<Bounds>,
    /// When a close request was last forwarded to it and not answered.
    pub asked: Option<Instant>,
    /// The window watchdog found its page not answering: nobody can make a required choice, so the OS close may.
    pub unresponsive: bool,
}

impl OpenDialog {
    /// A dialog opened by `parent` for `host`, as its contract says.
    pub fn new(
        host: impl Into<String>,
        parent: impl Into<String>,
        dialog_id: impl Into<String>,
        contract: &crate::contract::Contract,
    ) -> Self {
        OpenDialog {
            host: host.into(),
            parent: parent.into(),
            dialog_id: dialog_id.into(),
            instance: None,
            title: String::new(),
            kind: contract.kind,
            modality: contract.modality,
            required_choice: contract.required_choice,
            input: None,
            bounds: None,
            asked: None,
            unresponsive: false,
        }
    }
}

/// What a close request (the title bar is gone, so: Cmd/Ctrl+W, the window manager, the OS) does to a dialog window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDecision {
    /// Let the window close (not a dialog this layer knows, a dialog that did not answer in time, or a required choice
    /// whose page does not answer).
    Close,
    /// Keep it and forward the request to the dialog as its Close/Cancel (it decides: unsaved edits).
    Ask,
    /// Keep it: a required choice is made in its content only.
    Keep,
}

/// Which dialog windows exist, keyed by label, and the contracts that govern them. The Tauri adapter manages one.
#[derive(Debug)]
pub struct Dialogs {
    map: Mutex<BTreeMap<String, OpenDialog>>,
    next: AtomicU32,
    contracts: Arc<Contracts>,
    route: String,
    exiting: AtomicBool,
}

impl Dialogs {
    /// Dialogs governed by `contracts`, loaded from [`DEFAULT_ROUTE`].
    pub fn new(contracts: Arc<Contracts>) -> Self {
        Self::with_route(contracts, DEFAULT_ROUTE)
    }

    /// Dialogs governed by `contracts`, loaded from the app's `route` (`index.html#/dialog`; the shell appends the
    /// dialog id, its host and its label as query parameters).
    pub fn with_route(contracts: Arc<Contracts>, route: impl Into<String>) -> Self {
        Dialogs {
            map: Mutex::default(),
            next: AtomicU32::default(),
            contracts,
            route: route.into(),
            exiting: AtomicBool::new(false),
        }
    }

    /// Dialogs governed by the owners' static tables (the shell's, a feature's, the app's): two tables declaring one id
    /// is a build defect, reported to the caller.
    pub fn from_tables(
        tables: &[&crate::DialogTable],
    ) -> Result<Self, crate::contract::ContractError> {
        Contracts::from_tables(tables).map(|c| Self::new(Arc::new(c)))
    }

    /// The contracts.
    pub fn contracts(&self) -> &Arc<Contracts> {
        &self.contracts
    }

    /// The route dialogs load.
    pub fn route(&self) -> &str {
        &self.route
    }

    fn lock(&self) -> MutexGuard<'_, BTreeMap<String, OpenDialog>> {
        self.map.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// What the window `label` may call, from this layer's own record of the dialog it shows (never the webview's
    /// word): the command guard's hook. `None` for a window that is not an open dialog.
    pub fn grant_of(&self, label: &str) -> Option<Grant> {
        self.lock()
            .get(label)
            .map(|e| Grant::new(self.contracts.clone(), e.dialog_id.clone()))
    }

    /// The window of `dialog_id` (and `instance`) opened by `parent`, if there is one.
    pub fn find(&self, parent: &str, dialog_id: &str, instance: Option<&str>) -> Option<String> {
        self.lock()
            .iter()
            .find(|(_, e)| {
                e.parent == parent && e.dialog_id == dialog_id && e.instance.as_deref() == instance
            })
            .map(|(l, _)| l.clone())
    }

    /// Records a new dialog and gives it its label `d-<host>-<n>`; refuses a host outside the label scheme and more
    /// than [`MAX_OPEN_PER_HOST`] dialogs per host.
    pub fn reserve(&self, entry: OpenDialog) -> Result<String, DialogError> {
        if !matches!(
            Label::parse(&entry.host),
            Some(Label::Main | Label::Window(_))
        ) {
            return Err(DialogError::new(
                DialogErrorCode::NotADialog,
                format!("{:?} is not a window that can own dialogs", entry.host),
            ));
        }
        let mut map = self.lock();
        if map.values().filter(|e| e.host == entry.host).count() >= MAX_OPEN_PER_HOST {
            return Err(DialogError::new(
                DialogErrorCode::Limit,
                format!("At most {MAX_OPEN_PER_HOST} dialogs can be open at once; close one first"),
            ));
        }
        let n = self.next.fetch_add(1, Ordering::SeqCst).wrapping_add(1);
        let label = format!("{DIALOG_PREFIX}{}-{n}", entry.host);
        map.insert(label.clone(), entry);
        Ok(label)
    }

    /// Forgets a dialog window.
    pub fn remove(&self, label: &str) -> Option<OpenDialog> {
        self.lock().remove(label)
    }

    /// The record of a dialog window.
    pub fn get(&self, label: &str) -> Option<OpenDialog> {
        self.lock().get(label).cloned()
    }

    /// Labels of the host's dialogs, sorted.
    pub fn of_host(&self, host: &str) -> Vec<String> {
        self.lock()
            .iter()
            .filter(|(_, e)| e.host == host)
            .map(|(l, _)| l.clone())
            .collect()
    }

    /// How many modal dialogs `parent` has open (the parent is re-enabled when the last one closes).
    pub fn modal_children_of(&self, parent: &str) -> usize {
        self.lock()
            .values()
            .filter(|e| e.parent == parent && e.modality == Modality::Modal)
            .count()
    }

    /// A close request reached the dialog window `label` at `now`: a required choice stays (unless its page does not
    /// answer); any other dialog is asked, and one that did not answer within [`CLOSE_GRACE`] of being asked closes.
    pub fn close_request(&self, label: &str, now: Instant) -> CloseDecision {
        let mut map = self.lock();
        let Some(entry) = map.get_mut(label) else {
            return CloseDecision::Close;
        };
        if entry.required_choice {
            // No Cancel to map the request to: the dialog stays until it is answered, unless its page cannot answer.
            return if entry.unresponsive {
                CloseDecision::Close
            } else {
                CloseDecision::Keep
            };
        }
        if entry
            .asked
            .is_some_and(|t| now.saturating_duration_since(t) >= CLOSE_GRACE)
        {
            return CloseDecision::Close;
        }
        entry.asked = Some(now);
        CloseDecision::Ask
    }

    /// The window watchdog found the page of `label` not answering (`true`) or answering again. Returns whether `label`
    /// is an open dialog.
    pub fn set_unresponsive(&self, label: &str, unresponsive: bool) -> bool {
        self.lock()
            .get_mut(label)
            .map(|e| e.unresponsive = unresponsive)
            .is_some()
    }

    /// The dialog window `label` moved or was resized to `bounds`.
    pub fn set_bounds(&self, label: &str, bounds: Option<Bounds>) {
        if let Some(e) = self.lock().get_mut(label) {
            e.bounds = bounds;
        }
    }

    /// The app is exiting: closing dialogs then neither refocus their parents nor report to them.
    pub fn set_exiting(&self, exiting: bool) {
        self.exiting.store(exiting, Ordering::SeqCst);
    }

    /// Whether the app is exiting.
    pub fn exiting(&self) -> bool {
        self.exiting.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_commands::DialogGrant;

    crate::dialogs! {
        static TABLE;
        "settings" => Tool { commands: &[paddock_commands::DialogCallable::Command("settings_set")] },
        "close-prompt" => Decision { required_choice: true },
        "ask" => Decision,
    }

    fn contracts() -> Arc<Contracts> {
        Arc::new(Contracts::from_tables(&[&TABLE]).unwrap())
    }

    fn entry(d: &Dialogs, host: &str, parent: &str, id: &str) -> OpenDialog {
        OpenDialog::new(host, parent, id, d.contracts().get(id).unwrap())
    }

    #[test]
    fn labels_are_per_host_and_dialogs_are_found_by_opener_id_and_instance() {
        let d = Dialogs::new(contracts());
        assert_eq!(d.route(), DEFAULT_ROUTE);
        let a = d.reserve(entry(&d, "main", "main", "settings")).unwrap();
        let mut keyed = entry(&d, "w-2", "w-2", "settings");
        keyed.instance = Some("k1".into());
        let b = d.reserve(keyed).unwrap();
        let c = d.reserve(entry(&d, "main", "d-main-1", "ask")).unwrap();
        assert_eq!(
            (a.as_str(), b.as_str(), c.as_str()),
            ("d-main-1", "d-w-2-2", "d-main-3")
        );
        assert_eq!(d.find("main", "settings", None), Some(a.clone()));
        assert_eq!(d.find("w-2", "settings", Some("k1")), Some(b.clone()));
        assert_eq!(d.find("w-2", "settings", None), None);
        assert_eq!(d.of_host("main"), [a.clone(), c.clone()]);
        assert_eq!(d.modal_children_of("d-main-1"), 1);
        assert_eq!(d.modal_children_of("main"), 0);
        assert_eq!(d.get(&a).map(|e| e.kind), Some(Kind::Tool));
        assert!(d.remove(&a).is_some() && d.get(&a).is_none() && d.remove(&a).is_none());
    }

    #[test]
    fn only_top_level_windows_own_dialogs_and_at_most_eight_each() {
        let d = Dialogs::new(contracts());
        for host in ["d-main-1", "p-main-1", "browser", ""] {
            let e = d.reserve(entry(&d, host, host, "ask")).unwrap_err();
            assert_eq!(e.code, DialogErrorCode::NotADialog, "{host}");
        }
        for _ in 0..MAX_OPEN_PER_HOST {
            d.reserve(entry(&d, "main", "main", "ask")).unwrap();
        }
        let e = d.reserve(entry(&d, "main", "main", "ask")).unwrap_err();
        assert!(
            e.code == DialogErrorCode::Limit && e.message.contains("At most 8"),
            "{e}"
        );
        assert!(
            d.reserve(entry(&d, "w-1", "w-1", "ask")).is_ok(),
            "per host"
        );
    }

    #[test]
    fn the_grant_comes_from_the_record_never_from_the_webview() {
        let d = Dialogs::new(contracts());
        let l = d.reserve(entry(&d, "main", "main", "settings")).unwrap();
        let g = d.grant_of(&l).expect("an open dialog has a grant");
        assert_eq!(g.dialog(), "settings");
        assert!(g.allows("settings_set") && g.allows("dialog_close") && !g.allows("app_quit"));
        assert!(d.grant_of("d-main-9").is_none() && d.grant_of("main").is_none());
    }

    #[test]
    fn a_close_request_goes_to_the_dialog_first_and_a_hung_dialog_is_closed_on_the_next() {
        let d = Dialogs::new(contracts());
        let l = d.reserve(entry(&d, "main", "main", "settings")).unwrap();
        let t0 = Instant::now();
        assert_eq!(
            d.close_request(&l, t0),
            CloseDecision::Ask,
            "the dialog decides"
        );
        assert_eq!(d.get(&l).unwrap().asked, Some(t0));
        let soon = t0 + CLOSE_GRACE / 2;
        assert_eq!(
            d.close_request(&l, soon),
            CloseDecision::Ask,
            "asked again within the grace"
        );
        assert_eq!(
            d.close_request(&l, soon + CLOSE_GRACE),
            CloseDecision::Close,
            "no answer: closed anyway"
        );
        // A clock that went backwards never closes early.
        let l2 = d.reserve(entry(&d, "main", "main", "ask")).unwrap();
        assert_eq!(d.close_request(&l2, soon), CloseDecision::Ask);
        assert_eq!(d.close_request(&l2, t0), CloseDecision::Ask);
        assert_eq!(
            d.close_request("d-main-9", t0),
            CloseDecision::Close,
            "unknown windows just close"
        );
    }

    #[test]
    fn a_required_choice_stays_until_answered_unless_its_page_hangs() {
        let d = Dialogs::new(contracts());
        let l = d
            .reserve(entry(&d, "main", "main", "close-prompt"))
            .unwrap();
        let t0 = Instant::now();
        assert_eq!(d.close_request(&l, t0), CloseDecision::Keep);
        assert_eq!(
            d.close_request(&l, t0 + CLOSE_GRACE * 4),
            CloseDecision::Keep,
            "never closed behind the user's back"
        );
        assert!(
            d.get(&l).unwrap().asked.is_none(),
            "nothing to cancel, so it is not even asked"
        );
        assert!(d.set_unresponsive(&l, true));
        assert_eq!(
            d.close_request(&l, t0),
            CloseDecision::Close,
            "the OS close ends a hung page"
        );
        assert!(d.set_unresponsive(&l, false));
        assert_eq!(
            d.close_request(&l, t0),
            CloseDecision::Keep,
            "answering again: required again"
        );
        assert!(!d.set_unresponsive("d-main-9", true));
    }

    #[test]
    fn bounds_exiting_and_a_poisoned_lock_are_handled() {
        let d = Arc::new(Dialogs::with_route(contracts(), "app.html#/d"));
        assert_eq!(d.route(), "app.html#/d");
        let l = d.reserve(entry(&d, "main", "main", "ask")).unwrap();
        let b = Bounds {
            x: 1.0,
            y: 2.0,
            width: 3.0,
            height: 4.0,
        };
        d.set_bounds(&l, Some(b));
        d.set_bounds("d-main-9", Some(b));
        assert_eq!(d.get(&l).unwrap().bounds, Some(b));
        assert!(!d.exiting());
        d.set_exiting(true);
        assert!(d.exiting());
        let poison = d.clone();
        let _ = std::thread::spawn(move || {
            let _held = poison.lock();
            panic!("poison the lock");
        })
        .join();
        assert!(d.get(&l).is_some(), "a poisoned lock recovers");
        assert!(format!("{d:?}").contains("Dialogs"));
    }

    #[test]
    fn static_tables_build_the_dialogs_or_say_why_not() {
        let d = Dialogs::from_tables(&[&TABLE]).unwrap();
        assert_eq!(d.contracts().ids().count(), 3);
        assert!(
            Dialogs::from_tables(&[&TABLE, &TABLE]).is_err(),
            "one id declared twice"
        );
    }
}
