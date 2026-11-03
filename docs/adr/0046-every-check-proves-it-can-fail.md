# 46. Every check proves it can fail: a negative fixture per gate, check script, conformance check and contract test

- Status: **Draft (WIP, paused 2026-11-03 for Dyno 1.1.0)**; to become Proposed. Approved for drafting by the user
  (2026-11-03, item 8). Proposes rule **65** (numbers: see ADR 0045's note).
- Related: ADR 0039 (accepted; §4 "gates fail closed and are proved against the historical defects": broken-artifact
  tests for the release verifier only; this ADR generalises the same demand to every other check and does not repeat it),
  rule 5 (a bug-fix review runs a mutation check; the CI `cargo mutants --in-diff` job is an open follow-up, lessons
  "A bug-fix review runs a mutation check (2026-10-30)"), C62 (pipefail), ER-11, ER-21 / rule 40.

## Evidence (read 2026-11-03)

| Instance | Source |
|---|---|
| Dyno's Windows PE-subsystem and logon-entry gate scripts exited 0 without checking: `import.meta.url === \`file://${argv[1]}\`` never matches a Windows path | fixed by `pathToFileURL` in `7690a1f5a` on `fix/dyno-windows-shell` (`apps/dyno/scripts/check-pe-subsystem.mjs`, `check-logon-entry.mjs`) |
| `service-smoke.yml` compared the multi-line `--self-test` output (`awk '{print $2}'` over every line) to the version and could never pass | `8964af468` on `fix/service-smoke-self-test` (message lists the causes ruled out) |
| A test that asserts nothing: `attaching_never_panics_and_is_false_without_a_console` is `let _ = attach_parent_console();` | `crates/paddock-proc/src/console.rs` at `98703a274` (lines 73-77) |
| A test that greps its own source: `a_release_build_is_a_gui_subsystem_program` asserts `include_str!("main.rs")` contains `windows_subsystem` | `apps/dyno/crates/dyno-agent/src/main.rs` at `98703a274` |
| An always-true assert: `(fit.size.height + decor.height) * scale >= 163.0 * scale + frame_px - 1e-6` after `fit.size.height == 163` and `decor.height == frame_px / scale` is an identity | `crates/paddock-dialogs/src/window.rs` at `f90e50d24`, changed in `08ad47b22` |

## Pattern and invariant (draft)

**Pattern:** a check passes because it cannot fail (it never runs its body on one OS, compares the wrong thing, asserts
nothing, asserts a tautology or reads its own source). **Invariant:** every check has been seen to fail, in CI, on an
input made to break it.

## Decision (outline, to be written)

1. Each gate, CI check script, conformance check and contract test declares a negative fixture (broken artifact, wrong
   value, missing file) next to it; a `negative` test runs the check on it and expects failure and its `::error::` line.
2. A script that gates an OS is launched as a child process, the way CI starts it, on each OS it gates (catches the
   `import.meta.url` shape).
3. A conformance check (provisional **C75**) enumerates checks (conformance `C*` modules, `scripts/**/check-*`,
   `apps/*/scripts/check-*`, workflow `run:` steps that call them) and fails on one without a negative fixture.
4. Vacuous-test lint: Rust tests with no assertion or `#[should_panic]`, `include_str!` of the test's own file,
   constant-folded asserts; TS specs without `expect`. Plus the deferred `cargo mutants --in-diff` CI job (rule 5's
   follow-up) on shared crates, bounded.

## Still to do

Instance sweep and count (all gates and check scripts; vacuous tests), work packages with agent-days, G.7 placement,
alternatives (mutation-only, review-only), open decisions (lint vs mutation pass; scope of C75).
