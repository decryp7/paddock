//! The load gate: wait while the 1-minute load average is at or above the threshold. Unix reads the load average.
//! Windows has no load average (sysinfo reports zeros); a CPU-queue counter needs PDH and is not worth the weight here,
//! so the gate is skipped there with a note (the lock still serialises heavy steps).

use std::sync::Mutex;

/// The 1-minute load, or None when this platform has none.
pub trait LoadSource: Send + Sync {
    /// The 1-, 5- and 15-minute load averages.
    fn load(&self) -> Option<[f64; 3]>;
}

/// The operating system's load average.
#[derive(Debug, Default, Clone, Copy)]
pub struct SysLoad;

impl LoadSource for SysLoad {
    fn load(&self) -> Option<[f64; 3]> {
        if cfg!(windows) {
            return None;
        }
        let l = sysinfo::System::load_average();
        Some([l.one, l.five, l.fifteen])
    }
}

/// A load made by hand, for tests.
#[derive(Debug)]
pub struct FakeLoad(Mutex<Option<[f64; 3]>>);

impl FakeLoad {
    /// A load of `one` (the other averages equal it).
    pub fn new(one: f64) -> FakeLoad {
        FakeLoad(Mutex::new(Some([one, one, one])))
    }
    /// Changes the load.
    pub fn set(&self, one: f64) {
        if let Ok(mut m) = self.0.lock() {
            *m = Some([one, one, one]);
        }
    }
    /// A platform without a load average.
    pub fn none() -> FakeLoad {
        FakeLoad(Mutex::new(None))
    }
}

impl LoadSource for FakeLoad {
    fn load(&self) -> Option<[f64; 3]> {
        self.0.lock().ok().and_then(|m| *m)
    }
}

/// `1.23 4.56 7.89`.
pub fn load_text(l: [f64; 3]) -> String {
    format!("{:.2} {:.2} {:.2}", l[0], l[1], l[2])
}
