---
name: affected-plan
description: Use when deciding which tests and CI jobs a change reaches, reading `scripts/affected.mjs --explain`, or changing the affected-plan planner.
---

- **Affected tests (rule 3).** `node scripts/affected.mjs origin/main --explain` lists the projects a change reaches (the
  Cargo and npm graphs, Tauri `frontendDist`, `tsconfig`/`angular.json`/ESLint config paths, declared doc inputs). Run
  those, plus the touched specs. CI runs the same plan; nightly.yml runs everything.
  Only `rust-toolchain`, the root `package.json`, `.npmrc`, `.nvmrc`, `.gitattributes` and `tsconfig/` still mark
  everything; the table above `GLOBAL` in `scripts/affected.mjs` says what every other former global reaches:
  `scripts/affected*.mjs` mark tools, ui and paddock-dev (the `plan` and `tools` jobs run their tests; the golden test of
  `dev` uses the script as oracle), `.cargo/` and `lint/` the Rust builds (`lint/` also conformance), a `ci.yml` change only
  the jobs whose text changed (everything if the file cannot be split into jobs, uses YAML anchors, its header changed or
  the `plan` job changed), a shared action or a script of `conformance/ci` or `conformance/release` only the jobs and
  workflows that use it, following scripts that import or name it (imports built at run time are not seen), reusable
  workflows to their callers, and ignoring mentions in comments and README prose; an action nobody uses marks everything.
  `--files` means working tree against HEAD: a committed change cannot be diffed, so it marks everything. Users that run only
  nightly, on a tag or manually are named as such in `--explain`; `e2e-webkit` never runs on a pull request. The Rust port
  in `tools/paddock-dev` keeps `.gitea/` and `conformance/(ci|release|src/util)/` global (a superset of the script).
