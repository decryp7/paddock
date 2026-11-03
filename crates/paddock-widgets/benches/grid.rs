//! Budgets of ADR 0044 §7: a preview under 50 microseconds, a `LayoutView` with its three arrangements under 200
//! microseconds, for 32 placements. Run `cargo bench -p paddock-widgets`; results go to `docs/benchmarks.md` (rule 48).

use std::hint::black_box;
use std::time::Instant;

use paddock_widgets::{
    Dir, GridGesture, Item, PlacementId, Rect, Size, SizeRule, Steps, arrange, derive, moves_all,
};

const RULE: SizeRule = SizeRule {
    widths: Steps::Range { min: 2, max: 12 },
    heights: Steps::Range { min: 2, max: 14 },
    default: Size { w: 3, h: 3 },
};

fn board() -> Vec<Item> {
    (0..32u16)
        .map(|n| Item {
            id: PlacementId::try_from(format!("w{n}")).expect("a valid id"),
            rect: Rect::new(u8::try_from(n % 4 * 3).unwrap_or(0), n / 4 * 4, 3, 4),
            rule: RULE,
        })
        .collect()
}

/// The fastest of 15 batches (the machine is shared and loaded; the minimum is the least disturbed one).
fn time<T>(name: &str, runs: u32, mut f: impl FnMut() -> T) {
    let batch = (runs / 15).max(1);
    let best = (0..15)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..batch {
                black_box(f());
            }
            start.elapsed().as_secs_f64() * 1e6 / f64::from(batch)
        })
        .fold(f64::MAX, f64::min);
    println!("{name}: {best:.1} us per run (best of 15 batches of {batch})");
}

fn main() {
    let items = board();
    let id = |n: u16| PlacementId::try_from(format!("w{n}")).expect("a valid id");
    time("preview (drop on the middle of the board)", 2000, || {
        arrange(
            &items,
            12,
            &GridGesture::Place {
                id: id(5),
                rect: Rect::new(3, 12, 3, 4),
            },
        )
    });
    time("nudge down", 2000, || {
        arrange(
            &items,
            12,
            &GridGesture::Nudge {
                id: id(5),
                dir: Dir::Down,
            },
        )
    });
    time("derive 6 and 1 columns", 2000, || {
        (derive(&items, 12, 6), derive(&items, 12, 1))
    });
    time("moves of all 32 placements", 2000, || moves_all(&items, 12));
    time(
        "view: three arrangements and the moves of 32 placements",
        2000,
        || {
            let _ = derive(&items, 12, 6);
            let _ = derive(&items, 12, 1);
            moves_all(&items, 12)
        },
    );
}
