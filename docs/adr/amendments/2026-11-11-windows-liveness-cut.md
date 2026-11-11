# Cut of the four Windows and liveness amendments of 2026-11-11 (user, 2026-11-11)

- Status: Decided (user, 2026-11-11). Applies to the amendments accepted on 2026-11-11 on
  `origin/docs/adr-amendments-windows-liveness` (head `9dedc8a86`): `0021-windows-verbatim-paths`,
  `0021-windows-env-names`, `0021-lf-checkouts`, `0081-zombies-groups-and-heavy`, and the pointer
  `0087-line-ends-control`. Their files come to `main` with that branch; this note stands beside them and wins where
  they disagree.
- Built: only the shared helpers: `paddock_env::winpath`, `EnvMap`, and `tools/heavy` moved onto `presence`.
- Dropped: the Clippy ban on `canonicalize`, check C109, and the global `eol=lf` switch (and ADR 0087's line-ends
  control that depended on it). A fixture that needs LF gets a per-folder `.gitattributes` line when a test needs it.
- The register rows the amendments name (`docs/adr/register/R-2026-11-11-*.toml`) are not made: the register is gone
  (ADR 0096, simplified the same day).
