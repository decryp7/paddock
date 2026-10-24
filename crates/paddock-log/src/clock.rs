//! The log's time: the family's clock port (`paddock_jobs::time`), read as UTC for stamps, rotation and file names.

use chrono::{DateTime, Utc};
pub use paddock_jobs::time::{Clock, FakeClock, SystemClock};

/// The clock's wall reading as UTC (time stamps, daily file names, the crash marker).
pub fn now_utc(clock: &dyn Clock) -> DateTime<Utc> {
    DateTime::<Utc>::from(clock.wall())
}
