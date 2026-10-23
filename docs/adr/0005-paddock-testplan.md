# 5. `paddock-testplan`: `.sighterplan` and `.sighterresult`

- Status: proposed (2026-10-23)
- Related: Sighter proposal section 7.2 and 7.3; Pacenotes requirements and tests plugins

## Context

Pacenotes drafts UI test cases from requirements; Sighter imports them, runs them, and returns results so Pacenotes can show
requirement coverage. Hand-written plans are equally valid. Two tools must not drift, so the schema, parser and writer live in
one pure crate with no I/O beyond reading bytes. Files are TOML (reviewable in a pull request); `.sighterresult` is TOML too, with
evidence by reference (paths relative to the result file), never inline blobs.

## Decision

```
crates/paddock-testplan/src/
  lib.rs  plan.rs  result.rs  ids.rs  step.rs  secret.rs  validate.rs  diff.rs  junit.rs  error.rs
  tests/ fixtures/ (invented data only)  fuzz/
```

### Common rules

- `format = "sighterplan" | "sighterresult"`, `version = <u32>` (major only; additive keys are not a version bump). Unknown keys
  are an error naming the line (`deny_unknown_fields`) at the **same major**, and a file with a newer major is refused with
  "this file needs Sighter/Pacenotes N or newer". A reader accepts the last two majors.
- Limits: file 4 MiB, 5,000 cases, 200 steps per case, strings 4 KiB (ids 64), nesting as the grammar allows. Parse, don't
  validate: `parse_plan(&[u8]) -> Result<Plan, PlanError>` yields typed values; `Plan` is immutable and `Plan::to_toml()` is canonical
  (stable key and entry order) so a regenerated plan diffs cleanly.
- **Ids** are newtypes: `RequirementId`, `AcceptanceRef` (`REQ-142#AC-1`), `CaseId`, `ScreenName`, `PlanId`. Charset `[A-Za-z0-9._:/#-]`, max 64.
  Requirement ids come from the tracker or Pacenotes' requirements plugin and are opaque text; they appear in every case, step
  result and JUnit/Allure export.

### Plan

```rust
pub struct Plan { pub id: PlanId, pub title: String, pub app: AppRef, pub generated_by: Option<Generator>,
                  pub requirements: Vec<Requirement>, pub screens: Vec<ScreenExpect>, pub cases: Vec<Case>, pub data: Vec<DataTable> }
pub struct Requirement { pub id: RequirementId, pub title: String, pub acceptance: Vec<String>, pub source: Option<SourceLink> } // a link, never file contents
pub struct Case { pub id: CaseId, pub title: String, pub requirements: Vec<ReqRef>, pub priority: Priority, pub tags: Vec<Tag>,
                  pub data: Option<DataName>, pub steps: Vec<Step> }
pub enum Step {                                   // closed enum: the allowed actions are exactly these
    Click{ target: Target }, Type{ target: Target, value: Value }, Press{ keys: String }, Select{ target: Target, option: Value },
    WaitFor{ target: Target, timeout_ms: u32 }, AssertScreen{ expect_screen: ScreenName, timeout_ms: u32 },
    AssertText{ target: Option<Target>, text: Value }, Wait{ ms: u32 },
}
pub struct Target { pub text: Option<String>, pub name: Option<String>, pub class: Option<ControlClass>, pub inside: Option<ScreenName>,
                    pub near: Option<Box<Target>>, pub nth: Option<u16> }       // relative queries like Sighter's resolver
pub enum Value { Literal(String), Data(Column), Secret(SecretRef) }             // "{{data.col}}", "{{secret:NAME}}"; parsed once
```

### Secrets by reference

`SecretRef(NAME)` (`[A-Z0-9_]{1,64}`) is the only way a secret appears. A literal that *looks* like a secret in a field named
`password`/`token`/`card` triggers a **lint warning**, not an error. `Resolver` is a trait implemented by Sighter (OS keychain,
`sighter` service); the crate never resolves. Writers (`result.rs`, `junit.rs`) take a `Redactor` and replace resolved values by
`{{secret:NAME}}` so reports never contain them. Data tables may have `secret = true` columns that hold references too.

### Result

```rust
pub struct RunResult { pub plan: PlanId, pub plan_hash: String /* sha256 of canonical plan */, pub run_id: String, pub started: Timestamp,
                       pub env: Environment /* os, dpi, theme, language, resolution, app build, sighter version */, pub cases: Vec<CaseResult> }
pub struct CaseResult { pub case: CaseId, pub requirements: Vec<ReqRef>, pub status: Status, pub iteration: Option<u32> /* data row */,
                        pub duration_ms: u64, pub steps: Vec<StepResult>, pub evidence: Vec<EvidenceRef>, pub reason: Option<String> }
pub enum Status { Pass, Fail, Blocked, NeedsRecording, Skipped }                  // never a bool
pub struct StepResult { pub index: u32, pub status: Status, pub screen_seen: Option<ScreenName>, pub score: Option<f32>, pub evidence: Vec<EvidenceRef> }
pub struct EvidenceRef { pub kind: EvidenceKind /* Screenshot|Recording|OcrText */, pub path: RelPath, pub sha256: String } // RelPath rejects ".." and absolute
```
`plan_hash` lets Pacenotes show "result is for an older version of the plan"; `Coverage::from(plan, results) -> Matrix`
(requirement x case x last status) is provided here as pure logic so both tools compute traceability identically. `diff(old, new)
-> PlanDiff` (new/changed/removed steps, links kept by case id and step text) is shared for Sighter's re-import. `junit::write`
puts requirement ids in `<properties>`.

### Errors

`PlanError { Syntax{line,col,msg}, UnknownKey{path,line}, Version{found,supported}, Limit{what,max}, BadId{value}, BadSecret{..},
DanglingRef{kind,id}, Duplicate{kind,id} }` all with a fix-it message.

## Consequences

- Sighter's `sighter-plan` and Pacenotes' tests plugin both depend on this crate only; neither knows the other's internals.
- Sighter's UI test execution and Pacenotes' requirements are decoupled by files; no live protocol is needed in v1.
- A fuzz target and property test (`parse(write(p)) == p`) are mandatory before release.

## Open questions

Add JSON Schema export for editors; whether `ControlClass` should be a closed enum (proposed, aligned with Sighter's detector
classes) or free text; whether results may embed small thumbnails (proposed: no).
