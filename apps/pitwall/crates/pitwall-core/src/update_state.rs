//! The state of model update checks (ADR 0017): the schedule setting and when a check is due, the notices the last
//! checks found, and which of them the user dismissed or snoozed. Pure and versioned like the other state files
//! (`model-updates.json`): the server reads the text, this module parses it tolerantly (primary file, then its backup,
//! then empty), and hands back the text to write atomically.
//!
//! **Nothing runs before the user opts in.** The setting is [`UpdateCheckSettings`]: `enabled` is off by default and
//! `schedule` (weekly by default) only matters once it is on. The first scheduled check happens right after the user
//! turns it on; "Check now" always works (rate-limited), whatever the setting.

use crate::time::{Millis, DAY, MINUTE};
use crate::updates::UpdateNotice;
use serde::{Deserialize, Serialize};

/// Schema version of `model-updates.json`.
pub const VERSION: u32 = 1;
/// The largest file read.
pub const MAX_BYTES: usize = 1024 * 1024;
/// Most notices kept.
pub const MAX_NOTICES: usize = 200;
/// Most dismissals kept (the oldest go first).
pub const MAX_DISMISSALS: usize = 500;
/// The shortest gap between two checks the user starts ("Check now").
pub const MIN_GAP: Millis = MINUTE;
/// How long a scheduled check waits after an attempt that failed or was refused before trying again.
pub const RETRY_AFTER_FAILURE: Millis = 60 * MINUTE;
/// The longest a notice can be snoozed, in days.
pub const MAX_SNOOZE_DAYS: u32 = 90;
/// Longest stored status message.
const MAX_MESSAGE: usize = 300;

/// How often the scheduled check runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateSchedule {
    /// Every seven days.
    #[default]
    Weekly,
    /// Every day.
    Daily,
}

impl UpdateSchedule {
    /// The time between scheduled checks.
    pub fn interval(self) -> Millis {
        match self {
            UpdateSchedule::Weekly => 7 * DAY,
            UpdateSchedule::Daily => DAY,
        }
    }
}

/// The update-check setting (`models.updateCheck` in `settings.toml`, `UpdateCheckSettings` in `api.types.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateCheckSettings {
    /// The scheduled check is on. Off until the user turns it on.
    pub enabled: bool,
    /// How often it runs once on.
    pub schedule: UpdateSchedule,
}

/// A hidden notice: dismissed for good (until the repository offers something different) or snoozed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dismissal {
    /// The notice id.
    pub id: String,
    /// What was dismissed (see `UpdateNotice::fingerprint`).
    pub fingerprint: String,
    /// When a snooze ends; `None` hides this offer until it changes.
    pub until: Option<Millis>,
    /// When the user did it.
    pub at: Millis,
}

/// Why a notice could not be dismissed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DismissError {
    /// No notice with this id (it may already be gone).
    #[error("there is no update notice \"{0}\" (it may already have been dealt with)")]
    NotFound(String),
    /// The snooze length is out of range.
    #[error("a snooze lasts between 1 and {MAX_SNOOZE_DAYS} days (it was {0})")]
    BadSnooze(u32),
}

/// The check that has just finished, as the state needs it.
#[derive(Debug, Clone)]
pub struct Finished<'a> {
    /// When it finished.
    pub at: Millis,
    /// Ids of the models whose repositories were answered (their old notices are replaced).
    pub checked: &'a [String],
    /// Ids of every installed model (notices for models that are gone are dropped).
    pub installed: &'a [String],
    /// What it found.
    pub found: Vec<UpdateNotice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct File {
    version: u32,
    last_checked_at: Option<Millis>,
    last_attempt_at: Option<Millis>,
    last_message: Option<String>,
    notices: Vec<UpdateNotice>,
    dismissals: Vec<Dismissal>,
}

/// Notices, dismissals and when the checks last ran.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateState {
    /// When a check last completed.
    pub last_checked_at: Option<Millis>,
    /// When a check was last started (completed or not).
    pub last_attempt_at: Option<Millis>,
    /// How the last attempt went, in a sentence.
    pub last_message: Option<String>,
    notices: Vec<UpdateNotice>,
    dismissals: Vec<Dismissal>,
}

