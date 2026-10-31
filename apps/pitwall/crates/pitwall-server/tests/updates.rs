//! Model update notices end to end (ADR 0017) over a fake Hugging Face: nothing is asked before the user opts in or
//! while offline mode is on, a changed file becomes a notice that can be dismissed, notices and dismissals survive a
//! restart and a damaged state file, "Check now" is rate limited, and a check never downloads anything.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;
use common::{harness_in, Harness};
use futures_util::future::BoxFuture;
use pitwall_core::hf::{RepoFiles, RepoId, Variant};
use pitwall_core::updates::{Provenance, RepoSnapshot};
use pitwall_server::egress::DownloadError;
use pitwall_server::paths::{now_ms, Dirs};
use pitwall_server::updates::RepoSource;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// A fake Hugging Face with one answer per repository; counts every request and remembers what was asked.
#[derive(Default)]
struct FakeHf {
    files: Mutex<Vec<Variant>>,
    asked: Mutex<Vec<String>>,
    calls: AtomicUsize,
    fail: Mutex<bool>,
}

impl RepoSource for FakeHf {
    fn snapshot<'a>(&'a self, repo: &'a RepoId) -> BoxFuture<'a, Result<RepoSnapshot, DownloadError>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.asked.lock().unwrap().push(repo.as_str().to_string());
            if *self.fail.lock().unwrap() {
                return Err(DownloadError::Network("fake outage".into()));
            }
            Ok(RepoSnapshot {
                revision: Some("f".repeat(40)),
                files: RepoFiles {
                    variants: self.files.lock().unwrap().clone(),
                    projectors: vec![],
                },
            })
        })
    }
}

fn variant(file: &str, size: u64, sha: char) -> Variant {
    Variant {
        file: file.into(),
        parts: vec![file.into()],
        quant: pitwall_core::quant::label_from_file_name(file),
        size,
        sha256: vec![sha.to_string().repeat(64)],
    }
}

/// A server whose only model is `o/r/tiny-Q4_K_M.gguf`, asking `fake`.
async fn setup() -> (Harness, Arc<FakeHf>) {
    let h = harness_in(tempfile::tempdir().unwrap(), false).await;
    let fake = Arc::new(FakeHf::default());
    h.st.set_update_source(fake.clone());
    (h, fake)
}

fn disk_size(h: &Harness) -> u64 {
    std::fs::metadata(h.dir.path().join("models/o/r/tiny-Q4_K_M.gguf")).unwrap().len()
}

async fn notices(h: &Harness) -> Vec<Value> {
    h.rpc("update_notices", json!({})).await.json().as_array().unwrap().clone()
}

fn enable(h: &Harness, enabled: bool) {
    let mut s = h.st.cfg();
    s.models.update_check.enabled = enabled;
    h.st.save_settings(s).unwrap();
}

fn go_offline(h: &Harness) {
    let mut s = h.st.cfg();
    s.network.offline = true;
    h.st.save_settings(s).unwrap();
}

#[tokio::test]
async fn nothing_is_asked_before_the_user_opts_in_and_the_status_says_so() {
    let (h, fake) = setup().await;
    let st = h.rpc("update_check_status", json!({})).await.json();
    assert_eq!(st["settings"], json!({"enabled": false, "schedule": "weekly"}));
    assert_eq!(
        (st["host"].as_str(), st["offline"].as_bool(), st["running"].as_bool()),
        (Some("huggingface.co"), Some(false), Some(false))
    );
    assert!(st["nextCheckAt"].is_null() && st["lastCheckedAt"].is_null());
    assert!(h.st.scheduled_update_tick().await.is_none(), "disabled: the schedule does nothing");
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0);
    assert!(notices(&h).await.is_empty());
}

#[tokio::test]
async fn offline_mode_makes_zero_requests_for_both_check_now_and_the_schedule() {
    let (h, fake) = setup().await;
    enable(&h, true);
    go_offline(&h);
    let r = h.rpc("check_updates_now", json!({})).await;
    assert_eq!(r.status, 200, "{}", r.body);
    let r = r.json();
    assert_eq!(r["outcome"], "offline");
    assert!(r["message"].as_str().unwrap().contains("Offline mode"));
    assert!(h.st.scheduled_update_tick().await.is_none());
    let st = h.rpc("update_check_status", json!({})).await.json();
    assert_eq!((st["offline"].as_bool(), st["nextCheckAt"].is_null()), (Some(true), true));
    assert_eq!(fake.calls.load(Ordering::SeqCst), 0, "no request while offline");
}

