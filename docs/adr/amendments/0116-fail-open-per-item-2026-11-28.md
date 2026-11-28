# Amendment to ADR 0116 (2026-11-28): a lookup fails open per item, not per call

- Status: **Accepted** (user, 2026-11-28, in chat: a lookup fails open per key with one retry). Amends ADR 0116 §5.
- Related: ER-27, ER-33, ER-44; PRINCIPLES "Bulk APIs" (errors are per item unless the operation is atomic); user decision 136 (RustFS stays on a
  slow disk, so timeouts are normal); `tools/paddock-dev/src/test_cache/{store,s3,mod}.rs`.

## What changed

§5 said an unreachable, refusing or slow store turns reuse off for the whole run. In run 9335 one request of 61 timed out
(`curl: (28)` after the 5 s budget), the whole Linux lookup failed, and 56 reusable packages ran and none was replaced (9329 had reused
them). The store is on a slow disk, so a timeout of one request is normal, and a call-wide failure is the wrong unit.

- `Store::get_many` returns one `Result<Option<Record>, StoreError>` per key. A key whose request failed is an error for that key only;
  the other keys keep their answers (`decide`: that target is `Why::Off(cause)` and runs, the others are reused or run by their own
  answer). Writes and deletes were atomic in this first text; the addendum below (user, 2026-11-28) makes them per key too.
- The S3 store keeps what curl said per transfer even when curl exits non-zero, and tries each key that got no answer (status 0) or a
  5xx once more, alone. One retry, never a loop (ER-12: no longer limit). A call that failed whole (curl did not start, the 60 s call
  budget ended, a redirect) is every key's error and is not retried.
- Every failed key is counted in the log under "store lookup failed" with the first cause (the plan's "reuse off N (cause)", the record
  step's "held back ...: N store lookup failed (cause)"), never a bare count.
- A record step prints its shadow line when at least one key was answered; with none answered it prints none (not measured, ER-33).
  A target whose lookup failed is never replaced (no definite answer).

The fail-open rule itself is unchanged: a target without an answer runs, never the other way round.

## Addendum, 2026-11-28 (user, 2026-11-28, in chat): writes and deletes are per key, 2 at a time, with backed-off retries

Run 9350 (crate, windows-latest): the selfcheck passed, then one record PUT timed out after 5015 ms and the one atomic write lost every record
of the leg ("records not stored: curl failed: curl: (28)"). Accepted by the user:

- `Store::put_many` and `Store::delete_many` return one result per key. A record that still fails is the only one lost, with its cause counted in
  the warning (`N of M records not stored: K x <cause>`).
- Writes and deletes run **2 in parallel** (`WRITE_PARALLEL`; lookups keep 16) with a request limit of 30 s (`WRITE_REQUEST_SECS`; reads keep 5 s).
- A write or delete that gets 503, another 5xx or no answer is retried **up to 3 times, after 1, 3 and 9 s** (`WRITE_RETRY_WAITS`), each time only for
  the requests that still failed. **Reads keep one retry with no wait.**

Cause (ER-12), measured on 2026-11-28 against the real RustFS on a spinning disk (user decision 136), 400 to 451-byte records, machine load 7 to 24
(`ci test-cache probe --shared`, and the same PUTs by hand): the server answers 503 to PUTs in flight together.

| PUTs in parallel | 503 answers | longest PUT |
|---|---|---|
| 16 | 30, 32 and 37 of 61; 32 of 50 | 15.6 to 18 s |
| 8 | 18 and 21 of 61 | 12.8 s |
| 4 | 2, 1 and 4 of 50 or 61 | 9.9 to 11.3 s |
| 2 | none of 61 (median 3.4 s) | 9.1 s |
| 1 | none | 3.3 s |

Half the PUTs at 16 in parallel took more than 5 s, which is run 9350's timeout. The numbers are for the spinning disk; the probe
(`.gitea/workflows/test-cache-probe.yml`, by hand on every OS) is run again after RustFS moves to NVMe, and the constants are revisited then.
