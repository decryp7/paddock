//! Budgets of ADR 0044 §7: a preview under 50 microseconds, a `LayoutView` with its three arrangements under 200
//! microseconds, for 32 placements. Run `cargo bench -p paddock-widgets`; results go to `docs/benchmarks.md` (rule 48).

use std::hint::black_box;
use std::time::Instant;

use paddock_widgets::{
    Dir, GridGesture, Item, PlacementId, Rect, Size, SizeRule, Steps, arrange, derive, moves,
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

fn time<T>(name: &str, runs: u32, mut f: impl FnMut() -> T) {
    let start = Instant::now();
    for _ in 0..runs {
        black_box(f());
    }
    println!(
        "{name}: {:.1} us per run ({runs} runs)",
        start.elapsed().as_secs_f64() * 1e6 / f64::from(runs)
    );
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
    time(
        "view: three arrangements and the moves of 32 placements",
        200,
        || {
            let _ = derive(&items, 12, 6);
            let _ = derive(&items, 12, 1);
            items
                .iter()
                .filter(|i| moves(&items, 12, &i.id).first)
                .count()
        },
    );
}
