# Paddock wiki

A small, sourced summary of how things work **today**, so an agent can read a page instead of re-deriving the state from
the lessons, the ADRs and the hand-off logs. It started on 2026-10-31 as a two-week pilot of the LLM Wiki pattern; the user
concluded the pilot and made it official the same day (ER-30 and `skills/wiki` on engineering-rules `docs/er-30-wiki`, Paddock rule 59 on `docs/rule-wiki`).
The tooling is one Rust `wiki` CLI, [tools/wiki](../../tools/wiki/README.md), which replaced `scripts/wiki-lint.mjs`. The
design and the evaluation plan are in the research note `research-llm-wiki-2026-10-31.md` (hand-off folder, outside this
repository), and the pattern's primary source is Karpathy's
[LLM Wiki gist](https://gist.github.com/karpathy/442a6bf555914893e9891c11519de94f) (read as data, adapted, not adopted as
written).

## What belongs here and what does not

- Here: the current state of a topic (how CI, releasing, logging, test isolation and process starts work now), the open
  decisions, and links to where the truth lives.
- Not here: rules, ADR decisions, the copies register, the conformance checks. They stay authoritative in
  [rules.md](../rules.md), [adr/](../adr/README.md), [known-copies.toml](../known-copies.toml) and
  [conformance/README.md](../../conformance/README.md). A wiki page **links** to them and says what they are about; it
  never restates their text. Where a page disagrees with one of them, the page is wrong and is fixed.
- Not here: lessons. [lessons.md](../lessons.md) keeps the incidents; a page may link a lesson as a source.

## Layers

1. Sources, never edited by the wiki: code, workflows, ADRs, rules, lessons, commits, CI runs, hand-off files.
2. The wiki: the topic pages in this folder, plus `index.md` (one line per page) and `log.md` (one dated line per change).
3. This schema.

## Page format

```markdown
---
title: CI and runners
verified: 2026-10-31
sources: [.gitea/workflows/ci.yml, rule 50, ADR 0025]
---
Summary in three to five lines.

## Current state
- One claim per bullet, with its source in the same bullet: a Markdown link, or `src: <path or id>` for a source that
  cannot be linked (a hand-off file outside the repository, a CI run, a commit that is not on main).

## Open questions
## See also
```

- `verified: YYYY-MM-DD`: the day someone last checked every claim of the page against its sources. An old date draws a lint
  warning (30 days by default); the writer re-verifies the page, nobody deletes it.
- `sources:` lists what the page rests on. Every bullet also carries its own source.
- Dates come from the clock (`date +%F`), never typed from memory. Facts that are only on a branch say so ("on branch X at
  commit Y, not on main"), so the page does not claim more than main has.
- The steward log's times are labels the coordinator typed, some duplicated or dated wrongly. Cite an entry by its line,
  `merge-steward.md:<line>` (or a quoted keyword), never by time; lines shift when the file is edited, so re-check them at
  each verification. The lint rejects a clock time next to a steward-log citation.
- Binding words are not used in a wiki sentence. A page says "rule 54 covers this" and links the rule.

## Operations

- **Ingest** (after a merged batch, an incident, a decision of the user): collect the "Wiki notes:" of the agents'
  hand-offs and the sources; for each affected page change the claim, set its source, set `verified`; update `index.md`;
  append a line to `log.md` (`wiki log ingest "page | subject"` writes `## [YYYY-MM-DD] ingest | page | subject` with the
  clock's date); run `wiki lint`; commit `docs(wiki): ...` as its own commit.
- **Query**: read `index.md`, then the pages it points to, and answer with the sources of the page. If the answer was
  missing or wrong, leave a wiki note for the next ingest.
- **Lint**: `wiki lint` (the [wiki tool](../../tools/wiki/README.md), installed with `cargo install --path tools/wiki --locked`; settings in `wiki-lint.json`; `wiki new <slug> --title ...` creates a page and its index line) (links resolve and their anchors exist; no clock-time citation of the steward log; every page is in the index; no orphan; every
  page has `verified:` and `sources:`; every bullet names a source; pages with an old `verified:` date draw a warning; no run of 8 or more
  words is shared with any rules source: `docs/rules.md`, `origin/main`, the open rule branches and engineering-rules
  `RULES.md`, given with `--rules`). A second, reading pass by a reviewer checks pages against the rules and ADRs they
  cite, a few pages at a time.

## One writer

Only the coordinator or the merge steward edits `docs/wiki/`. Every other agent ends its hand-off with a `Wiki notes:`
section (facts that changed, each with its source) and does not touch this folder. This keeps `index.md` and `log.md` free
of merge conflicts and the pages under one editor.

## Pages

See [index.md](index.md); the optional evaluation is [eval.md](eval.md).
