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
- **Rust tests**: make scratch folders with `tempfile::TempDir` (or `tempfile::Builder::new().prefix("debrief-...-")`) and
  keep the guard alive for the test, so the folder is removed on panic too. Unit tests in `debrief-core` use
  `crate::test_dirs::scratch_dir(prefix)`, which keeps the guard in a thread-local for the test's thread.
  Never build a path from `std::env::temp_dir()` and `remove_dir_all` it at the end of the test: a failing assert skips the cleanup.
- **Measure locally**: run the tests with `TMPDIR` pointing at an empty folder and list it afterwards:
  `mkdir /tmp/t && TMPDIR=/tmp/t cargo test -p debrief-core; ls /tmp/t`.

## TODO: lint `std::env::temp_dir`

Audit D4 asks for a clippy `disallowed-methods` entry for `std::env::temp_dir` (tests use `tempfile::TempDir`). It is not in
`clippy.toml` yet: about 159 uses exist, so enabling it workspace-wide fails the build today. Convert the uses crate by crate
(after the current merges), then add the entry; the CI leak check above is the safety net until then.
