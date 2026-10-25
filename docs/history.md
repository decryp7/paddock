# Imported repositories

Each app came into the monorepo with its full history (ADR 0014 §2): paths rewritten below `apps/<app>/`, tags renamed to
`<app>-vX.Y.Z`. Commit hashes changed; the old hashes stay valid in the old repository, which is a frozen pointer after
the import (§3). To find an old commit here, take its subject from the old repository and search `git log --grep`, or
start from the import merge, whose body names the old repository's last commit.

| App | Old repository | Old `main` at the import | Import merge | Tags |
|---|---|---|---|---|
| Debrief | [decryp7/debrief](https://dev.decryptology.net/decryp7/debrief) | `e42bb85e`, plus `test/x50-shell-baseline` (`8671f1be`) and `perf/test-speed` (old `37efa6a0`, rebased) | "chore(debrief): import history into apps/debrief" | `debrief-v1.0.0`, `debrief-v1.0.1`, `debrief-v1.1.1` |
