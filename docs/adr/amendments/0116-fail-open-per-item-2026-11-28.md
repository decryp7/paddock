# Amendment to ADR 0116 (2026-11-28): a lookup fails open per item, not per call

- Status: **Proposed**: decided by the coordinator on 2026-11-28 under the user's standing "complete fix" direction; needs the user's yes. Amends ADR 0116 §5.
- Related: ER-27, ER-33, ER-44; PRINCIPLES "Bulk APIs" (errors are per item unless the operation is atomic); user decision 136 (RustFS stays on a
  slow disk, so timeouts are normal); `tools/paddock-dev/src/test_cache/{store,s3,mod}.rs`.

## What changed

§5 said an unreachable, refusing or slow store turns reuse off for the whole run. In run 9335 one request of 61 timed out
(`curl: (28)` after the 5 s budget), the whole Linux lookup failed, and 56 reusable packages ran and none was replaced (9329 had reused
them). The store is on a slow disk, so a timeout of one request is normal, and a call-wide failure is the wrong unit.

- `Store::get_many` returns one `Result<Option<Record>, StoreError>` per key. A key whose request failed is an error for that key only;
  the other keys keep their answers (`decide`: that target is `Why::Off(cause)` and runs, the others are reused or run by their own
  answer). Writes and deletes stay atomic: an error is for the call.
- The S3 store keeps what curl said per transfer even when curl exits non-zero, and tries each key that got no answer (status 0) or a
  5xx once more, alone. One retry, never a loop (ER-12: no longer limit). A call that failed whole (curl did not start, the 60 s call
  budget ended, a redirect) is every key's error and is not retried.
- Every failed key is counted in the log under "store lookup failed" with the first cause (the plan's "reuse off N (cause)", the record
  step's "held back ...: N store lookup failed (cause)"), never a bare count.
- A record step prints its shadow line when at least one key was answered; with none answered it prints none (not measured, ER-33).
  A target whose lookup failed is never replaced (no definite answer).

The fail-open rule itself is unchanged: a target without an answer runs, never the other way round.