fn parse_one(text: &str) -> Option<UpdateState> {
    let f: File = serde_json::from_str(text).ok()?;
    // A file from a newer Pitwall is not guessed at.
    if f.version == 0 || f.version > VERSION {
        return None;
    }
    let mut notices: Vec<UpdateNotice> = vec![];
    for n in f.notices {
        if !n.id.is_empty() && !notices.iter().any(|o| o.id == n.id) && notices.len() < MAX_NOTICES {
            notices.push(n);
        }
    }
    let skip = f.dismissals.len().saturating_sub(MAX_DISMISSALS);
    Some(UpdateState {
        last_checked_at: f.last_checked_at,
        last_attempt_at: f.last_attempt_at,
        last_message: f.last_message.map(|m| m.chars().take(MAX_MESSAGE).collect()),
        notices,
        dismissals: f.dismissals.into_iter().skip(skip).collect(),
    })
}

impl UpdateState {
    /// Reads the state: the primary file, else the backup, else empty. The flag says a file was there but could not
    /// be read (the caller tells the user; the next check rebuilds the notices).
    pub fn load(primary: Option<&str>, backup: Option<&str>) -> (UpdateState, bool) {
        if let Some(s) = primary.and_then(parse_one).or_else(|| backup.and_then(parse_one)) {
            return (s, false);
        }
        (UpdateState::default(), primary.is_some() || backup.is_some())
    }

    /// The file's text.
    pub fn to_json(&self) -> String {
        let f = File {
            version: VERSION,
            last_checked_at: self.last_checked_at,
            last_attempt_at: self.last_attempt_at,
            last_message: self.last_message.clone(),
            notices: self.notices.clone(),
            dismissals: self.dismissals.clone(),
        };
        serde_json::to_string(&f).unwrap_or_default()
    }

    fn recent_failure(&self, now: Millis) -> bool {
        self.last_attempt_at
            .is_some_and(|a| a <= now && now - a < RETRY_AFTER_FAILURE && self.last_checked_at != self.last_attempt_at)
    }

    /// Whether the scheduled check should run now. Off until enabled; the first run is right after enabling; then
    /// every interval after the last completed check; after a failed or refused attempt it waits
    /// [`RETRY_AFTER_FAILURE`]. A clock that went backwards (last check in the future) counts as due.
    pub fn due(&self, s: &UpdateCheckSettings, now: Millis) -> bool {
        if !s.enabled || self.recent_failure(now) {
            return false;
        }
        match self.last_checked_at {
            None => true,
            Some(t) => t > now || now - t >= s.schedule.interval(),
        }
    }

    /// When the next scheduled check is due (`None` when the schedule is off; `now` when it is due already).
    pub fn next_due(&self, s: &UpdateCheckSettings, now: Millis) -> Option<Millis> {
        if !s.enabled {
            return None;
        }
        let by_schedule = self
            .last_checked_at
            .filter(|t| *t <= now)
            .map_or(now, |t| t.saturating_add(s.schedule.interval()));
        let retry = self
            .last_attempt_at
            .filter(|_| self.recent_failure(now))
            .map_or(0, |a| a.saturating_add(RETRY_AFTER_FAILURE));
        Some(by_schedule.max(retry).max(now))
    }

    /// Whether the user may start a check now: `Err` carries the seconds to wait.
    pub fn manual_allowed(&self, now: Millis) -> Result<(), u64> {
        match self.last_attempt_at {
            Some(a) if a <= now && now - a < MIN_GAP => Err((MIN_GAP - (now - a)).div_ceil(1000)),
            _ => Ok(()),
        }
    }

    /// Notes that an attempt started at `now` and how it ended (when it did not complete).
    pub fn attempted(&mut self, now: Millis, message: Option<&str>) {
        self.last_attempt_at = Some(now);
        if let Some(m) = message {
            self.last_message = Some(m.chars().take(MAX_MESSAGE).collect());
        }
    }

