//! The integration tests of this crate, one binary (linking one test binary per file cost more than running them).
//! Each module is a former `tests/<name>.rs`; run one with `cargo test --test it <name>::`.

mod common;
#[path = "../../../debrief-plugin-windows/tests/it/support/synth_minidump.rs"]
mod synth_minidump;

// Measurements by hand that read the process's CPU time with getrusage (Unix only).
#[cfg(unix)]
mod capture_open;
mod dumps;
mod engine;
mod evtx_corpus;
#[cfg(unix)]
mod evtx_idle;
mod evtx_memory;
mod formats_samples;
mod native_profiling;
mod otel_samples;
mod perf_plugins;
mod real_libraries;
mod registry_samples;
mod syslog_samples;
mod web_samples;
mod windows_annotations;
mod windows_markers;
mod windows_perfmon;
mod windows_samples;
mod windows_template;
