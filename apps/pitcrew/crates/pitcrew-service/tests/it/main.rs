//! The service's integration tests, one binary (`supervision`: the supervisor end to end with a fake runner; `transport`: the
//! named pipe or Unix socket end to end).
#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod supervision;
mod transport;
