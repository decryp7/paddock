# Amendment to ADR 0014 (2026-11-22): RustFS S3 is the default sccache backend

- Status: Accepted (user, 2026-11-21): the family's compile cache defaults to the RustFS S3 server, and falls back to the local disk. Built on `feat/sccache-s3-default` (`83fc64a8b`, `587cc91cc`); recorded here by the ADR audit of 2026-11-22.
- Amends: ADR 0014 §1, the "Amended 2026-11-16" paragraph, whose last sentence says "the user's 2026-10-28 decision against a shared compile-cache backend stands: the default is the local disk, and the S3 setting is an option."

## What changes

1. **The default is the RustFS S3 backend** (the user's server, see `docs/dev-setup.md`), for branch runs, train runs and local builds.
   The user's 2026-10-28 decision against a shared backend is reversed by his decision of 2026-11-21.
2. **The fallback is the local disk.** When the S3 server is unreachable (the probe fails) the build warns once and uses the local sccache
   folder; it never fails and never stalls on the backend.
3. **Release, rule 54 and tag builds stay on the local disk.** They never consume a cache that branch runs can write (ADR 0085, item 1;
   `conformance/ci/kit.sh release-path` selects it). This is unchanged.
4. What stays as in 0014: the folder is passed as `--config build.target-dir=...`, never `CARGO_TARGET_DIR` (sccache 0.19 hashes `CARGO_*`).
