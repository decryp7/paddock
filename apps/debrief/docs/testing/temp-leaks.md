# Temp folders left by tests

Tests must not leave `debrief-*` or `imt-*` folders in the OS temp directory. CI fails the `build` workflow's
`rust tests` and `frontend` jobs when a run leaves one.

- **Check**: `scripts/ci/check-temp-leaks.mjs`. `--snapshot <file>` records the matching folders that exist before the
  tests; `--check <file>` exits 1 naming the ones that appeared since. Without a baseline every matching folder counts.
  `--dir <path>` scans another folder. Its test is `scripts/ci/check-temp-leaks.test.mjs`.
- **Shared folders**: when the temp folder is too long for a socket path (96 bytes; macOS' is), an endpoint folder
  falls back to `$XDG_RUNTIME_DIR` or `/tmp`. Without `--dir` the check also scans those two for names of exactly the
  endpoint shape (`debrief-<prefix>-<12 hex>`, or a confined test folder `dbw` plus six characters), never other files.
  A test that kills a host on purpose (the store worker's kill tests) confines it: `WorkerConfig::env` sets `TMPDIR` and
  `XDG_RUNTIME_DIR` to a short folder under `/tmp` that the test owns (`confined()` in `debrief-store-worker/tests/it/worker.rs`).
- **Children that end by `exit`**: a child whose lifeline (stdin) closes ends with `process::exit`, which runs no
  destructors. A host that is stopped while it starts a child (the store worker starting a plugin) would leave the
  endpoint folder `debrief-plugin-<12 hex>` behind, so `debrief_ipc::endpoint::remove_live()` removes every live
  endpoint folder right before that exit (`lifeline()` does). Anything else that ends a process without unwinding
  must call it too.
- **Test processes stopped by a timeout**: nextest stops a test past its time budget with SIGTERM (SIGKILL after a grace
  period). `debrief-app`'s test helpers (`test_support::TempDir`, `scratch_dir`, `process_scratch_dir`) record their
  folders, and the first one arms a SIGTERM/SIGINT handler (`ctrlc`) that removes the live endpoint folders and exits; the
  `atexit` handler then removes the recorded folders. So a timed-out test fails as a timeout only, not also as a leak
  (CI 7096 left `debrief-app-test-ai-*` and `debrief-test-stores-*`). Only SIGKILL or a crash still leaves them.
- **Rust tests**: make scratch folders with `tempfile::TempDir` (or `tempfile::Builder::new().prefix("debrief-...-")`) and
  keep the guard alive for the test, so the folder is removed on panic too. Unit tests in `debrief-core` use
  `crate::test_dirs::scratch_dir(prefix)`, which keeps the guard in a thread-local for the test's thread.
  Never build a path from `std::env::temp_dir()` and `remove_dir_all` it at the end of the test: a failing assert skips the cleanup.
- **A removal that fails**: a fixture's `Drop` must not ignore a failed removal. `debrief-core`'s integration tests use
  `scratch::remove_or_defer`: it says so on stderr and removes the folder again when the test process exits (CI 7108
  left `debrief-prop-*` folders whose removal had failed silently).
- **debrief-store-duck's fixtures**: `debrief_store_duck::test_scratch::remove_or_defer` (the `test-support` feature, and
  `#[cfg(test)]` for the unit tests) removes a scratch folder. A failure is written straight to stderr as `::warning::`
  (libtest captures `eprintln!`), leaves a marker `debrief-leak-deferred-<folder>` that `check-temp-leaks.mjs` fails on and
  prints (folder, error, what was left), and the folder is removed again at exit. There is no retry: CI 7813 left
  `debrief-duck-diff-*` and `debrief-maintain-reorder-skew-*` with all tests passing and the cause is not known (suspected,
  not reproduced); the marker will say the error and what was in the folder the next time.
- **The temp folder is shared**: on the Mac runner, CI jobs and agents' local runs use the same user temp folder, so a
  local run killed during a CI job leaves folders the job's check then reports.
- **Measure locally**: run the tests with `TMPDIR` pointing at an empty folder and list it afterwards:
  `mkdir /tmp/t && TMPDIR=/tmp/t cargo test -p debrief-core; ls /tmp/t`.

The `std::env::temp_dir` and `reqwest::Client::new` paths are in `clippy.toml` (C13): a test builds its folder from `tempfile` or the helpers above, and a
test that must look at the shared temp folder carries `#[expect(clippy::disallowed_methods, reason = ...)]`.
