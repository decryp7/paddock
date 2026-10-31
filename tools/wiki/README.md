# wiki

The LLM-wiki tool, one implementation for every project (package `paddock-wiki`, binary `wiki`). It replaces the per-project
Node lint scripts. The practice and the page format are in [docs/wiki/README.md](../../docs/wiki/README.md).

```sh
cargo install --path tools/wiki --locked     # from a Paddock checkout; later from the decryp7 registry
wiki lint [--wiki docs/wiki] [--repo .] [--rules <file|git-ref:path>]... [--er RULES.md] [--today YYYY-MM-DD] [--strict] [--json]
wiki log <ingest|lint|fix> "<page | subject>"      # appends `## [today] kind | subject` to log.md
wiki new <slug> --title "..." [--summary "..."] [--source ...]...   # page from the template, plus its index.md line
```

- **Exit codes** (this tool's contract): 0 ok, 1 findings (errors, or warnings with `--strict`), 2 usage or I/O errors (a rules
  source or the wiki folder cannot be read, a bad argument, a page that already exists). `paddock-cli`'s own numbers differ
  (findings 5); this tool keeps the 0/1/2 the Node lints had, so existing callers do not change.
- **`--json`**: standard output is exactly one object (`errors`, `warnings` as `{file, line, message}`, counts,
  `rules_sources`; or the error object with `ok: false`). With the `CI` variable set, annotations
  (`::error file=...,line=...::message`) go to standard error, and so does `::error::` for a failure.
- **Settings**: `wiki-lint.json` (`wiki`, `rules`, `ruleBranches`, `citeByLine`, `maxAge`, `er`), found by walking up from
  `--repo` (default: the current folder) to the git top folder, so it works from any subfolder. Paths in it are relative to its
  folder; flags are relative to where you run the command, and win. `--config <file>` names it explicitly.
- **Rules sources** (`--rules`, repeatable, or `rules`): a file, or `<git ref>:<path>` read with `git show`. `ruleBranches` are
  patterns such as `origin/docs/*`, resolved with `git for-each-ref`: every rules *file* is also read from each matching
  branch (open rule branches). A listed ref that does not exist here, or a file a branch lacks, is a **warning**; any other
  unreadable source is an **error** with the cause and the next step. With no rules the defaults are `docs/rules.md` and
  `origin/main:docs/rules.md` (when readable); the engineering rules (`--er`, default `../engineering-rules/RULES.md` next to
  the settings folder) are added when the file exists.
- **`--cite-by-line <name>`** (or `citeByLine`): a line that mentions the log `name` and a clock time like 14:33 is an error;
  cite `name:<line>` instead (the generalised steward-log check).
- **Dates** are the machine's local date, like `date +%F` (the clock is injectable in tests; `--today` overrides). A log line's
  date must be a real date. Pages named `*-template.md` are templates and are skipped.
- **Bounds**: at most 5000 Markdown files of at most 4 MiB each in a wiki folder, rules sources of at most 4 MiB, at most 256
  rules sources, subjects, titles and sources of at most 500 characters, `git` ended after 30 s. Line ends
  `\r\n` read as `\n`.
- The checks are those of the Node scripts this replaced (links and anchors, index coverage, orphans, `verified:` and
  `sources:`, staleness, a source in every bullet, the log line shape, no run of 8 words copied from a rules source in any
  page, README, index or log). `tests/golden/cases/` is a shared suite: wiki folders with the expected output, run through the
  command line (see the header of `tests/it/golden.rs`).

## Tests

`cargo nextest run` in this folder. `tests/it/lint_cases.rs` are the Node script's 18 cases; `golden.rs` is the parity
golden; `hostile.rs` is the never-panics and bounds tests; git-ref sources are tested against a temporary
repository created by the test.
