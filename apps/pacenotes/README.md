<div align="center">

<img src="docs/images/icon-512.png" alt="Pacenotes icon (draft)" width="128">

# Pacenotes

**Understands your code: navigate, review, diagram and trace it, and serve it to local AI over MCP.**

Pacenotes reads a repository without ever running it, builds an index you can search and ask questions of, and gives
your local AI the same view over MCP. Everything runs on your machine.

</div>

> **Status: in design; implementation starts after Debrief's release.** There is no product code in this repository.
> The design is in [docs/proposal.md](docs/proposal.md) and open questions are pending.

## What it will do

Pacenotes is a small core (code index, headless service, MCP server, UI shell) with its functions as plugins:

- **Understand.** Symbols, references, call graphs, a token-budgeted repo map, search, stack-trace and log-site
  resolution, ownership and hotspots.
- **Review.** Rule packs plus an optional local-AI pass over a diff, with suppressions and SARIF output.
- **Diagrams.** C4, module, class, call-graph and sequence diagrams as draw.io files that keep your hand edits when
  regenerated.
- **Requirements.** Import requirements from Word, Excel, CSV and Markdown (later PowerPoint, PDF, ReqIF), link them to
  code and tests, and see what is untraced, uncovered or suspect.
- **Tests & coverage.** Coverage import, patch coverage, test inventory and the untested hotspots that matter.
- **Debrief bridge.** Debrief asks "does this log line, exception or config key exist in the code?" and gets a verdict
  with evidence.
- **Language packs.** Grammars and query packs per language, signed and tested.

It never executes repository code, is read-only towards your repositories, redacts secrets from what it returns, and
sends nothing off the machine.

## How it fits the family

Pacenotes is the seventh app of the Paddock family. [Pitwall](https://dev.decryptology.net/decryp7/pitwall) is the
local AI that calls Pacenotes over MCP (and is optional: every function works without it),
[Debrief](https://dev.decryptology.net/decryp7/debrief) checks stack traces and log lines against the code,
[Sighter](https://dev.decryptology.net/decryp7/sighter) runs the UI tests whose plans Pacenotes can draft from
requirements, and [Pitcrew](https://dev.decryptology.net/decryp7/pitcrew) runs the headless service in CI. See the
[family overview](https://dev.decryptology.net/decryp7/paddock/src/branch/main/docs/ecosystem.md).

## Documentation

- [Design proposal](docs/proposal.md) (status: proposal, open questions pending)
- [Architecture direction](docs/architecture-direction.md)
- [Lessons from the suite](docs/suite-lessons.md)
- [Engineering rules](AGENTS.md)

## Licence

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
