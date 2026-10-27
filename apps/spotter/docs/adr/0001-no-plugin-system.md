# 1. No plugin system: built-in sources and declarative packs

- Status: accepted (2026-10-22)

## Context
The first redevelopment of Spotter carried an out-of-process plugin SDK (`spotter-plugin-api`, `spotter-plugin-host`,
example plugins). Sources are few and well understood (folders, listings, JSON, forges, feeds, pages), and everything
a plugin could add is data: where to look, what to select, how to order versions.

## Decision
Delete the plugin crates. Sources are built-in modules behind the `ReleaseSource` port and a catalog of kinds
(`SourceKindSpec` forms in `spotter-core::schema`). Presets (known software) and version schemes are declarative TOML
packs, validated at the edge. Rendered pages go through the renderer (ADR 0002), not a plugin.

## Consequences
Less surface to secure and version; a new kind of source is a code change in the open, a new preset is a TOML file.
