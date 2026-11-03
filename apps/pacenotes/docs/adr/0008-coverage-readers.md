# 8. `pacenotes-cover`: coverage readers

Status: proposed (2026-10-30), design note of package CV1 (plan, stage 1). Numbered at merge.

## Context

Review and the test views need to know which lines a test run executed (patch coverage, test gaps). The reports come from other tools
and from other machines: LCOV, Cobertura, JaCoCo, llvm-cov JSON and istanbul. Each is untrusted input, written with the paths of the
machine that ran the tests, and none says which revision it measured. Pitcrew collects the artefacts; Pacenotes only reads them.

## Decision

- **Crate** `pacenotes-cover`, pure: `pacenotes-core` (paths, commit hashes), **`pacenotes-xml`** for XML (the one safe entry point of ADR 0010),
  `serde_json` for JSON. No file or network access: the port `CoverageSource` returns text; an adapter in the engine reads the file the
  user chose.
- **One model** (`CoverageReport`): per repository file, hits per instrumented line and taken/total branches per line, plus the tool
  and the revision the caller recorded. Cobertura and JaCoCo give line hits and branch counts; JaCoCo counts instructions, so a covered
  line has 1 hit and a missed one 0; llvm-cov lines are derived from its segments the way llvm-cov does (a region that ends at column 1
  leaves that line uncounted); istanbul lines come from statement starts, the larger count winning on a shared line.
- **Path mapping** (`PathMapper`): strip the build machine's checkout prefix (either separator, any case for a drive letter), refuse
  what is still absolute or leads out with `..`, and **count** what is left out (`Skipped::unmapped_files`) instead of dropping it
  silently. JaCoCo has no source folders in its report, so the mapper holds them, and an optional existence check chooses among several.
- **Revision** is recorded by the caller. `CoverageSet::add` adds hits only for runs of the **same** revision: two different revisions
  are refused (`BlendError::DifferentRevisions`), and so is any run without a revision once there is more than one run.
- **Bounds and hostile input**: report size (128 MiB), files (200,000), instrumented lines per file (2,000,000) and **in the whole
  report (10,000,000, checked while llvm-cov gaps are filled, because a few hundred bytes of segments can describe millions of lines)**,
  XML nodes (5,000,000) and XML nesting (256). A limit is an error that says what to raise. XML goes through `pacenotes-xml`, which
  refuses a `DOCTYPE` with declarations (the entity bomb), blanks a bare one (JaCoCo's) and counts nesting with a scan that
  understands quoted attributes (`<a x="/>">`). llvm-cov line numbers are 32-bit and its segments must be in order, anything else is
  a malformed report; JaCoCo branch counts are 32-bit. Malformed input is an error with the line where there is one. Counted, not
  silent: files that cannot be placed (`unmapped_files`) or do not exist (`missing_files`), lines that are not lines, llvm-cov
  segments of the wrong shape and istanbul hit counts that are not non-negative integers. The never-panics tests mutate the bytes of
  every format **and** replace each number in a report by the extremes that have broken a reader (the largest of each width, one
  past it, a negative, a float, an exponent), and feed deep and amplifying shapes.
- **Which file.** The mapper strips the **longest** matching prefix. When the caller can say what exists, every format picks the first
  candidate path that exists (JaCoCo's source folders, each Cobertura `<source>`), and a file that exists nowhere is counted, not
  guessed.
- **The revision comes from the artefact, never from HEAD.** The engine records the revision the report was made at (the commit the CI
  run built, from the run's metadata or the artefact), not whatever is checked out now: a report from last week read against today's
  HEAD would blend into the wrong revision. A report without a recorded revision cannot be blended with others.
- **Blending.** A report can carry the identity of its artefact (`run_id`); adding the same one twice is refused. Hits are
  executions where the tool counts them and 0 or 1 where it counts instructions (JaCoCo), so a blended number is a sum of different
  things: it is for "covered or not", not for counting executions.
- **Not read**: function coverage, region coverage and per-test contexts. They can be added as new maps without changing these.

## Parallelism and cost (rule 51)

A report is read once, in one pass, with memory proportional to its lines. XML is a tree (the node limit bounds it); LCOV is a line
scan. A very large report (hundreds of megabytes) is the case that would want a streaming reader; nothing is measured yet, and the
size limit stands in. Blending adds line maps, linear in lines.

## Consequences

- Patch coverage needs the lines of a diff, from the index; that join is the engine's, not this crate's.
- A path the mapper cannot place is counted, so a coverage figure can be shown with "N files not in the repository".
