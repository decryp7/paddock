# 3. Tree-sitter grammars for tier 2: which, in what order, under which licence

Status: proposed (2026-10-29), the licence record that package A2 (`pacenotes-parse`) needs before it adds a grammar.
Numbered at merge. Docs only: no dependency is added by this ADR.

## Context

Tier 2 (proposal 20.1) is tree-sitter: symbols, outline, imports, call candidates and chunks for every language with a
grammar. AGENTS.md requires the licence of every dependency and grammar to be recorded in the ADR that adopts it, verified
against the official repository (no GPL, AGPL, LGPL or source-available code). The user fixed the order on 2026-10-29
(plan 7.1, item 4): **Go, Java, Kotlin, C and C++, then C#, TypeScript/JavaScript, Rust, Python**.

## Decision

Use the tree-sitter Rust runtime and one published grammar crate per language, pinned to the versions below in
`Cargo.lock` (Renovate updates them through its group; the grammar and the runtime move together because each grammar crate
depends on `tree-sitter-language`). Core grammars are compiled in; a grammar added by a pack is a later decision (proposal
7.2, wasm) and is not covered here.

Checked on 2026-10-29. The licence column is the `license` field of the crates.io release **and** the first lines of the
`LICENSE` file at the head of the repository named in the crate's `repository` field; both agree for every row.

| Order | Language | Crate | Version | Licence | Repository (LICENSE copyright line) | Last crates.io release |
|---|---|---|---|---|---|---|
| runtime | (parser runtime) | `tree-sitter` | 0.27.1 | MIT | tree-sitter/tree-sitter (Max Brunsfeld, 2018) | 2026-10-08 |
| 1 | Go | `tree-sitter-go` | 0.25.0 | MIT | tree-sitter/tree-sitter-go (Max Brunsfeld, 2014) | 2025-08-29 |
| 2 | Java | `tree-sitter-java` | 0.23.5 | MIT | tree-sitter/tree-sitter-java (Ayman Nadeem, 2017) | 2024-12-21 |
| 3 | Kotlin | `tree-sitter-kotlin-ng` | 1.1.0 | MIT | tree-sitter-grammars/tree-sitter-kotlin (Amaan Qureshi, 2024) | 2025-01-09 |
| 4 | C | `tree-sitter-c` | 0.24.2 | MIT | tree-sitter/tree-sitter-c (Max Brunsfeld, 2014) | 2026-04-22 |
| 5 | C++ | `tree-sitter-cpp` | 0.23.4 | MIT | tree-sitter/tree-sitter-cpp (Max Brunsfeld, 2014) | 2024-11-11 |
| 6 | C# | `tree-sitter-c-sharp` | 0.23.5 | MIT | tree-sitter/tree-sitter-c-sharp (Max Brunsfeld and contributors, 2014-2023) | 2026-04-14 |
| 7 | TypeScript, TSX | `tree-sitter-typescript` | 0.23.2 | MIT | tree-sitter/tree-sitter-typescript (Max Brunsfeld, 2017) | 2024-11-11 |
| 8 | JavaScript, JSX | `tree-sitter-javascript` | 0.25.0 | MIT | tree-sitter/tree-sitter-javascript (Max Brunsfeld, 2014) | 2025-09-01 |
| 9 | Rust | `tree-sitter-rust` | 0.24.2 | MIT | tree-sitter/tree-sitter-rust (Maxim Sokolov, 2017) | 2026-03-27 |
| 10 | Python | `tree-sitter-python` | 0.25.0 | MIT | tree-sitter/tree-sitter-python (Max Brunsfeld, 2016) | 2025-09-11 |

All are MIT, which `deny.toml` allows. The generated parsers and the C runtime are part of these crates and carry the same
licence. The crates build C code with the `cc` crate, so a C compiler is needed on every CI leg (the images have one; the
Windows leg uses the MSVC toolchain that Rust already requires).

### Kotlin

Two MIT crates exist: `tree-sitter-kotlin` 0.3.8 (fwcd, last release 2024-08-03) and `tree-sitter-kotlin-ng` 1.1.0
(tree-sitter-grammars, last release 2025-01-09). Choose **`-ng`**: it is the one in the `tree-sitter-grammars` organisation
and the newer; the older crate stays on an older tree-sitter ABI. Kotlin is tier 2 only in v1 (its language-server pack is
later, plan 7.1 item 4). If `-ng` proves incomplete on the golden files of A2 (outline of classes, objects, extension
functions, `suspend`, DSL lambdas), the decision is revisited with that evidence, not before.

### What A2 must still check when it adds each crate

1. `cargo deny check` passes with the crate (its transitive tree is `tree-sitter-language`, `cc` and the runtime; any
   other licence would be recorded here before merging).
2. The crate's `tree-sitter-language` version matches the runtime's; one `Cargo.lock` entry per crate.
3. The golden outline of the language (generated files, no real code) passes before the next language is added: the
   order above is also the order of work, one commit per language.

## Limits (what the parser library enforces, per file)

A file over the byte cap (default 2 MiB, a setting) is not parsed; its tier is 1 and the reason is recorded. The runtime is
given a **node cap** (the walk stops and the file is marked partial) and a **progress callback** that stops a parse when the
job is cancelled; no wall-clock timeout is used to judge a parse (ADR 0031): a worker's stall is judged by the supervisor in
stage 2. Arbitrary bytes never panic: invalid UTF-8 is parsed as bytes, and a parse that returns no tree is an error value.
The conformance check C36 (parser hardening) applies.

## Parallelism and cost (rule 51)

Parsing is CPU-bound and independent per file: the index job maps files over a pool of cores minus one at background
priority, results in input order. The cheap paths are not parsing at all: files whose `Fingerprint` is unchanged are
skipped, and a language the repository does not contain loads no grammar (grammars are constructed on first use, and a
parser is reused per thread). Measurements, when A2 has them, go to `docs/performance.md` (rule 48).

## Consequences

- Eleven small MIT dependencies and no licence exception.
- Adding a language means one crate, one row here and a golden outline, in that order.
- A grammar that lags its language shows as a lower pack-quality figure in the UI rather than as a hidden gap.
