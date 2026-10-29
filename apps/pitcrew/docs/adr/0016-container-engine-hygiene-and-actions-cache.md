# 16. Container engine hygiene and the Actions cache

- Status: accepted (2026-10-23)
- Amended by [ADR 0022](0022-host-environment-and-shared-folders.md) (accepted 2026-10-29): cache trimming takes the maintenance lock and runs "when idle" instead of refusing while a job runs.
- Related: ADR 0006 (housekeeping); code: `pitcrew_core::hygiene`, `pitcrew_service::{hygiene, supervisor::ops}`,
  `app/src/app/features/storage/`

## Context

After a runner's `work/` folder, the biggest hidden disk eater on a Gitea or Forgejo runner machine is the container
engine (images up to about 20 GB, build cache, stopped containers, volumes) and the caches under `$HOME/.cache`
(`act` for action checkouts, `actcache` for the runner's built-in Actions cache server). Cleaning the runner folder
(ADR 0006) does not touch them.

## Decision

**Report first, remove only on request.** `Request::EngineReport` (a query) runs `docker system df` (or `podman`, found
on the login-shell `PATH` the service already uses for runners, never a fixed path) and measures the two cache folders
without following links, within file-count and time limits. A missing engine or a stopped daemon is a report with a next
step, not an error.

**Prune = preview, confirm, prune.** The owner picks the kinds (stopped containers, dangling images, unused images,
build cache, volumes; volumes off by default) and an age in days. `EnginePrunePreview` lists what each kind would
remove from the engine's own listings (`images`, `ps -a`, `volume ls`, the `system df` reclaimable figure): count, size
where the engine says it, a few names. Only then can the UI offer "Prune...", behind a confirmation that names what goes
and how much; any change of the selection invalidates the preview. `EnginePrune` runs the engine's own prune commands
(`container|image|builder|volume prune -f [--filter until=...]`), each with a timeout, and reports the space the engine
says it reclaimed and any failures. Nothing is pruned while any job runs or an update is under way (`Busy`), because an
image pulled for a job that has not started its container yet looks unused.

**The Actions cache.** The runner's built-in cache server stores in `cache.dir` of its `config.yaml` (default
`$HOME/.cache/actcache`) and has its own `cache.size_limit`. Pitcrew does not run a second cache server: it reports the
runner's (folder, size, entries, the biggest ones, over its limit), lets the owner switch it on or off in the
Configuration tab (the existing typed form), and trims it to limits the owner sets (a size and/or an age): old entries
first, then the oldest until the size holds, again with a preview first and refusal while a job runs. Safety: the
folder must be absolute and not a root or the home folder itself; entry names are plain names inside the folder; links
are never followed.

## Consequences

- Dry runs are as exact as the engine's listings allow. Docker does not offer a real dry run for build cache and
  volumes, so those show the reclaimable size or a count.
- Podman has no `builder prune`; that kind reports an error for Podman rather than guessing.
- An external shared cache server (a container image run by Pitcrew) is not part of v1.
