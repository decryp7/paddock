# Amendment to ADR 0080 (2026-11-22): OS-scoped test placement everywhere, a full nightly on every OS, and WP-7 is back

- Status: Accepted (user, 2026-11-22), recorded by the ADR audit of 2026-11-22.
- Amends: D11 (a), §4.2 and the trim of 2026-11-11 (WP-7); ER-9 gets a matching one-sentence amendment (proposed in the audit report).

## 1. D11 (a) and §4.2

D11 (a) kept the landing run and the member runs full on every OS, and put "Linux first" only in branch runs and fix rounds. The user
reverses that: **a test that does not depend on the OS runs on one OS (Linux) in every kind of run, and an OS-specific test runs on the
OS it is specific to.** This holds for branch runs, fix rounds and train landings alike. §4.2's rules (the declared OS set per test
binary, checked by the conformance check, WP-11) are the mechanism; nothing wider is built beside them.

The net that replaces "full on every OS at landing" is **a full nightly run on every OS** (macOS, Windows, Linux). Scheduled CI is paused
until after Dyno 1.2.0 (memory 2026-11-20); until it is back, the release gates and the tag build are the only full runs on all three.

## 2. WP-7: package-level selection

The trim of 2026-11-11 dropped WP-7. The nextest package filter (D4, train 19) brings it back: a job runs only the packages the plan
reaches. The user's approval of D4 reverses the trim for this part only; the rest of WP-7 stays dropped.
