use paddock_slots::{FsLocks, SlotId, acquire_first};
use std::sync::Barrier;
use std::sync::atomic::{AtomicU32, Ordering};

const SLOTS: u32 = 3;
const THREADS: usize = 12;
const ROUNDS: usize = 40;

#[test]
fn many_racing_acquirers_never_hold_one_slot_twice() {
    let _gate = crate::share_gate();
    let dir = tempfile::tempdir().unwrap();
    let locks = FsLocks::new(dir.path());
    let inside: Vec<AtomicU32> = (0..SLOTS).map(|_| AtomicU32::new(0)).collect();
    let wins = AtomicU32::new(0);
    let start = Barrier::new(THREADS);
    std::thread::scope(|s| {
        for _ in 0..THREADS {
            s.spawn(|| {
                start.wait();
                for _ in 0..ROUNDS {
                    let Some((id, guard)) = acquire_first(&locks, SLOTS).unwrap() else {
                        continue;
                    };
                    let n = &inside[id.number() as usize - 1];
                    assert_eq!(n.fetch_add(1, Ordering::SeqCst), 0, "{id} held twice");
                    wins.fetch_add(1, Ordering::SeqCst);
                    std::thread::yield_now();
                    n.fetch_sub(1, Ordering::SeqCst);
                    drop(guard);
                }
            });
        }
    });
    assert!(
        wins.load(Ordering::SeqCst) >= SLOTS,
        "somebody must have won"
    );
    let ids: Vec<SlotId> = (1..=SLOTS).filter_map(SlotId::new).collect();
    assert_eq!(ids.len(), SLOTS as usize);
}

/// The CI kit deletes an idle slot's kept folders under [`with_idle`]: no acquirer may get that slot while the deletion runs,
/// and a slot a job holds is never swept.
#[test]
fn a_slot_being_swept_is_never_handed_out_and_a_held_slot_is_never_swept() {
    use paddock_slots::with_idle;
    use std::sync::atomic::AtomicBool;
    let dir = tempfile::tempdir().unwrap();
    let locks = FsLocks::new(dir.path());
    let s1 = SlotId::new(1).unwrap();
    let sweeping = AtomicBool::new(false);
    let start = Barrier::new(THREADS + 1);
    std::thread::scope(|s| {
        s.spawn(|| {
            start.wait();
            for _ in 0..ROUNDS * 4 {
                with_idle(&locks, s1, || {
                    sweeping.store(true, Ordering::SeqCst);
                    std::thread::yield_now();
                    sweeping.store(false, Ordering::SeqCst);
                })
                .unwrap();
            }
        });
        for _ in 0..THREADS {
            s.spawn(|| {
                start.wait();
                for _ in 0..ROUNDS {
                    if let Some((id, guard)) = acquire_first(&locks, 1).unwrap() {
                        assert!(
                            !sweeping.load(Ordering::SeqCst),
                            "{id} handed out while it was swept"
                        );
                        std::thread::yield_now();
                        drop(guard);
                    }
                }
            });
        }
    });
    let (_, held) = acquire_first(&locks, 1).unwrap().unwrap();
    let mut ran = false;
    assert!(with_idle(&locks, s1, || ran = true).unwrap().is_none());
    assert!(!ran, "a held slot is not swept");
    drop(held);
    assert!(with_idle(&locks, s1, || ()).unwrap().is_some());
}