    /// Applies a finished check: the notices of the models that were answered are replaced by what was found (a
    /// notice that was already known keeps its first-found time), notices of models that are no longer installed go,
    /// and dismissals that no longer apply are forgotten.
    pub fn finish(&mut self, run: Finished<'_>, message: &str) {
        let known: Vec<UpdateNotice> = std::mem::take(&mut self.notices);
        let mut kept: Vec<UpdateNotice> = known
            .iter()
            .filter(|n| run.installed.contains(&n.model_id) && !run.checked.contains(&n.model_id))
            .cloned()
            .collect();
        for mut n in run.found {
            if let Some(old) = known.iter().find(|o| o.id == n.id && o.fingerprint() == n.fingerprint()) {
                n.found_at = old.found_at;
            }
            kept.push(n);
        }
        kept.truncate(MAX_NOTICES);
        self.notices = kept;
        self.last_checked_at = Some(run.at);
        self.last_attempt_at = Some(run.at);
        self.last_message = Some(message.chars().take(MAX_MESSAGE).collect());
        let notices = &self.notices;
        self.dismissals
            .retain(|d| notices.iter().any(|n| n.id == d.id) && d.until.is_none_or(|u| u > run.at));
    }

    /// A download of `file` from `repo` finished: the notices that offered it are done (the next check looks again).
    /// Returns how many went.
    pub fn downloaded(&mut self, repo: &str, file: &str) -> usize {
        let before = self.notices.len();
        self.notices.retain(|n| !(n.repo == repo && n.file == file));
        before - self.notices.len()
    }

    /// Every notice from the last checks, dismissed or not.
    pub fn all(&self) -> &[UpdateNotice] {
        &self.notices
    }

    /// The notices to show now: not dismissed for this exact offer, not snoozed.
    pub fn visible(&self, now: Millis) -> Vec<UpdateNotice> {
        self.notices
            .iter()
            .filter(|n| {
                !self
                    .dismissals
                    .iter()
                    .any(|d| d.id == n.id && d.fingerprint == n.fingerprint() && d.until.is_none_or(|u| u > now))
            })
            .cloned()
            .collect()
    }

