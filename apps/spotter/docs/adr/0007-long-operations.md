# 0007. Long operations: phases, progress and Cancel

Status: accepted (2026-10-23)

## Context
Downloads, the browser install, folder scans and "check everything" ran with no progress channel and no Cancel. The
UI gave up after a fixed time (120 s for a download of up to 64 GiB) while the work went on, a resumed download read
its whole part before the first new byte without saying so, the browser archive was hashed twice, and a share was
walked by one thread.

## Decision
- **One small model, `spotter_core::job`.** A `Job` carries a cooperative `Cancel` flag and a throttled progress sink.
  Progress is a `Phase` (fetching the checksum, checking the part already downloaded, downloading, copying, unpacking,
  testing, finding files, waiting for files to settle, checking) with its own `Unit` (bytes or count) and `done`/`total`;
  `total: None` is an indeterminate phase with a growing count. The first report of a phase always goes out, the rest
  at most ten times a second. Operations take a `&Job` (`download_job`, `scan_job`, `install_pinned`,
  `check_now_job`, `test_source_job`); the old entry points pass `Job::none()`. The module is self-contained so it can be
  replaced by the shared `paddock-jobs` crate.
- **The shell names jobs.** `jobs::Jobs` registers a job under an id the UI chose (validated, bounded), forwards its
  reports as `spotter://job` events (`JobEvent`: running with a phase, or finished, cancelled, failed) and `cancel_job`
  raises the flag. The UI never times a job out: `download_release`, `install_renderer`, `check_now` and `test_source`
  have no UI time limit; the Operations panel shows the phase, its numbers and Cancel, and "still working" when the
  engine has been quiet for a while.
- **Hash once.** The download hashes while writing. A resumed download has to read its part once (SHA-256 cannot be
  continued from saved state); it does so while the request is already in flight and labels it with its own progress.
  The browser installer takes the digest computed while downloading and extracts without hashing again.
- **Deterministic parallel walk.** A folder is listed by up to eight threads sharing a queue of folders. Matches are
  sorted, folders keyed, counts are totals and the sample of ignored names is the first names in sorted order, so the
  answer does not depend on thread timing. The count of entries seen is reported live; the settle wait sleeps in slices
  and ends at Cancel; dropping the scan or giving up the wait stops the walk.
- **Cancelled is not a failure.** A cancelled check records nothing on the watch, a cancelled download is not put in the
  history (its `.part` stays so the next run resumes it), a cancelled install leaves nothing behind.

## Consequences
- Every new long operation takes a `Job`, names its phases and polls the flag; the UI shows it without code of its own.
- Swapping to `paddock-jobs` means replacing `spotter_core::job` and the event mapping in `app/src-tauri/src/jobs.rs`.
