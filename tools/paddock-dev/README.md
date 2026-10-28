# paddock-dev (`./dev`)

The developer and CI tool of ADR 0022. Run it with `./dev <command>` (`dev.cmd` on Windows) from anywhere in the repository.

- **Binary cache.** `./dev` names the binary by an input hash (`inputs.txt` plus the compiler cargo uses) and keeps it in
  `$PADDOCK_DEV_HOME/bin/<hash>/` (default `~/.paddock-dev`). **Agents set `PADDOCK_DEV_HOME=/Volumes/d7sst7/dev-targets/paddock-dev-home`
  (never a folder in `~`); CI sets it under `PADDOCK_CI_HOME`. Tests always use a temporary `PADDOCK_DEV_HOME`.**
- **Build folder.** A miss, or uncommitted changes under an input path, builds into `$PADDOCK_DEV_TARGET_DIR`
  (default `tools/paddock-dev/target/`, ignored by git). Agents point it outside `~`, with `CARGO_BUILD_JOBS=2`, through `heavy.sh`.
- **Release jobs** set `PADDOCK_DEV_FRESH=1`: build from source in a fresh folder, no compiler wrapper.
- Commands today: `self hash`, `ci slot acquire|release|status`, `graph affected`. `--json` writes one object; exit codes are
  `paddock-cli`'s (2 usage, 4 refused, ...).
- When a path dependency in `crates/` changes its dependencies, regenerate this lockfile in the same change.
