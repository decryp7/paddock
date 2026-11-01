# CI artifacts: v3 to v4 (2026-11-01)

Gitea 28.2.0 (`routers/api/v1/repo/action.go`) lists only artifacts whose `content_encoding` contains `/`, which means
v4 artifacts. v3 uploads were invisible to the REST API, so tooling such as the VM harness could not fetch them.

**Chosen:** `ChristopherHX/gitea-upload-artifact` and `ChristopherHX/gitea-download-artifact`, branch `v4` (no tags exist),
pinned by digest (upload `81f940d0...`, download `75635f32...`, both "disable ghes no support check" on 2024-01-22).
The upstream `actions/*-artifact@v4` refuse a Gitea server with `GHESNotSupportedError`; the maintainer-suggested forks work
(go-gitea/gitea#36973, read 2026-11-01: the `@v4` fork worked, `@v6` failed with "unauthorized"). The forks are upstream
v4.0.0 with that check removed, so `merge-multiple`, `pattern`, `overwrite` and `compression-level` exist.
Renovate keeps the digest current (it follows the `v4` branch).

**v3 to v4 differences that matter here**
- An artifact is immutable and its name must be unique per run (a re-run attempt that uploads the same name fails; pass
  `overwrite: true` where that becomes a problem). All names in our workflows are already unique per job (OS, shard, target).
- No merging of uploads into one name.
- `download-artifact` without `name` still extracts each artifact into `<path>/<name>/`, the v3 layout, so the release jobs'
  `find artifacts -type f` is unchanged.
- No `retention-days` is set anywhere, so the server default applies as before.
