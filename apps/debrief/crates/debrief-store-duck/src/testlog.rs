//! The log-record capture of the tests lives in `debrief_core::testlog` (shared by the store, the worker and the engine's own
//! tests); this re-export keeps the paths the store's and the worker's tests use.

pub use debrief_core::testlog::*;