    /// Hides a notice: until the repository offers something different (`snooze_days` `None`) or for that many days.
    pub fn dismiss(&mut self, id: &str, snooze_days: Option<u32>, now: Millis) -> Result<(), DismissError> {
        if let Some(d) = snooze_days.filter(|d| *d == 0 || *d > MAX_SNOOZE_DAYS) {
            return Err(DismissError::BadSnooze(d));
        }
        let n = self
            .notices
            .iter()
            .find(|n| n.id == id)
            .ok_or_else(|| DismissError::NotFound(id.to_string()))?;
        let d = Dismissal {
            id: id.to_string(),
            fingerprint: n.fingerprint(),
            until: snooze_days.map(|d| now.saturating_add(u64::from(d) * DAY)),
            at: now,
        };
        self.dismissals.retain(|o| o.id != id);
        self.dismissals.push(d);
        if self.dismissals.len() > MAX_DISMISSALS {
            let excess = self.dismissals.len() - MAX_DISMISSALS;
            self.dismissals.drain(..excess);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::HOUR;
    use crate::updates::UpdateKind;

    fn n(model: &str, file: &str, size: u64, at: Millis) -> UpdateNotice {
        UpdateNotice {
            id: format!("{model}#betterQuant"),
            model_id: model.into(),
            repo: "acme/Tiny-GGUF".into(),
            kind: UpdateKind::BetterQuant,
            file: file.into(),
            quant: Some("Q5_K_M".into()),
            size_bytes: size,
            installed_file: "t-Q4_K_M.gguf".into(),
            installed_quant: Some("Q4_K_M".into()),
            summary: "s".into(),
            found_at: at,
            target_sha256: None,
        }
    }

    fn run<'a>(at: Millis, checked: &'a [String], installed: &'a [String], found: Vec<UpdateNotice>) -> Finished<'a> {
        Finished {
            at,
            checked,
            installed,
            found,
        }
    }

    fn on(schedule: UpdateSchedule) -> UpdateCheckSettings {
        UpdateCheckSettings { enabled: true, schedule }
    }

    #[test]
    fn nothing_is_due_until_the_user_turns_it_on_then_the_first_run_is_immediate() {
        let st = UpdateState::default();
        let s = UpdateCheckSettings::default();
        assert!(!s.enabled && s.schedule == UpdateSchedule::Weekly, "off by default, weekly once on");
        assert!(!st.due(&s, 5 * DAY) && st.next_due(&s, 5 * DAY).is_none());
        assert!(st.due(&on(UpdateSchedule::Weekly), 5 * DAY));
        assert_eq!(st.next_due(&on(UpdateSchedule::Weekly), 5 * DAY), Some(5 * DAY));
    }

    #[test]
    fn weekly_and_daily_schedules_run_after_their_interval() {
        let mut st = UpdateState::default();
        st.finish(run(10 * DAY, &[], &[], vec![]), "ok");
        let (w, d) = (on(UpdateSchedule::Weekly), on(UpdateSchedule::Daily));
        assert!(!st.due(&w, 10 * DAY + 6 * DAY) && st.due(&w, 10 * DAY + 7 * DAY));
        assert!(!st.due(&d, 10 * DAY + 23 * HOUR) && st.due(&d, 11 * DAY));
        assert_eq!(st.next_due(&d, 10 * DAY + HOUR), Some(11 * DAY));
        assert!(st.due(&w, 3 * DAY), "a clock that went backwards does not stall the schedule");
        assert_eq!(st.next_due(&w, 3 * DAY), Some(3 * DAY));
    }

    #[test]
    fn a_failed_attempt_retries_after_an_hour_not_every_tick() {
        let mut st = UpdateState::default();
        let s = on(UpdateSchedule::Daily);
        st.attempted(1_000, Some("Hugging Face did not answer"));
        assert!(!st.due(&s, 1_000 + 59 * MINUTE));
        assert!(st.due(&s, 1_000 + 61 * MINUTE));
        assert_eq!(st.next_due(&s, 1_000 + 10 * MINUTE), Some(1_000 + RETRY_AFTER_FAILURE));
        assert_eq!(st.last_message.as_deref(), Some("Hugging Face did not answer"));
    }

    #[test]
    fn check_now_is_rate_limited() {
        let mut st = UpdateState::default();
        assert!(st.manual_allowed(0).is_ok());
        st.attempted(100_000, None);
        assert_eq!(st.manual_allowed(100_000 + 20_000), Err(40));
        assert!(st.manual_allowed(100_000 + MIN_GAP).is_ok());
        assert!(st.manual_allowed(50_000).is_ok(), "a clock that went backwards is not a rate limit");
    }

    #[test]
    fn a_finished_check_replaces_answered_models_keeps_others_and_drops_removed_ones() {
        let ids: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let mut st = UpdateState::default();
        st.finish(
            run(
                1_000,
                &ids,
                &ids,
                vec![n("a", "x", 1, 1_000), n("b", "y", 1, 1_000), n("c", "z", 1, 1_000)],
            ),
            "3 models",
        );
        assert_eq!(st.all().len(), 3);
        // Next: only "a" answered (b's repository failed), "c" was removed, "a" has the same finding again.
        let installed: Vec<String> = vec!["a".into(), "b".into()];
        let checked: Vec<String> = vec!["a".into()];
        st.finish(run(9_000, &checked, &installed, vec![n("a", "x", 1, 9_000)]), "1 model");
        let got: Vec<(&str, u64)> = st.all().iter().map(|x| (x.model_id.as_str(), x.found_at)).collect();
        assert_eq!(
            got,
            vec![("b", 1_000), ("a", 1_000)],
            "b kept, c gone, a keeps its first-found time"
        );
        // A changed offer for a model is a new finding with a new time.
        st.finish(run(20_000, &checked, &installed, vec![n("a", "x2", 2, 20_000)]), "");
        assert_eq!(st.all().iter().find(|x| x.model_id == "a").map(|x| x.found_at), Some(20_000));
        // Answered and found nothing: its notice is gone.
        st.finish(run(30_000, &checked, &installed, vec![]), "");
        assert!(st.all().iter().all(|x| x.model_id != "a"));
        assert_eq!((st.last_checked_at, st.last_attempt_at), (Some(30_000), Some(30_000)));
    }

    #[test]
    fn dismissing_hides_this_offer_until_it_changes() {
        let ids: Vec<String> = vec!["a".into()];
        let mut st = UpdateState::default();
        st.finish(run(1_000, &ids, &ids, vec![n("a", "x", 1, 1_000)]), "");
        assert_eq!(st.visible(2_000).len(), 1);
        st.dismiss("a#betterQuant", None, 2_000).unwrap();
        assert!(st.visible(2_000 + 400 * DAY).is_empty(), "a plain dismissal does not expire");
        // The same offer found again stays dismissed; a different offer comes back.
        st.finish(run(3_000, &ids, &ids, vec![n("a", "x", 1, 3_000)]), "");
        assert!(st.visible(3_000).is_empty());
        st.finish(run(4_000, &ids, &ids, vec![n("a", "x-bigger", 2, 4_000)]), "");
        assert_eq!(st.visible(4_000).len(), 1);
        // Gone for good once the notice is gone (nothing accumulates).
        st.dismiss("a#betterQuant", None, 4_500).unwrap();
        st.finish(run(5_000, &ids, &ids, vec![]), "");
        assert!(st.to_json().contains("\"dismissals\":[]"));
    }

    #[test]
    fn snoozing_hides_for_the_given_days_only() {
        let ids: Vec<String> = vec!["a".into()];
        let mut st = UpdateState::default();
        st.finish(run(0, &ids, &ids, vec![n("a", "x", 1, 0)]), "");
        st.dismiss("a#betterQuant", Some(7), 1_000).unwrap();
        assert!(st.visible(1_000 + 6 * DAY).is_empty());
        assert_eq!(st.visible(1_000 + 7 * DAY).len(), 1);
        // A later check after the snooze ended forgets it.
        st.finish(run(1_000 + 8 * DAY, &ids, &ids, vec![n("a", "x", 1, 0)]), "");
        assert_eq!(st.visible(1_000 + 8 * DAY).len(), 1);
        // Re-dismissing replaces the earlier dismissal.
        st.dismiss("a#betterQuant", Some(1), 2_000 * DAY).unwrap();
        st.dismiss("a#betterQuant", None, 2_000 * DAY).unwrap();
        assert_eq!(st.to_json().matches("\"fingerprint\"").count(), 1);
    }

    #[test]
    fn a_finished_download_clears_the_notices_that_offered_it() {
        let ids: Vec<String> = vec!["a".into(), "b".into()];
        let mut st = UpdateState::default();
        st.finish(run(0, &ids, &ids, vec![n("a", "x", 1, 0), n("b", "y", 1, 0)]), "");
        assert_eq!(st.downloaded("acme/Tiny-GGUF", "x"), 1);
        assert_eq!(
            st.downloaded("other/repo", "y"),
            0,
            "another repository's file of the same name is not it"
        );
        assert_eq!(st.all().iter().map(|n| n.model_id.as_str()).collect::<Vec<_>>(), vec!["b"]);
    }

    #[test]
    fn dismiss_validates_its_arguments() {
        let ids: Vec<String> = vec!["a".into()];
        let mut st = UpdateState::default();
        st.finish(run(0, &ids, &ids, vec![n("a", "x", 1, 0)]), "");
        assert_eq!(st.dismiss("nope", None, 1), Err(DismissError::NotFound("nope".into())));
        assert_eq!(st.dismiss("a#betterQuant", Some(0), 1), Err(DismissError::BadSnooze(0)));
        assert_eq!(st.dismiss("a#betterQuant", Some(91), 1), Err(DismissError::BadSnooze(91)));
        assert!(st.dismiss("a#betterQuant", Some(90), 1).is_ok());
        assert!(DismissError::BadSnooze(0).to_string().contains("1 and 90"));
        assert!(DismissError::NotFound("q".into()).to_string().contains("\"q\""));
    }

    #[test]
    fn dismissals_are_bounded() {
        let mut st = UpdateState::default();
        let ids: Vec<String> = (0..3).map(|i| format!("m{i}")).collect();
        st.finish(run(0, &ids, &ids, ids.iter().map(|m| n(m, "x", 1, 0)).collect()), "");
        st.dismissals = (0..MAX_DISMISSALS + 5)
            .map(|i| Dismissal {
                id: format!("old{i}"),
                fingerprint: String::new(),
                until: None,
                at: 0,
            })
            .collect();
        st.dismiss("m0#betterQuant", None, 5).unwrap();
        assert_eq!(st.dismissals.len(), MAX_DISMISSALS);
        assert_eq!(st.dismissals.last().map(|d| d.id.as_str()), Some("m0#betterQuant"));
    }

    #[test]
    fn the_file_round_trips_and_survives_damage() {
        let ids: Vec<String> = vec!["a".into(), "b".into()];
        let mut st = UpdateState::default();
        st.finish(
            run(1_000, &ids, &ids, vec![n("a", "x", 1, 1_000), n("b", "y", 1, 1_000)]),
            "2 models checked",
        );
        st.dismiss("b#betterQuant", Some(3), 2_000).unwrap();
        let text = st.to_json();
        let (back, damaged) = UpdateState::load(Some(&text), None);
        assert!(!damaged);
        assert_eq!(back, st);
        // Damaged primary: the backup, which is older but intact.
        let (from_backup, damaged) = UpdateState::load(Some("{{{"), Some(&text));
        assert!(!damaged && from_backup == st);
        // Both damaged, or from a newer Pitwall: empty, and the caller is told.
        let newer = text.replace("\"version\":1", "\"version\":9");
        for (p, b) in [
            (Some("{{{"), Some("")),
            (Some(newer.as_str()), None),
            (None, Some("[]")),
            (Some("{\"version\":0}"), None),
        ] {
            let (s, damaged) = UpdateState::load(p, b);
            assert!(damaged && s == UpdateState::default(), "{p:?}");
        }
        // Nothing there yet: a fresh start, not damage.
        assert_eq!(UpdateState::load(None, None), (UpdateState::default(), false));
    }

    #[test]
    fn a_hostile_file_is_bounded() {
        let mut notices: Vec<UpdateNotice> = (0..MAX_NOTICES + 20).map(|i| n(&format!("m{i}"), "x", 1, 0)).collect();
        let mut dup = n("m0", "x", 1, 0);
        dup.summary = "dup".into();
        notices.push(dup);
        notices.push(UpdateNotice {
            id: String::new(),
            ..n("e", "x", 1, 0)
        });
        let dismissals = (0..MAX_DISMISSALS + 7)
            .map(|i| Dismissal {
                id: format!("d{i}"),
                fingerprint: String::new(),
                until: None,
                at: 0,
            })
            .collect();
        let f = File {
            version: 1,
            notices,
            dismissals,
            last_message: Some("m".repeat(1000)),
            ..File::default()
        };
        let (s, _) = UpdateState::load(Some(&serde_json::to_string(&f).unwrap()), None);
        assert_eq!(s.all().len(), MAX_NOTICES);
        assert!(s.all().iter().all(|x| !x.id.is_empty() && x.summary == "s"));
        assert_eq!(
            (s.dismissals.len(), s.dismissals[0].id.as_str()),
            (MAX_DISMISSALS, "d7"),
            "the newest are kept"
        );
        assert_eq!(s.last_message.as_deref().map(str::len), Some(MAX_MESSAGE));
        let mut st = s;
        st.attempted(5, Some(&"z".repeat(1000)));
        assert_eq!(st.last_message.as_deref().map(str::len), Some(MAX_MESSAGE));
    }
}