#[tokio::test]
async fn a_changed_file_becomes_a_notice_that_can_be_dismissed_and_stays_dismissed_after_a_restart() {
    let (h, fake) = setup().await;
    // An old entry (no provenance record): compared by size.
    *fake.files.lock().unwrap() = vec![variant("tiny-Q4_K_M.gguf", disk_size(&h) + 1, 'b')];
    let r = h.rpc("check_updates_now", json!({})).await.json();
    assert_eq!(
        (r["outcome"].as_str(), r["reposChecked"].as_u64(), r["notices"].as_u64()),
        (Some("checked"), Some(1), Some(1)),
        "{r}"
    );
    assert_eq!(
        fake.asked.lock().unwrap().clone(),
        vec!["o/r"],
        "only the repository id of a downloaded model is asked"
    );
    let n = notices(&h).await;
    assert_eq!(n.len(), 1);
    assert_eq!(
        (n[0]["kind"].as_str(), n[0]["repo"].as_str(), n[0]["file"].as_str()),
        (Some("newerRevision"), Some("o/r"), Some("tiny-Q4_K_M.gguf"))
    );
    assert!(n[0]["summary"].as_str().unwrap().contains("changed on Hugging Face"));
    // The checker raised an event but fetched nothing: the file on disk is untouched.
    let events = h.rpc("events", json!({"limit": 50})).await.json();
    assert!(events.as_array().unwrap().iter().any(|e| e["kind"] == "modelUpdate"));
    assert_eq!(h.st.library.downloads().len(), 0, "a check never starts a download");

    let id = n[0]["id"].as_str().unwrap().to_string();
    let after = h.rpc("dismiss_update", json!({"id": id})).await;
    assert_eq!(after.status, 200, "{}", after.body);
    assert!(after.json().as_array().unwrap().is_empty());
    assert_eq!(h.rpc("update_check_status", json!({})).await.json()["notices"], 0);

    // A restart keeps the dismissal and the check times; the same offer found again stays hidden.
    let path = h.dir.path().to_path_buf();
    let again = pitwall_server::app::AppState::open(Dirs::at(&path), None).await;
    again.set_update_source(fake.clone());
    assert!(again.update_notices().is_empty());
    assert!(again.update_status().last_checked_at.is_some());
    assert_eq!(again.updates.read(|s| s.all().len()), 1, "the notice itself is kept, only hidden");
}

#[tokio::test]
async fn the_recorded_hash_decides_when_there_is_one() {
    let (h, fake) = setup().await;
    let file = h.dir.path().join("models/o/r/tiny-Q4_K_M.gguf");
    let rec = Provenance::new(
        "o/r",
        "tiny-Q4_K_M.gguf",
        disk_size(&h),
        vec!["a".repeat(64)],
        Some("1".repeat(40)),
        now_ms(),
    );
    std::fs::write(pitwall_server::library::sidecar_path(&file), rec.to_json()).unwrap();
    *fake.files.lock().unwrap() = vec![variant("tiny-Q4_K_M.gguf", disk_size(&h), 'a')];
    let r = h.rpc("check_updates_now", json!({})).await.json();
    assert_eq!(r["notices"], 0, "{r}");
    // Same size, different hash: a newer revision, and the summary names both repository revisions.
    *fake.files.lock().unwrap() = vec![variant("tiny-Q4_K_M.gguf", disk_size(&h), 'b')];
    // The rate limit uses the real clock: start from a fresh state file so the second check is allowed.
    let path = h.dir.path().to_path_buf();
    std::fs::remove_file(path.join("model-updates.json")).unwrap();
    let h2 = pitwall_server::app::AppState::open(Dirs::at(&path), None).await;
    h2.set_update_source(fake.clone());
    let r = h2.run_update_check().await.result;
    assert_eq!(r.notices, 1);
    let n = h2.update_notices();
    assert!(n[0].summary.contains("revision 1111111, now fffffff"), "{}", n[0].summary);
}

#[tokio::test]
async fn check_now_is_rate_limited_and_a_failing_hugging_face_is_reported_not_raised() {
    let (h, fake) = setup().await;
    *fake.fail.lock().unwrap() = true;
    let r = h.rpc("check_updates_now", json!({})).await.json();
    assert_eq!(r["outcome"], "failed", "{r}");
    assert!(r["message"].as_str().unwrap().contains("could not be reached"));
    let calls = fake.calls.load(Ordering::SeqCst);
    let again = h.rpc("check_updates_now", json!({})).await.json();
    assert_eq!(again["outcome"], "rateLimited");
    assert!(again["retryInSecs"].as_u64().unwrap() <= 60);
    assert_eq!(fake.calls.load(Ordering::SeqCst), calls, "the refused click asked nothing");
    let st = h.rpc("update_check_status", json!({})).await.json();
    assert!(st["lastCheckedAt"].is_null() && st["lastAttemptAt"].is_number());
    assert!(st["lastMessage"].as_str().unwrap().contains("could not be reached"));
}

