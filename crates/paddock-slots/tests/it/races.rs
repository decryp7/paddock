use paddock_slots::{FsLocks, SlotId, acquire_first};
use std::sync::Barrier;
use std::sync::atomic::{AtomicU32, Ordering};

const SLOTS: u32 = 3;
const THREADS: usize = 12;
const ROUNDS: usize = 40;

#[test]
fn many_racing_acquirers_never_hold_one_slot_twice() {
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
