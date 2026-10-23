# Diagram generators

Sources for Sighter's design diagrams. They use the Paddock diagram kit (`../../../../paddock/diagrams`, Sighter theme) and
need a Paddock checkout next to this repo.

- `overview.mjs` draws the hero `docs/images/sighter-overview.svg` (`--png` also renders light and dark PNGs with Chrome).
- `c4.mjs`, `seq.mjs`, `state.mjs` write the `.drawio` files in `docs/diagrams/`.
- Export SVG and 2x PNG: `scripts/export-diagrams.sh docs/diagrams docs/images/diagrams` (PNGs are git-ignored).
- After changing an image run `node ../paddock/docs/images/src/version-links.mjs README.md docs/proposal.md` to refresh the `?v=` links.