#[tokio::test]
async fn enabling_the_schedule_runs_the_first_check_then_waits_for_the_interval() {
    let (h, fake) = setup().await;
    *fake.files.lock().unwrap() = vec![variant("tiny-Q4_K_M.gguf", disk_size(&h) + 1, 'b')];
    let st = h.rpc("set_update_check", json!({"enabled": true, "schedule": "daily"})).await;
    assert_eq!(st.status, 200, "{}", st.body);
    assert_eq!(st.json()["settings"], json!({"enabled": true, "schedule": "daily"}));
    // The first check is started by enabling it.
    for _ in 0..100 {
        if fake.calls.load(Ordering::SeqCst) > 0 && h.st.update_status().last_checked_at.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    assert!(h.st.scheduled_update_tick().await.is_none(), "not due again for a day");
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    let s = h.st.update_status();
    assert!(s.next_check_at.unwrap() > now_ms() + 20 * 3_600_000, "{:?}", s.next_check_at);
    // The setting is saved with the other settings and survives a restart.
    let back = pitwall_server::app::AppState::open(Dirs::at(h.dir.path()), None).await;
    assert!(back.cfg().models.update_check.enabled);
}

#[tokio::test]
async fn bad_arguments_are_refused_with_a_message() {
    let (h, _) = setup().await;
    let r = h.rpc("set_update_check", json!({"enabled": true, "schedule": "hourly"})).await;
    assert_eq!(r.status, 400, "{}", r.body);
    let r = h.rpc("dismiss_update", json!({"id": "nope#betterQuant"})).await;
    assert_eq!(r.status, 404, "{}", r.body);
    assert!(r.body.contains("no update notice"));
    let r = h.rpc("dismiss_update", json!({"id": "x", "snoozeDays": 0})).await;
    assert_eq!(r.status, 400, "{}", r.body);
    assert!(r.body.contains("1 and 90"));
    assert_eq!(h.rpc("update_check_status", json!({})).await.json()["settings"]["enabled"], false);
}

#[tokio::test]
async fn a_damaged_state_file_starts_empty_with_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("model-updates.json"), b"{ not json").unwrap();
    let h = harness_in(dir, false).await;
    assert!(notices(&h).await.is_empty());
    let events = h.rpc("events", json!({"limit": 50})).await.json();
    assert!(events
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["message"].as_str().unwrap_or("").contains("notices file was damaged")));
    // It still works: a check rebuilds it.
    let fake = Arc::new(FakeHf::default());
    h.st.set_update_source(fake.clone());
    *fake.files.lock().unwrap() = vec![variant("tiny-Q4_K_M.gguf", disk_size(&h) + 5, 'c')];
    assert_eq!(h.rpc("check_updates_now", json!({})).await.json()["notices"], 1);
}

#[tokio::test]
async fn deleting_a_model_removes_its_record() {
    let (h, _) = setup().await;
    let file = h.dir.path().join("models/o/r/tiny-Q4_K_M.gguf");
    let side = pitwall_server::library::sidecar_path(&file);
    std::fs::write(
        &side,
        Provenance::new("o/r", "tiny-Q4_K_M.gguf", disk_size(&h), vec![], None, 1).to_json(),
    )
    .unwrap();
    let installed = h.st.library.installed();
    assert_eq!(
        (installed.len(), installed[0].repo.as_str(), installed[0].file.as_str()),
        (1, "o/r", "tiny-Q4_K_M.gguf")
    );
    let id = pitwall_core::ids::ModelId::parse("tiny-q4_k_m").unwrap();
    h.st.library.delete(&id).unwrap();
    assert!(!side.exists() && !file.exists());
}

/// A Hugging Face that never answers: a check against it runs until it is cancelled.
struct SilentHf(AtomicUsize);

impl RepoSource for SilentHf {
    fn snapshot<'a>(&'a self, _: &'a RepoId) -> BoxFuture<'a, Result<RepoSnapshot, DownloadError>> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            std::future::pending().await
        })
    }
}

#[tokio::test]
async fn a_check_can_be_cancelled_by_its_op_id_and_leaves_the_checker_free() {
    let h = Arc::new(harness_in(tempfile::tempdir().unwrap(), false).await);
    let silent = Arc::new(SilentHf(AtomicUsize::new(0)));
    h.st.set_update_source(silent.clone());
    let h2 = h.clone();
    let check = tokio::spawn(async move { h2.rpc("check_updates_now", json!({ "opId": "check-1" })).await });
    // The check is asking Hugging Face (and the status says it runs) before the cancel is sent.
    while silent.0.load(Ordering::SeqCst) == 0 {
        tokio::task::yield_now().await;
    }
    assert_eq!(h.rpc("update_check_status", json!({})).await.json()["running"], true);
    assert_eq!(h.rpc("cancel_op", json!({ "opId": "check-1" })).await.status, 200);
    let r = tokio::time::timeout(std::time::Duration::from_secs(60), check)
        .await
        .expect("the check ends")
        .unwrap();
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.json(), json!({ "cancelled": true }));
    assert_eq!(
        h.rpc("update_check_status", json!({})).await.json()["running"],
        false,
        "a cancelled check releases the guard"
    );
    assert!(h.st.ops.lock().unwrap().is_empty());
}
