use std::num::NonZeroUsize;

/// A bounded worker count: 1 to [`Workers::MAX`]. The app's CPU limit is passed here, never read from globals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Workers(NonZeroUsize);

impl Workers {
    pub const MAX: usize = 64;

    /// Clamps `n` into 1..=MAX.
    pub fn new(n: usize) -> Self {
        let n = n.clamp(1, Self::MAX);
        Workers(NonZeroUsize::new(n).unwrap_or(NonZeroUsize::MIN))
    }

    /// One worker.
    pub fn one() -> Self {
        Workers(NonZeroUsize::MIN)
    }

    /// Cores minus one (keeps the UI responsive), at least 1, at most 16.
    pub fn auto() -> Self {
        let cores = std::thread::available_parallelism().map_or(2, NonZeroUsize::get);
        Self::new(cores.saturating_sub(1).clamp(1, 16))
    }

    pub fn get(self) -> usize {
        self.0.get()
    }
}

impl Default for Workers {
    fn default() -> Self {
        Self::auto()
    }
}
