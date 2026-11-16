use paddock_testkit::{Scratch, env_lock};
use std::ffi::OsString;

const A: &str = "PADDOCK_TESTKIT_A";
const B: &str = "PADDOCK_TESTKIT_B";

#[test]
fn changes_are_restored_on_drop_including_removal_and_repeated_sets() {
    {
        let mut env = env_lock();
        env.set(B, "original");
    }
    let mut env = env_lock();
    env.remove(A);
    assert_eq!(env.get(B), None, "the earlier test's change was undone");
    env.set(A, "1");
    env.set(A, "2");
    env.remove(B);
    assert_eq!(env.get(A), Some(OsString::from("2")));
    assert_eq!(
        env.changed().get(std::ffi::OsStr::new(A)),
        Some(&None),
        "the first value is the one kept"
    );
    drop(env);
    let env = env_lock();
    assert_eq!(env.get(A), None);
    assert_eq!(env.get(B), None);
}

#[test]
fn a_lock_poisoned_by_a_failed_test_is_recovered_and_the_variable_restored() {
    let r = std::thread::spawn(|| {
        let mut env = env_lock();
        env.set(A, "set by a test that then fails");
        panic!("failing test");
    })
    .join();
    assert!(r.is_err());
    let env = env_lock();
    assert_eq!(env.get(A), None);
}

#[test]
fn hermetic_points_the_user_folders_and_tmp_into_the_scratch() {
    let scratch = Scratch::new();
    let before = std::env::var_os("HOME");
    {
        let mut env = env_lock();
        env.hermetic(&scratch);
        assert_eq!(env.get("HOME"), Some(scratch.home().into_os_string()));
        assert_eq!(env.get("TMPDIR"), Some(scratch.tmp().into_os_string()));
        for key in [
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_CACHE_HOME",
            "XDG_STATE_HOME",
            "XDG_RUNTIME_DIR",
            "TEMP",
            "TMP",
        ] {
            let value = env
                .get(key)
                .map(std::path::PathBuf::from)
                .unwrap_or_default();
            assert!(
                value.starts_with(scratch.work().parent().unwrap_or(&value)),
                "{key} = {}",
                value.display()
            );
        }
    }
    assert_eq!(std::env::var_os("HOME"), before);
    scratch.finish().expect("nothing was left in tmp");
}

#[test]
fn two_tests_never_hold_the_lock_at_once() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static INSIDE: AtomicUsize = AtomicUsize::new(0);
    let workers: Vec<_> = (0..4)
        .map(|_| {
            std::thread::spawn(|| {
                let _env = env_lock();
                assert_eq!(
                    INSIDE.fetch_add(1, Ordering::SeqCst),
                    0,
                    "another holder is inside"
                );
                INSIDE.fetch_sub(1, Ordering::SeqCst);
            })
        })
        .collect();
    for w in workers {
        assert!(w.join().is_ok());
    }
}
