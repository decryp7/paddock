# The joint rules model (D-rules)

Status: decisions answered by the user 2026-10-30 (section 10); revised after the independent review of d29a9b9ac; awaiting a second
review. Docs only; no crate is written from it until it is approved. It is the note ADR 0022 section 6 (phase 4) and Pacenotes plan
section 3.3 ask for before `paddock-rules` and before `pacenotes-rules` (C1).

## 1. Why one model

Two things in this repository report "findings": the conformance checks (`conformance/src`, JavaScript, `dev check` in ADR 0022
phase 4) and Pacenotes' review (proposal section 24, `pacenotes-core` types, `pacenotes-rules` in C1). They overlap in nearly every
concept: a rule with an id, a severity, a place, a message, a one-sentence fix, a scoped waiver with a reason and an expiry, a count of
what was waived, and a report in several formats. Building two copies breaks ER-22 and rule 42. This note fixes one model that both fit,
maps every field of both to it, and names what stays outside it.

Sources read (2026-10-30): `conformance/src/{finding,report,runner}.mjs`, `util/waivers.mjs`, `paddock.waivers.toml`, the inline waiver
markers of the checks (`conformance: not-a-header`, `freed-port`, `grace-free`, `hang-guard`); `apps/pacenotes/crates/pacenotes-core/src/
{rule,finding,tier,ids,location}.rs`; Pacenotes proposal section 24; ADR 0002 (types), ADR 0021 (counted waivers), ADR 0022 sections 6
and 7. SARIF 2.1.0 is the OASIS standard (https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html).

## 2. The two models today

| Concept | Conformance (JavaScript) | Pacenotes (`pacenotes-core`, proposal 24) |
|---|---|---|
| Rule | a check id (`C37`, `W001`) and a `rule` number (a rule of `docs/rules.md`) | `RuleSpec`: `RuleId` (`[a-z0-9][a-z0-9._/-]{0,95}`), title, severity, `include`/`exclude` globs, `languages`, `check` (`Check::Pattern(String)` or `Check::Markdown(String)`), optional `fix`, `good` and `bad` examples |
| Severity | `error` or `warn` | `Info`, `Low`, `Medium`, `High`, `Blocker` (ordered) |
| Place | `file` (optional) and `line` (default 1) | `Location = RepoPath + Span`; `Span` is two 1-based `Position`s (line, column in UTF-8 bytes) |
| Message and fix | `message`, `fix` (one sentence, required) | evidence excerpts, an explanation, a suggested fix as a diff (never applied) |
| State | none (a finding is there or not) | `FindingState`: Open, Confirmed, Dismissed, Fixed, Suppressed, with a transition table |
| Identity | none | the file-content `Fingerprint` (SHA-256 of normalised content) exists; a finding identity does not yet |
| Waiver | `paddock.waivers.toml`: `check`, `path` glob, `reason`, `expires` (all four required); a bad one is itself a finding (`W002` incomplete, `W001` expired); ADR 0021 adds **counted** waivers (a ceiling on how many findings a waiver may hide) | scoped suppressions (line, file, path, rule), required reason, optional expiry, in code comments or `.pacenotes/suppressions.toml` |
| In-source waiver | markers in the checked file (`/* conformance: not-a-header */`, `freed-port`, `grace-free`, `hang-guard`); today they carry no reason | an annotation comment with a reason |
| Baseline | none | by fingerprint; CI fails only on new findings |
| Reports | text with `::error` annotations, Markdown job summary, `conformance.json`; **one report per app** (`--app <name>`) | in-app panel, Markdown, SARIF 2.1.0, rdjson import, exit codes 0/1/2/3 |
| Counts | errors, warnings, waived | by severity, by state |
| Fails closed | a check that throws is an error finding | a rule that does not compile refuses the pack with a message |

## 3. The joint model

