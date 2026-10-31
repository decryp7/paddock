# wiki

The LLM-wiki tool, one implementation for every project (package `paddock-wiki`, binary `wiki`). It replaces the per-project
Node lint scripts. The practice and the page format are in [docs/wiki/README.md](../../docs/wiki/README.md).

```sh
cargo install --path tools/wiki --locked     # from a Paddock checkout; later from the decryp7 registry
wiki lint [--wiki docs/wiki] [--repo .] [--rules <file|git-ref:path>]... [--er RULES.md] [--today YYYY-MM-DD] [--strict] [--json]
wiki log <ingest|lint|fix> "<page | subject>"      # appends `## [today] kind | subject` to log.md
wiki new <slug> --title "..." [--summary "..."] [--source ...]...   # page from the template, plus its index.md line
```

- **Exit codes** are `paddock-cli`'s (ADR 0022): 0 ok; 1 an I/O failure (an unreadable wiki folder, settings or rules file);
  2 a wrong command line; 4 refused (`wiki new` on a page that exists); 5 findings (errors, or warnings with `--strict`).
- **Output**: errors as `error <file>:<line>: <message>` on standard error; warnings, notes and the summary
  `wiki-lint: N error(s), M warning(s); K rules source(s)` on standard output. With `CI`, `GITHUB_ACTIONS` or `GITEA_ACTIONS`
  equal to `true` each finding is `::error::wiki-lint: ...` or `::warning::wiki-lint: ...`, and a failure is one
  `::error::wiki-lint: <cause> <next step>`. `--json`: standard output is exactly one object (`errors`, `warnings` as
  `{file, line, message}`, counts, `rules_sources`; or the error object with `ok: false`).
- **Log (ER-27)**: every run writes a per-run CLEF JSON-lines file (`wiki-<time>-<OperationId>.log`) in `$WIKI_LOG_DIR` (`off`
  disables), else `~/Library/Logs/wiki`, `$XDG_STATE_HOME/wiki` or `~/.local/state/wiki`, `%LOCALAPPDATA%\wiki\logs`; the
  newest 20 are kept. Its first line has the version, arguments, OS and whether CI mode is on; then phases with durations, every
  git command exactly as run with its exit code (and the tail of its standard error when it failed), every rules source and
  whether it was found, pages checked, and a summary with the exit code. `--log-file <file>` appends to a file of your choice;
  `--verbose` prints the same lines live on standard error. Never file contents.
- **Settings**: `wiki-lint.json` (`wiki`, `rules`, `ruleBranches`, `citeByLine`, `maxAge`, `er`), found by walking up from
  `--repo` (default: the current folder) to the git top folder, so it works from any subfolder. Paths in it are relative to its
  folder; flags are relative to where you run the command, and win. `--config <file>` names it explicitly.
- **Rules sources** (`--rules`, repeatable, or `rules`): a file, or `<git ref>:<path>` read with `git show`. `ruleBranches` are
  patterns such as `origin/docs/*`, resolved with `git for-each-ref`: every rules *file* is also read from each matching
  branch (open rule branches). A listed ref that does not exist here, or a file a branch lacks, is a **warning**; any other
  unreadable source is an **error** with the cause and the next step. With no rules listed in the flags or the settings the
  defaults are `docs/rules.md` and `origin/main:docs/rules.md` under the settings folder (each when readable). The engineering
  rules are one more source: `--er` / `er` (a **warning** if that file does not exist), else `../engineering-rules/RULES.md`
  next to the settings folder when it exists. One copied span is one finding, naming up to three of the rules sources that
  contain it and counting the rest.
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
