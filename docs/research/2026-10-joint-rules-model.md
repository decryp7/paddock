# The joint rules model (D-rules)

Status: decisions answered by the user 2026-10-30 (section 8); awaiting independent review. Author: the Pacenotes C1 package, for the conformance owner and the user to approve. Docs only; no
crate is written from it until it is approved. It is the note ADR 0022 section 6 (phase 4) and Pacenotes plan section 3.3 ask for
before `paddock-rules` and before `pacenotes-rules` (C1).

## 1. Why one model

Two things in this repository report "findings": the conformance checks (`conformance/src`, JavaScript, `dev check` in ADR 0022
phase 4) and Pacenotes' review (proposal section 24, `pacenotes-core` types, `pacenotes-rules` in C1). They overlap in nearly
every concept: a rule or check with an id, a severity, a place, a message, a one-sentence fix, a scoped suppression with a reason
and an expiry, a count of what was suppressed, and a report in several formats. Building two copies breaks ER-22 and rule 42.
This note fixes one model that both fit, maps every field of both to it, and names what stays outside it.

Sources read for this note (2026-10-30): `conformance/src/finding.mjs`, `report.mjs`, `runner.mjs`, `util/waivers.mjs`,
`paddock.waivers.toml`; `apps/pacenotes/crates/pacenotes-core/src/{rule,finding,tier,ids,location}.rs`; Pacenotes proposal section 24;
ADR 0022 sections 6 and 7. SARIF 2.1.0 is the OASIS standard (https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html).

## 2. The two models today

| Concept | Conformance (JavaScript) | Pacenotes (`pacenotes-core`, proposal 24) |
|---|---|---|
| Rule | a check id (`C37`, `W001`) and a `rule` number (a rule of `docs/rules.md`) | `RuleSpec`: `RuleId` (`[a-z0-9][a-z0-9._/-]{0,95}`), title, severity, `include`/`exclude` globs, `languages`, `check` (`Pattern` or `Prose`), optional `fix`, `good` and `bad` examples |
| Severity | `error` or `warn` | `Info`, `Low`, `Medium`, `High`, `Blocker` (ordered) |
| Place | `file` (optional) and `line` (default 1) | `Location` (path, byte and line span, revision) |
| Message and fix | `message`, `fix` (one sentence, required) | evidence excerpts, an explanation, a suggested fix as a diff (never applied) |
| State | none (a finding is there or not) | `FindingState`: Open, Confirmed, Dismissed, Fixed, Suppressed, with a transition table |
| Identity | none | `Fingerprint` (rule, path, normalised snippet hash), for baselines and renames |
| Suppression | `paddock.waivers.toml`: `check`, `path` glob, `reason`, `expires` (all four required); a bad one is itself a finding (`W002` incomplete, `W001` expired) | scoped suppressions (line, file, path, rule), required reason, optional expiry, in code comments or `.pacenotes/suppressions.toml` |
| Baseline | none | by fingerprint; CI fails only on new findings |
| Reports | text with `::error` annotations, Markdown job summary, `conformance.json` | in-app panel, Markdown, SARIF 2.1.0, rdjson import, exit codes 0/1/2/3 |
| Counts | errors, warnings, waived | by severity, by state |
| Fails closed | a check that throws is an error finding | a rule that does not compile refuses the pack with a message |

## 3. The joint model

One crate, `paddock-rules`, pure (no I/O, no clock: time is passed in), three layers. Names below are the proposal; the
crate's ADR fixes them.

### 3.1 Types