One crate, `paddock-rules`, pure (no I/O, no clock: today's date is passed in). It is the **single owner of the shared types** (3.1).
Names are the proposal; the crate's ADR fixes them.

### 3.1 One owner for the shared types

`paddock-rules` owns, and `pacenotes-core` re-exports (`pub use paddock_rules::{...}`, an amendment to ADR 0002 written with this note):
`Severity`, `FindingState` and its transitions, `Confidence`, `Position`, `Span`, `Location`, `RepoPath`
below, and `FindingFingerprint`. The file-content `Fingerprint`, `FileId`, `SymbolId`, `Rev`, `Tier`, `Language`, `RuleSpec` and `Check`
stay in `pacenotes-core`: they are about code, not about reporting. The shared `Rule` has **no `languages` field** (a conformance check
has none); Pacenotes' `RuleSpec` wraps a shared `Rule` and adds `languages`, `check` and the examples. ADR 0002's "Shared later" list
already names `RepoPath`; this moves the others with it.

**Temporary copies in C1.** C1 (`pacenotes-rules`) is written before the crate exists, so it holds local copies of these types in one
module, `pacenotes-rules/src/shared_copy.rs`, each marked `TODO(C3a, paddock-rules)`. They are removed in C3a. A test in
`pacenotes-rules` fails while both exist: it checks that `pacenotes-rules` has no dependency on `paddock-rules` yet **or** that the
module `shared_copy` is gone (never both present), so the copies cannot outlive the move. Until C3a, `pacenotes-core` keeps its own
`Severity`, `FindingState`, `Confidence`, `Position`, `Span`, `Location` and `RepoPath`, and C1's copies are `From`-convertible to them.

### 3.2 Types

- `RuleId` (a rule's identity) is an enum, so an imported rule can never collide with or be mistaken for one of ours:
  `Local(LocalRuleId)` with the Pacenotes validation (`[a-z0-9][a-z0-9._/-]{0,95}`; conformance ids are lower-cased on the way in and
  keep a display form, `C37`; engine findings show `W001`, `W002`), and `:` is not allowed in an external tool name (it separates tool and id), and `External(ExternalRuleId { tool, id })`. `ExternalRuleId` is **bounded** (tool at most 64 bytes, id at
  most 256, no control characters) and **lossless**: the id is exactly the string the tool gave, with no case folding or trimming, and
  it round-trips exactly through SARIF and the baseline file. Display form `tool:id`.
- `Rule`: id, title, `default_severity`, `fix` (a `FixSentence`, required), optional `policy_ref`, `version` (a number, raised when what
  the rule flags changes), `deprecated_ids` (earlier ids, so a baseline or waiver written for an old id still matches after a rename),
  and `scope` (include and exclude path globs, at most 64 each). `policy_ref` is a citation shown in reports (conformance's rule number,
  a document anchor); it is **not** a URL.
- `Severity`: the superset `Info < Low < Medium < High < Blocker`. Conformance's `warn` is `Medium` and `error` is `High` (decided); its
  report prints the old words by a display table, so its output does not change (a golden test holds that). The gate threshold is a
  severity: conformance uses "High and above fails", Pacenotes a setting.
- One span system: `Position` (1-based line, 1-based column counted in **UTF-8 bytes**) and `Span`, as in ADR 0002. A conformance finding
  with only a line is a span over that line (`Span::lines`). A repository-level finding has no `Location` at all.
- `Finding`: `RuleId`, `Severity`, optional `Location`, `message`, `fix` (a `FixSentence`, required, see 3.4), `FixSource`, optional
  `explanation`, optional `evidence` excerpts, optional `fix_diff`, optional `FindingFingerprint`, optional `Confidence`.
- `State` is **derived, not stored**: a finding is `Suppressed` exactly when the engine attached a waiver to it; Open, Confirmed,
  Dismissed and Fixed are Pacenotes' workflow states and live in its store, not in the shared `Finding`. The shared crate exposes the
  transition table and the one function `effective_state(workflow, waived) -> FindingState`, so "Suppressed" has a single source.
- Newtypes: `FixSentence` (one sentence: non-empty, at most 512 bytes, no line break; measured 2026-10-30: the longest fix string in
  conformance's source is 346 bytes, in C30, and the longest fix printed by a run over every app is 195), `Reason` (non-blank, at most 500 bytes, no line
  break), `IsoDate` (`YYYY-MM-DD`, a real calendar date).
- `Waiver` and `Suppression` are two types. A **`Waiver`** is a file entry that hides findings of a rule over a path: `rule` (or a
  deprecated id), `path` glob, `reason`, `expires`, `max_count: Option<u32>`, `owner` and `origin`. Both are built only by constructors that enforce what each origin must carry. A **file** waiver (`origin = File`, in `paddock.waivers.toml`) must have
  `expires: IsoDate`. A **config** waiver (`origin = Config`, built from a check's own exemption list) has **no expiry**: it still needs a
  `reason` and an `owner`, because such a list (`untimedCommands`, `a11y.skip`, `ownTestBinaries`) is a standing design decision, not a
  debt to pay by a date. It is still counted (`max_count`) and still raises `W003` when it hides nothing; for a config waiver the `owner` requirement is met by the CODEOWNERS owner of the check's config file, so no entry has to name one. A **`Suppression`**
  is the in-source or per-finding form Pacenotes uses: scope `Line`, `File`, `Path(glob)` or `Rule`, `reason` (required), optional
  `expires`. Both are checked by the same engine function and produce the same engine findings; they differ in where they are written and
  in what they must carry.
- `Baseline`: a set of `FindingFingerprint`s with the rule id, its `version` and a first-seen date. "New" means not in the baseline.
- `Report`: `scope` (a label: conformance's app name, or Pacenotes' repository), findings, hidden findings (each with the waiver or
  suppression that hid it), counts by severity, the rules that ran, and the rules that were **skipped** with the reason (so a green report
  never means "everything ran").

### 3.3 Finding identity: `FindingFingerprint`

A finding's identity must survive a rename of its file and a rerun, so it is **not** the file-content `Fingerprint` and it has no path in
it. `FindingFingerprint` is a 32-byte hash (SHA-256) of, in order and length-prefixed: the rule's **id**, an **algorithm version**
(`1`), the **normalised snippet** (the flagged text with line endings turned to `\n`, runs of whitespace collapsed, leading and
trailing whitespace dropped, at most 4 KiB), and the **enclosing qualified name** when the finding lies inside a symbol (ADR 0002's
qualified name; absent for a finding outside any symbol). Moving a file changes no part of it. Where two findings of one rule in
one file have the same fingerprint, the engine numbers them by order in the file (`#2`, `#3`) so the baseline keeps one entry for each.
The algorithm version is part of the hash, so changing the normalisation is a new version and the baseline file records it. It is
written to SARIF as `partialFingerprints["paddockFindingHash/v1"]`.

### 3.4 The fix sentence

A fix sentence is required for every rule and every finding (decided). Where it comes from is recorded:
`FixSource { Rule, Tool, Generated }`.

- **Our own rules**: `Rule.fix`, source `Rule`.
- **Imported results (SARIF, rdjson)**: the first available of, in order: the result's own `fixes[].description.text`, the rule's
  `help.text`, the rule's `fullDescription.text`, then the first sentence of it, cut to 512 bytes on a character boundary. Source `Tool`.
- **Nothing available**: the fixed sentence `"<tool> gives no fix for <rule>; see its documentation."` (tool and rule truncated to fit 512 bytes),
  source `Generated`. A `Generated` sentence is **never written back out as the tool's help**: the SARIF writer puts only `Rule` and
  `Tool` sentences in `help.text` or `properties.fix`, and skips `Generated` ones.
- **URLs** in a descriptor (`helpUri`) are kept only if absolute `http` or `https`, at most 2048 bytes; they are shown as plain text and
  never fetched. Opening one goes through the opt-in network path (Egress, "local first").

### 3.5 Waivers, counts and expiry (ADR 0021)

- **Expiry** is compared in **UTC**, on dates: a waiver whose `expires` is today is still in force; it is expired from the next UTC
  day. `now` is passed in. An expired waiver waives nothing and is `W001`.
- **Glob dialect** is conformance's `pathMatches`, so R8's goldens agree: a pattern whose only wildcard is a trailing `*` is a **prefix match** (`apps/*` covers `apps/x/y`, `*` covers everything, `src/fo*` covers `src/foo/bar`); otherwise `*` stays within a segment, `**` crosses segments and `?` is one character.
- **Globs** in waivers are matched in time linear in pattern length times path length (never exponential) and bounded when read: at most 256 bytes and 16 `*`. A waiver file is read leniently: a bad entry becomes a `W002` engine finding that names it, and the rest of the file still applies; a file that cannot be read at all (not TOML, wrong version) has no entries and one `W002`.
- **Counted waivers** (ADR 0021): `max_count` is an input ceiling, the most findings the waiver may hide; `hidden` is computed by the
  engine. If `hidden > max_count` the waiver **waives nothing** and the engine reports it (the findings it covered stay visible, plus
  one engine finding that names the waiver and both numbers). If `hidden < max_count` the report suggests lowering the ceiling to
  `hidden` (an `Info` finding), so the ceiling only goes down. Without `max_count` there is no ceiling.
- **Engine findings** (ids kept from conformance): `W001` expired; `W002` incomplete (a missing key or a blank reason); **`W003` unused
  waiver**, `Medium`, raised only for rules that actually **ran** in this report (a waiver for a check that was skipped, or that is not
  in this scope, is not unused); the inline "waives nothing" warning of the in-source marker checks is the same finding, not a
  second one.
- **Owner.** A waiver may name an `owner`; absent, the owner is whoever `CODEOWNERS` gives for the waiver file's path. The report prints
  the owner so an expired waiver has someone to ask.
- **In-source waivers** (the `conformance: <name>` markers; Pacenotes' annotation comments) must carry a **reason** in the same comment
  (`/* conformance: not-a-header -- the side panel is not a table row */`). A marker without one is `W002`. The separator between the marker name
  and the reason is `--` (ASCII, **canonical**: the documentation and any writer use it) or the em dash `—` (also accepted when
  reading). Markers are parsed by the rule's own check, not by this crate; they reach the engine as `Suppression`s.
- **Config exceptions** (a check's own config listing exempt names or paths: `untimedCommands`, `a11y.skip`, `ownTestBinaries`) are read by
  the check and enter the engine as `Waiver`s with `origin = Config`: no expiry, but a reason and an owner are required, they are
  counted, and an unused one is `W003`. A check that wants none of this may keep its list out of the waiver model; it then reports no
  `W003` for it.

### 3.6 Behaviour (pure functions)

1. `apply_waivers(findings, waivers, suppressions, ran, today) -> (kept, hidden, engine_findings)`: conformance's `applyWaivers`,
   generalised, with 3.5.
2. `apply_baseline(findings, baseline) -> (new, known, updated)`: per (rule, fingerprint) the baseline holds `n` entries, and the first `n` findings in file order are known (the recorded ordinal) or updated (the ordinal moved), while any beyond `n` are **new**, so a copy-pasted finding is never absorbed; a rule's `version` and `deprecated_ids` are consulted, so a rename or a version
   bump does not turn every baselined finding "new".
3. `fingerprint(rule, snippet, enclosing) -> FindingFingerprint` (3.3).
4. `exit_code(report, threshold, partial) -> 0 | 1 | 2 | 3`: 0 clean, 1 findings at or above the threshold, 2 error, 3 partial or budget
   exhausted. Conformance keeps "1 when any error" as `threshold = High`.
5. Writers: text (with the `::error`/`::warning` lines), Markdown job summary, JSON (the `conformance.json` shape is a profile), SARIF
   2.1.0, and the SARIF and rdjson **readers** (Pacenotes imports clippy, ESLint and others; conformance does not need them).

### 3.7 File formats

- **Waiver file** (`paddock.waivers.toml`, conformance): TOML, `[[waiver]]` tables with string values, plus `count` (integer, ADR 0021's key; the model field is `max_count`) and
  `owner`. `version = 1` at the top (today's JavaScript parser rejects that line; see the behaviour changes in section 6). Unknown keys are
  reported (`W002`), never guessed.
- **Suppression file** (`.pacenotes/suppressions.toml`, Pacenotes): `[[waiver]]` tables as above (rule, path glob, reason, required
  expiry) **and** `[[suppression]]` tables: `scope` (`line`, `file`, `path` or `rule`), `path` (for `line`, `file`, `path`), `line` (for
  `line`), `rule` (for `rule`, and optionally for the others), `reason` (required), `expires` (optional). One reader reads both table kinds.
- **Baseline file** (`.pacenotes/baseline.toml`, committable): TOML, `version = 1`, `algorithm = 1`, then `[[finding]]` tables with
  `rule`, `rule_version`, `fingerprint` (hex), `ordinal`, `first_seen`.
- **One parser serves the waiver files of both** (the same `[[waiver]]` reader, the same checks). The baseline has its own small reader in
  the same crate. Both are bounded (file size, entry count, string length) and never panic.

### 3.8 What stays outside the model

- **How findings are found.** Conformance's checks and Pacenotes' structural patterns and prose rules all produce `Finding`s; none of
  them lives in this crate. The pattern matcher stays in `pacenotes-rules` (it needs tree-sitter), on top of this crate.
- **The workflow around findings** in the UI (grouping, evidence popovers, learning from decisions) is Pacenotes' own.
- **Rule files.** Pacenotes' rule TOML and conformance's check registry are different sources; only rule identity and results meet here.

## 4. SARIF 2.1.0 mapping

| Model | SARIF 2.1.0 |
|---|---|
| `Report` | **one `run` per tool**: our findings in ours, and each imported tool's results written back in **that tool's own `run`** with its **raw rule id** in `ruleId`, so an external result round-trips to the same run and the same id; `tool.driver.name` and `version`; `rules` from the rules that ran; `originalUriBaseIds` declares `%SRCROOT%`; `columnKind: "utf16CodeUnits"` is **not** used (see below) |
| rule | `reportingDescriptor`: `id` (the `RuleId` display form), `name`, `shortDescription` (title), `defaultConfiguration.level`, `help.text` (a `Rule`/`Tool` fix sentence), `properties.policyRef`, `properties.ruleVersion`; `helpUri` only for an absolute http(s) URL, never for `policy_ref` |
| result to rule | `ruleId` **and** `ruleIndex` (the index into `rules`) |
| severity | `level`: Info -> `note`, Low and Medium -> `warning`, High and Blocker -> `error`; the exact severity in `properties.severity` so a round trip is lossless |
| `Location` | `result.locations[0].physicalLocation`: `artifactLocation` with `uri` (relative) and `uriBaseId: "%SRCROOT%"`; the region, below |
| region and columns | ours are **UTF-8 bytes**. The writer emits `region.startLine`, `startColumn`, `endLine`, `endColumn` **in code points** (`run.columnKind = "unicodeCodePoints"`), converting from bytes, and also `byteOffset` and `byteLength` in `region` where the byte offset is known. The reader converts from the **declared** `columnKind` (`utf16CodeUnits`, which is the SARIF default when absent, or `unicodeCodePoints`) to bytes using the file's text when it has it, and otherwise keeps the line and drops the columns, counting it. Each file's text is fetched **once per run** (the cache keeps the last 16 files, so results of one file should be together) and lines are found by binary search over a line-start index; a duplicate (rule, fingerprint, ordinal) in a baseline file is refused, in the writer and the readers. A round-trip test with non-ASCII text (a two-byte, a three-byte, and a four-byte character before the finding, plus a surrogate pair for the UTF-16 case) proves the columns survive |
| `message`, `fix` | `result.message.text`; a per-finding fix sentence in `properties.fix`; a `fix_diff` (a unified diff, which `fixes[].artifactChanges` cannot carry as it is) in `properties.fixDiff`, with no `fixes` written; the output is validated against the vendored official 2.1.0 schema in a test; a `Generated` sentence is not written |
| `FindingFingerprint` | `result.partialFingerprints["paddockFindingHash/v1"]` |
| waiver or suppression | the result **stays in `results`** with `suppressions[]` set: `kind` `inSource` or `external`, `status` `accepted`, `justification` = reason, `properties.expires`, `properties.owner` (the writer is given the waiver or suppression itself). On read **only an `accepted` suppression, or one with no status, hides a result**; `rejected` and `underReview` do not |
| baseline | `result.baselineState` written from the comparison: `new`, `unchanged`, `updated`, `absent` (`absent` for a baselined finding no longer found, emitted as a result with no location only when the report asks to list them) |
| workflow state | `result.properties.state` (SARIF has no Confirmed or Dismissed beyond suppressions) |
| `scope` | `run.automationDetails.id` |

**Reader rules.** The reader reads a result only if `kind` is `fail` or absent (`pass`, `open`, `review`, `notApplicable`, `informational`
are skipped and counted). `level` absent means: the rule's `defaultConfiguration.level`, and if that is absent too, `warning` (SARIF's
defaults). A result with `suppressions` becomes a hidden finding with that waiver as an `external` or `inSource` suppression, not a
dropped one. The input is **bounded** (file size, results count, string length, nesting) and the reader never panics on any input
(rule 36); unknown fields go to `properties` up to a cap and are otherwise ignored. rdjson (reviewdog) is read the same way:
`diagnostics[]` with `message`, `location.path`, `location.range` (rdjson columns are **UTF-8 bytes**, 1-based, the end exclusive, as in reviewdog's proto, so they are ours and need no conversion; each must fall on a character boundary when the file's text is at hand, else the columns are dropped and counted), `severity`,
`code`, `suggestions`. An imported rule is an `ExternalRuleId { tool, id }`, so it round-trips to the same `ruleId`.

## 5. Field-by-field map (the table the adoption test iterates over)

| Source field | Model field | Note |
|---|---|---|
| conformance `check` | `Finding.rule` (`RuleId::Local`) | upper-case display form kept |
| conformance `rule` (number) | `Rule.policy_ref` | shown as "rule 37"; SARIF `properties.policyRef` |
| conformance `severity` `error` / `warn` | `Severity::High` / `Severity::Medium` | display table keeps the words |
| conformance `file`, `line` | `Location` (a span over the line) | `file` absent: no `Location` |
| conformance `message`, `fix` | `Finding.message`, `Finding.fix` | `fix` required for every rule |
| conformance per-app run (`--app`) | `Report.scope` | one report per scope; the Markdown title keeps "Conformance: <app>" |
| conformance waiver `check`, `path` | `Waiver.rule`, `Waiver.path` | same glob dialect as `pathMatches` |
| conformance waiver `reason`, `expires` | `Waiver.reason`, `.expires` | both required; expiry day inclusive, UTC |
| ADR 0021 counted waiver (file key `count`) | `Waiver.max_count` (ceiling) and computed `hidden` | not implemented in JS yet (section 6); above the ceiling the waiver waives nothing |
| conformance `W001`, `W002` | engine findings | ids kept |
| unused waiver (new) and the inline "waives nothing" warning | engine finding `W003`, `Medium` | only for rules that ran |
| in-source markers (`conformance: ...`) | `Suppression` from the check, reason required | a marker without a reason is `W002` |
| config exceptions of a check | `Waiver` | counted and owned like any waiver |
| waiver owner (CODEOWNERS) | `Waiver.owner` or the CODEOWNERS owner of the file | printed in the report |
| `result.waived[]` | `Report.hidden` | with the waiver attached |
| conformance counts | `Report.counts` | the text line is unchanged by the display table |
| Pacenotes `RuleSpec.id`, `title`, `severity` | `Rule.id` (`Local`), `.title`, `.default_severity` | |
| `RuleSpec.include`, `exclude` | `Rule.scope` | at most 64 globs each |
| `RuleSpec.languages` | stays in `RuleSpec` | not in the shared `Rule` |
| `RuleSpec.check` (`Pattern`, `Markdown`) | `pacenotes-rules` only | the model has no matcher |
| `RuleSpec.fix` (`Option`) | `Rule.fix` (`FixSentence`, required) | becomes required in `pacenotes-core` (6.2) |
| `RuleSpec.good`, `bad` | stay in `pacenotes-rules` | the examples are that crate's tests |
| Pacenotes `Location` | `Location` | shared; `pacenotes-core` re-exports |
| `FindingState` and its transitions | the same type | shared; "Suppressed" derived (3.2) |
| file-content `Fingerprint` | unchanged, not used for findings | |
| (new) finding identity | `FindingFingerprint` | 3.3 |
| suppression scopes line, file, path, rule | `Suppression.scope` | |
| suppression `reason`, `expires` | `Reason`, `IsoDate` | reason required; refused when blank |
| baseline file | `Baseline` | 3.7 |
| exit codes 0/1/2/3 | `exit_code` | conformance uses 0/1 |
| Markdown and SARIF export | writers | |
| rdjson and SARIF import | readers | Pacenotes only; imported ids are `ExternalRuleId` |
| rule rename or change | `Rule.version`, `Rule.deprecated_ids` | baseline and waivers still match |

## 6. Adoption, work packages and tests

1. **This note approved** (the user), then the ADR for `paddock-rules` (a family ADR), and the amendment to ADR 0002 (3.1).
2. **`RuleSpec.fix` becomes required** in `pacenotes-core` (an A1 follow-up that C1 needs): a rule file without one is refused with a
   message that says to add one, with its test and a change to the rule fixtures.
3. **C1 (`pacenotes-rules`)** is written against the types of section 3 as the temporary module `shared_copy` (3.1): the structural
   matcher, rule tests, SARIF and rdjson reader and writer, waivers/suppressions, baselines and the `FindingFingerprint`. It does not
   wait for the crate (the plan: C1 depends on A2 and D-rules only). Its tests include the non-ASCII SARIF round trip, the
   `ExternalRuleId` round trip, the fix-source rules (3.4), counted waivers (3.5) and the "copies cannot outlive the move" test.
4. **C3a** (plan 4.4) creates `paddock-rules` from the shared half of C1, **moves `pacenotes-rules` and `pacenotes-core` onto it**, and
   deletes `shared_copy`. Conformance is **not** moved in C3a.
5. **R8** (ADR 0022, `dev check`) moves conformance onto the crate. **R8 owns the golden fixtures**: the JavaScript conformance run and the
   Rust `dev check` run over the same fixtures (`conformance/fixtures` and `main`) must give byte-identical text, Markdown and JSON.
   Those fixtures and their golden files are R8's failing test, written before the move. Until R8 the JavaScript checks stay.
6. Tests of the shared crate, once, in the crate (ER-22): waiver and expiry arithmetic (the expiry day inclusive, UTC), counted waivers
   above and below the ceiling, `W003` only for rules that ran, baseline matching across a rename (same fingerprint, new path), a rule
   rename through `deprecated_ids`, exit codes, SARIF round trips (including non-ASCII and external rule ids), a malformed SARIF is an
   error and never a panic, and the field map of section 5 as a table-driven test.

### Behaviour changes against today's JavaScript conformance

The stricter rules of this note change what conformance reports, and R8's goldens compare the JavaScript run with the Rust run byte for
byte. So each change has one place where it lands, and none lands in R8 silently:

| Change | Lands | How the goldens stay honest |
|---|---|---|
| `version = 1` in `paddock.waivers.toml` (today's parser reports the line as `W002`) | **JS conformance first**, in a change of its own before R8: the parser accepts a `version = 1` line and refuses another version | the goldens then compare like with like |
| In-source markers must carry a reason (`W002` without one); 15 markers have none today | **R8**, as a deliberate golden update, in the **same change that gives each of the 15 markers its reason**, so the run stays green; the JS checks do not enforce it earlier | the golden diff is only the new rule, and the marker fixes are in the same commit |
| `W003` unused waiver (not on `main` yet) | **JS conformance first**, as a change to the waiver engine with its own tests, raised only for checks that ran; waivers it finds unused are removed or fixed in that change | the goldens then contain `W003` on both sides |
| Config exceptions become waivers (`origin = Config`, no expiry) | **R8**: the Rust checks read their lists into waivers; JS keeps reading them as lists. JS's `W003` does not cover config lists, so an unused config entry (`W003` in Rust) is a **deliberate golden update in R8**, listed with the entries it flags and fixed or removed in that same change | the Rust run reproduces the JS output over the same lists |
| Counted waivers (`count` in the waiver file, `max_count` in the model) | **They do not exist yet**: there is no C38 and no count in `waivers.mjs` today. ADR 0021 describes them; whichever lands first, ADR 0021 WP-2 in JS or R8, defines them, and the goldens include them from then on. The file key stays **`count`** (ADR 0021), read into the model's `max_count` | no output change before they exist; after, both sides read the same key |
| `Report.scope` and the display table (`warn` printed as `warn`) | **R8**, designed to leave the text, Markdown and JSON unchanged | goldens unchanged |

## 7. Parallelism and cost (rule 51)

The model is data and small functions, so the cost is in the readers and the matcher. SARIF import is bounded streaming over the file (a
large clippy SARIF is tens of megabytes); findings are sorted once and the baseline lookup is a hash set, so a run is linear in
findings. The byte-to-code-point column conversion needs the file's text; it is done once per file with findings, from the index's
stored text or one read, not per finding. The structural matcher (`pacenotes-rules`) runs per file on the index's parse pool and is the
part that needs a measurement; it is designed in C1's own note, not here.

## 8. Risks

- A shared `Severity` change later touches both apps' reports; the display table and the golden fixtures are the guard.
- Fingerprint **ordinals can shift**: two identical snippets in one file are numbered by order, so deleting or inserting one above them
  renumbers the others and a baselined finding can look new. The baseline lists the ordinal, and a finding whose fingerprint matches but
  whose ordinal moved is reported as `updated`, not `new`, so CI does not fail on it.
- `FindingFingerprint` normalisation is a contract: changing it is a new algorithm version and a baseline migration, never a quiet
  edit.
- Moving `pacenotes-core` types behind a re-export must not change any wire encoding (`wire_tests` runs before and after).

## 9. What this note changes elsewhere

ADR 0002 gets an amendment (3.1). ADR 0022 gets a one-line amendment for the crate rename (`paddock-repo-rules` to `paddock-rules`). The
plan already uses the new name.

## 10. Decisions (answered by the user, 2026-10-30)

1. Conformance `warn` maps to `Medium` and `error` to `High`. The family's gate threshold is therefore "High and above fails" for
   conformance.
2. A fix sentence is **required for every rule**, Pacenotes rules included, not only in the conformance profile (3.4 says how an
   imported result gets one).
3. The shared crate is named **`paddock-rules`**, not `paddock-repo-rules`. ADR 0022, ADR 0024, `docs/plans/debrief-1-2.md` and the
   Pacenotes plan use the new name.
4. Conformance moves onto the Rust crate at `dev check` (ADR 0022 R8); the JavaScript checks stay until then, and the golden tests
   prove the two outputs equal (section 6 step 5 names the owner).
