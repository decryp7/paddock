# Diagram generators

Sources for Sighter's design diagrams. They use the Paddock diagram kit (`../../../../../diagrams`, Sighter theme) at the
monorepo root.

- `overview.mjs` draws the hero `docs/images/sighter-overview.svg` (`--png` also renders light and dark PNGs with Chrome).
- `c4.mjs`, `seq.mjs`, `state.mjs` write the `.drawio` files in `docs/diagrams/`.
- Export SVG and 2x PNG: `scripts/export-diagrams.sh docs/diagrams docs/images/diagrams` (PNGs are git-ignored).
- After changing an image run `node ../../docs/images/src/version-links.mjs README.md docs/proposal.md` (from `apps/sighter`) to refresh the `?v=` links.
