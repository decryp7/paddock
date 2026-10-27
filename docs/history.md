# Imported repositories

Each app came into the monorepo with its full history (ADR 0014 §2): paths rewritten below `apps/<app>/`, tags renamed to
`<app>-vX.Y.Z`. Commit hashes changed; the old hashes stay valid in the old repository, which is a frozen pointer after
the import (§3). To find an old commit here, take its subject from the old repository and search `git log --grep`, or
start from the import merge, whose body names the old repository's last commit.

| App | Old repository | Old `main` at the import | Import merge | Tags |
|---|---|---|---|---|
| Debrief | [decryp7/debrief](https://dev.decryptology.net/decryp7/debrief) | `e42bb85e`, plus `test/x50-shell-baseline` (`8671f1be`) and `perf/test-speed` (old `37efa6a0`, rebased) | "chore(debrief): import history into apps/debrief" | `debrief-v1.0.0`, `debrief-v1.0.1`, `debrief-v1.1.1` |
| Dyno | [decryp7/dyno](https://dev.decryptology.net/decryp7/dyno) | `6ac980b` (after merging `fix/agent-after-update`, `fix/hardened-runtime-off`) | "chore(dyno): import history into apps/dyno" | none |
| Pacenotes | [decryp7/pacenotes](https://dev.decryptology.net/decryp7/pacenotes) | `9c9c772` | "chore(pacenotes): import history into apps/pacenotes" | none |
| Pitcrew | [decryp7/Pitcrew](https://dev.decryptology.net/decryp7/Pitcrew) | `c82ec05` (after merging `fix/hardened-runtime-off`) | "chore(pitcrew): import history into apps/pitcrew" | `pitcrew-v1.0.0`, `pitcrew-v1.0.1` |
| Pitwall | [decryp7/pitwall](https://dev.decryptology.net/decryp7/pitwall) | `d13c06f` (after merging `feat/debrief-client` with `fix/pre-shell` and `fix/hardened-runtime-off`) | "chore(pitwall): import history into apps/pitwall" | `pitwall-v1.0.0` to `pitwall-v1.0.3` |
| Sighter | [decryp7/sighter](https://dev.decryptology.net/decryp7/sighter) | `4ead821` | "chore(sighter): import history into apps/sighter" | none |
| Spotter | [decryp7/spotter](https://dev.decryptology.net/decryp7/spotter) | `773c194` (after merging `fix/monotonic-found-count`, `fix/hardened-runtime-off`) | "chore(spotter): import history into apps/spotter" | none |
