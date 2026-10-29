# 30. Every unexplained browser abort names the causes it ruled out, in one driver

- Status: Proposed (2026-10-29). Decisions 1 to 3 after review; sub-decision 4 (reload once) Proposed until a Windows CI
  failure shows "network service restarted"
- Date: 2026-10-29
- Related: rule 40 / ER-21 (recurring bugs mean a missing design rule), ER-12 (no retries for flaky tests), ER-14 (tests
  touch only what they started), rules 42/43 (share first); `docs/plans/shared-next.md` X14 (`paddock-chromium` replaces
  Spotter's `spotter-render` driver and Debrief's `chromium-driver`); `crates/paddock-chromium/src/page.rs`
- Evidence: Gitea runs of 2026-10-27 to 2026-10-29 (logs read through the API, kept out of the repository):
  `paddock-chromium` `real_browser::*` failed with `net::ERR_ABORTED` in 14 jobs (Windows 10, macOS 4), and still in 4 of
  8 Windows crate jobs after the first-navigation fix (jobs 18449, 19025, 19043, 19880); `spotter-render`
  `real_browser::*` failed with the same code in 12 Spotter Windows jobs. Measured on macOS with the pinned Chrome for
  Testing 156.0.8078.47 (2026-10-29): ending the network service process while a load is in flight gives exactly the
  trace of the Windows failures.

## Context

Chrome answers `Page.navigate` with `net::ERR_ABORTED` for every load it cancels before the page commits. Causes named so
far, each found after a round of red CI: a download, a load replaced by another navigation, an answer with no content
(204), and (fixed on 2026-10-28) a load sent while the tab's own first navigation still ran. Each was fixed by hand in
two copies of the driver (`paddock-chromium` and Spotter's `spotter-render`; Debrief has a third). That is the
recurring pattern: **an abort the driver cannot explain, guessed at and fixed per copy.**

The Windows failures that remain match none of the named causes. Their trace (from the driver's own failure message,
run 19880):

```
[Page.navigate sent]; frameStartedNavigating url=http://127.0.0.1:P/; frameStartedLoading;
requestWillBeSent url=http://127.0.0.1:P/; loadingFailed type=Document error=net::ERR_ABORTED canceled=true;
frameStoppedLoading
```

and the local server saw no connection at all (run 19043: "the server saw []"). The request was cancelled inside the
browser, before it reached the network.

One candidate: Chrome carries every request through its network service, a child process. When that process ends, the
navigation's URL loader loses its connection and Chromium cancels the load with `net::ERR_ABORTED` (`ThrottlingURLLoader`,
read 2026-10-29: <https://chromium.googlesource.com/chromium/src/+/a753229/content/common/throttling_url_loader.h>), and
Chrome starts the service again on the next request. Ending the network service of the pinned Chrome for Testing while a
load was held at a local server gave, every time, the Windows signature: `errorText: net::ERR_ABORTED`, `isDownload:
false`, `loadingFailed ... canceled=true`, `frameStoppedLoading`, no other navigation.

**Not verified:** that the network service restarts on the Windows runner. No Windows machine is available to agents.
Supporting, not proving: the failures come within the first seconds of a fresh browser, in `paddock-chromium`, whose
four real-browser tests start four browsers at once (4 of 8 Windows jobs failed), and rarely in `spotter-render`, whose
real-browser tests take turns (1 of 13 Windows jobs since the first-navigation fix).

## Decision

1. **Every unexplained abort names what it ruled out.** An abort is unexplained when it is `net::ERR_ABORTED`, not a
   download and not replaced by another navigation. Its error carries what the browser reported about the tab (the
   trace, as today) and what happened to the network service: "restarted (process A then B)", "unchanged (process A)",
   or "unknown (the browser did not say)". The service counts as restarted only when it had a process before, a lookup
   after the abort answered with another process or none, and the document request got no `Network.responseReceived`
   (a 204 or a download was answered, so the service did its part). A failed lookup is unknown, never a restart.
2. **The lookups are fresh and bounded.** `SystemInfo.getProcessInfo` is an **experimental** DevTools domain (it may
   change or go between Chrome versions; a browser that does not answer it gives "unknown") and costs about 24 ms on a
   loaded Mac (measured in review). The page looks the service up right before each load and again after an
   unexplained abort, each bounded by what is left of the load's time budget. Nothing is cached: a cached process would
   blame a restart while the page sat idle on a later abort, which is exactly the false evidence sub-decision 4 must
   never see.
3. **Each named cause has a real-browser test that drives it on purpose,** with no timing: the network service is
   ended while the first request is held at a local server (`a_load_the_network_service_lost_names_the_restart`; before
   the kill the test checks that the process is a descendant of the browser it launched, ER-14), and a 204 never blames
   the service (`an_answered_abort_never_blames_the_network_service`). Pure parts have unit tests.
4. **Sub-decision, Proposed: reload once after a loss.** When the service restarted under a load, `Page::navigate` loads
   the page once more within the same budget. This is product behaviour on a detected browser-internal cause, not a
   test retry. It is held back until a Windows CI failure shows "network service restarted": then it is the measured
   cause and the reload lands. A reload sends the same GET twice; the driver only ever reads pages, but a server that
   counts requests sees two.
5. **One driver.** Mechanisms like these live in `paddock-chromium` only. Spotter and Debrief get them by moving onto
   `paddock-chromium` (X14), not by another hand copy.

## Alternatives

- **Diagnostics first (chosen):** decisions 1 to 3 land now; the next Windows failure says whether the service restarted.
- **Reload right away:** would make the Windows leg green if the guess is right, and hide the cause if it is wrong.
  Kept as sub-decision 4.
- **Serialise the real-browser tests** with a nextest test group (`max-threads = 1`), as Spotter does: the data (4 of 8
  failures with four browsers at once, 1 of 13 taking turns) says concurrency matters. It treats the test, not the
  product, and runs the four tests one after another; a fallback if the cause turns out to be load.
- **Chrome's own log on Windows CI** (`--enable-logging --v=1` to a file kept as a job artifact): says why the service
  died. Done alongside decision 1 for the Windows crate job's real-browser tests. Verbose logging writes for every
  browser event and so changes the timing on Windows: a race may fail less (or differently) with the log on. The log
  explains a failure it sees; a green run with it on does not prove the race gone.

## Consequences

- A Windows failure names its cause or rules the network service out; this ADR's evidence section is then updated.
- One extra browser-level call per load (about 24 ms on a loaded Mac), and one more after an unexplained abort.
- Browsers that do not answer `SystemInfo.getProcessInfo` keep today's behaviour, with "unknown" in the message.