- `RuleId`: the Pacenotes validation (`[a-z0-9][a-z0-9._/-]{0,95}`); conformance ids are lower-cased on the way in (`c37`) and keep a
  display form (`C37`). A rule also carries an optional `policy_ref` (conformance's `rule` number, or any citation), shown in reports.
- `Severity`: the superset `Info < Low < Medium < High < Blocker`. Conformance maps `warn` to `Medium` and `error` to `High` (decided), and a check may declare any level; conformance's report prints the old words by a display table, so its output does not change
  (a golden test holds that). The gate threshold is a severity: conformance uses "High and above fails", Pacenotes uses a setting.
- `Place`: path (relative, `/`-separated, validated like `RepoPath`), optional span (line and column, byte offsets), optional revision.
  Conformance fills path and line only.
- `Finding`: rule id, severity, `Place`, `message`, `fix` (one sentence, **required** for every finding; decided 2026-10-30), optional `explanation`, optional `evidence` excerpts, optional `fix_diff`, optional
  `fingerprint`, optional `Confidence`, `State`.
- `State`: Open, Confirmed, Dismissed, Fixed, Suppressed (Pacenotes' transition table moves over unchanged; conformance only ever
  produces Open and Suppressed).
- `Suppression`: scope (`Line`, `File`, `Path(glob)`, `Rule`, or a combination of rule and path glob, which is conformance's waiver),
  `reason` (required, non-blank), `expires` (optional date; required in the `waiver` profile), `count` (how many findings it hid,
  filled by the engine). A suppression that is incomplete, expired, or that hides nothing is itself a finding of the engine
  (conformance's `W002`, `W001`, and a new "unused suppression" at `Info`).
- `Baseline`: a set of fingerprints with the rule id and a first-seen date; "new" means not in the baseline.
- `Report`: findings, suppressed findings (with their suppression), counts by severity and state, the checks or rules that ran, and the
  ones that were skipped with the reason (so a green report never means "everything ran").

### 3.2 Behaviour (pure functions)

1. `apply_suppressions(findings, suppressions, today) -> (kept, suppressed, engine_findings)`: conformance's `applyWaivers`, generalised.
2. `apply_baseline(findings, baseline) -> (new, known)`.
3. `fingerprint(rule, path, snippet)`: the Pacenotes definition (normalised snippet hash); conformance does not use it.
4. `exit_code(report, threshold, partial) -> 0 | 1 | 2 | 3`: 0 clean, 1 findings at or above the threshold, 2 error, 3 partial or
   budget exhausted. Conformance keeps "1 when any error" as the profile `threshold = High`.
5. Writers: text (with the `::error`/`::warning` annotation lines), Markdown job summary, JSON (the `conformance.json` shape is a
   profile of it), SARIF 2.1.0, and the SARIF and rdjson **readers** (Pacenotes imports clippy, ESLint and others; conformance does not
   need the readers).

### 3.3 What stays outside the model

- **How findings are found.** Conformance's checks (JavaScript today, Rust under `dev check`) and Pacenotes' structural patterns over
  tree-sitter (`pacenotes-rules`, C1) and prose rules (Pitwall) all produce `Finding`s; none of them lives in this crate. The pattern
  matcher stays in `pacenotes-rules` (it needs tree-sitter), on top of this crate.
- **The workflow around findings** in the UI (grouping, evidence popovers, learning from decisions) is Pacenotes' own.
- **Rule files.** Pacenotes' rule TOML (`RuleSpec`) and conformance's check registry are different sources; only the `Rule` identity
  and the results meet here.

## 4. SARIF 2.1.0 mapping

| Model | SARIF 2.1.0 |
|---|---|
| `Report` | one `run` per tool; `tool.driver.name` and `version`, `rules` from the rules that ran |
| rule | `reportingDescriptor`: `id`, `shortDescription` (title), `helpUri` (policy_ref), `defaultConfiguration.level` |
| severity | `level`: Info -> `note`, Low and Medium -> `warning`, High and Blocker -> `error`; the exact severity in `properties.severity` so a round trip is lossless |
| `Place` | `result.locations[0].physicalLocation`: `artifactLocation.uri` (relative, `uriBaseId` `%SRCROOT%`) and `region` |
| `message`, `fix` | `result.message.text`; the fix sentence in `properties.fix`; a `fix_diff` as `result.fixes[]` with `artifactChanges` |
| `fingerprint` | `result.partialFingerprints["paddock/v1"]` |
| suppression | `result.suppressions[]`: `kind` `inSource` or `external`, `justification` = reason, `properties.expires` |
| baseline | `result.baselineState`: `new`, `unchanged`, `absent` |
| state | `result.properties.state` (SARIF has no equivalent of Confirmed or Dismissed beyond suppressions) |

The reader maps the other way and **bounds** its input (file size, results count, string length, nesting) and never panics on any
input (rule 36); unknown fields are kept in `properties` only up to a cap and otherwise ignored. rdjson (reviewdog) is read the same
way: `diagnostics[]` with `message`, `location.path`, `location.range`, `severity`, `code`, `suggestions`.

## 5. Field-by-field map (the failing-test list of the D-rules package)

Every conformance field and every Pacenotes field, and where it lands. This is the table the future adoption test iterates over.

| Source field | Model field | Note |
|---|---|---|
| conformance `check` | `Finding.rule` (`RuleId`) | upper-case display form kept |
| conformance `rule` (number) | rule's `policy_ref` | shown as "rule 37" |
| conformance `severity` `error` / `warn` | `Severity::High` / `Severity::Medium` | by the profile's display table |
| conformance `file`, `line` | `Place.path`, `Place.span.line` | `file` may be absent: a repository-level finding has no `Place` |
| conformance `message`, `fix` | `Finding.message`, `Finding.fix` | `fix` required for every rule |
| conformance waiver `check` | `Suppression.scope = Rule` | |
| conformance waiver `path` (glob) | `Suppression.scope` path glob | same glob dialect as `pathMatches` today |
| conformance waiver `reason`, `expires` | `Suppression.reason`, `.expires` | all four required in the `waiver` profile |
| conformance `W001`, `W002` | engine findings of `apply_suppressions` | ids kept |
| conformance `result.waived[]` | `Report.suppressed` | with the suppression attached |
| conformance counts | `Report.counts` | text line unchanged by the display table |
| Pacenotes `RuleSpec.id`, `title`, `severity` | `Rule.id`, `.title`, `.default_severity` | |
| `RuleSpec.include`, `exclude`, `languages` | `Rule.scope` | globs bounded (64 each) |
| `RuleSpec.check` (`Pattern`, `Prose`) | `pacenotes-rules` only | the model has no matcher |
| `RuleSpec.fix`, `good`, `bad` | `Rule.fix` (required, one sentence); examples stay in `pacenotes-rules` | the examples are that crate's tests; `RuleSpec.fix` is `Option` today and becomes required (section 6) |
| Pacenotes `Location` | `Place` | revision kept |
| `FindingState` and its transitions | `State` and its transitions | moved unchanged |
| `Fingerprint` | `Finding.fingerprint` | the same 32-byte hash |
| suppression scopes line, file, path, rule | `Suppression.scope` | adds the rule-and-path pair for conformance |
| suppression `reason`, `expires` | the same | reason required; refused when blank |
| baseline file | `Baseline` | committable text format, versioned |
| exit codes 0/1/2/3 | `exit_code` | conformance uses 0/1 |
| Markdown and SARIF export | writers | |
| rdjson and SARIF import | readers | Pacenotes only |

## 6. Adoption, order and tests

1. **This note approved** (the user), then ADR for `paddock-rules` (family ADR; one per crate).
2. **A fix sentence for every rule.** `RuleSpec.fix` in `pacenotes-core` is `Option<String>` today; it becomes a required field (a rule file
   without one is refused with a message that says to add one), with its test and a change to the rule fixtures. A finding takes its
   rule's sentence. An imported SARIF or rdjson result has no rule of ours: the reader builds the rule from the tool's descriptor and,
   when the tool gave no fix text, uses a fixed sentence that names the tool and its help link ("See the documentation of <rule> in
   <tool>: <helpUri>"), so the field is never empty.
3. **C1 (`pacenotes-rules`)** is written against the types of section 3 as local definitions behind the same names, so C3a moves it by
   changing imports, not logic: the structural matcher, rule tests, SARIF and rdjson reader and writer, suppressions and baselines. It
   does not wait for the crate (plan: C1 depends on A2 and D-rules only).
4. **C3a** (plan 4.4) creates `paddock-rules` from the types and functions of C1 that section 3 calls shared, and moves conformance
   onto it in the same change (rule 42); conformance's report text, Markdown and JSON stay byte for byte (golden tests over the
   existing conformance fixtures), and a Pacenotes SARIF output and a conformance run come from one `Finding` type.
5. Tests of the shared crate, once, in the crate (ER-22): suppression and expiry arithmetic, baseline matching, fingerprint stability under
   a rename, exit codes, SARIF round trip (write then read gives the same findings), a malformed SARIF is an error, never a panic, and
   the field map of section 5 as a table-driven test.

## 7. Parallelism and cost (rule 51)

The model is data and small functions, so the cost is in the readers and the matcher. SARIF import is bounded streaming over the
file (a large clippy SARIF is tens of megabytes); findings are sorted once and the baseline lookup is a hash set, so a run is linear in
findings. The structural matcher (`pacenotes-rules`) runs per file on the index's parse pool and is the part that needs a
measurement; it is designed in C1's own note, not here.

## 8. Decisions (answered by the user, 2026-10-30)

1. Conformance `warn` maps to `Medium` and `error` to `High`. The family's gate threshold is therefore "High and above fails" for
   conformance.
2. A fix sentence is **required for every rule**, Pacenotes rules included, not only in the conformance profile (section 6 step 2 says how
   an imported result gets one).
3. The shared crate is named **`paddock-rules`**, not `paddock-repo-rules`. ADR 0022, ADR 0024, `docs/plans/debrief-1-2.md` and the
   Pacenotes plan use the new name (renamed in the same change).
4. Conformance moves onto the Rust crate at `dev check` (ADR 0022 R8); the JavaScript checks stay until then, and the golden tests
   prove the two outputs equal.
