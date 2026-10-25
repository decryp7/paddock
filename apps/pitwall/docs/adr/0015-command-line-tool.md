# 15. The `pitwall` command-line tool

Status: accepted. The number is provisional; it is allocated again when this branch merges (see `docs/suite-lessons.md`).

## Context

Scripts, CI jobs and headless machines need to do what the window does: see what is running, load and unload models,
download a model, make a key for a script, try a chat. Using `curl` against `/mgmt/v1/rpc/...` with the admin key works but
is awkward and easy to get wrong (a key pasted into shell history, no progress, no exit codes). The roadmap approved a small
CLI that adds no server surface.

## Decision

- **A separate crate, `pitwall-cli`, binary `pitwall`.** It depends on `pitwall-core` (the DTOs, so the CLI and the UI read
  the same contract; the discovery file parser) and `pitwall-loopback`. It does not depend on `pitwall-server`, so it cannot
  reach the one outbound client (`egress.rs`) or the reqwest dependency; `scripts/ci/privacy-check.sh` still passes.
- **The only transport is `pitwall_loopback::Client`**, whose `LoopbackPort` can only mean 127.0.0.1. The clippy rule that bans
  building an HTTP client elsewhere is therefore satisfied by construction, and the CLI cannot be pointed at another machine
  (there is no `--host`).
- **No new server surface.** Management commands call the existing `POST /mgmt/v1/rpc/{method}` methods with the admin key;
  `chat` calls the standard `/v1/chat/completions` with a per-app key, so a script's key keeps the app's permissions and
  limits and the request shows up in Requests and Usage like any other app's. `pull` asks the server to download (host shown,
  offline mode honoured, resumable) and polls `downloads` for progress.
- **Keys never travel on the command line.** The admin key comes from `--key-file`, `PITWALL_ADMIN_KEY`, the OS credential
  store (the entry the window keeps) or `admin.key`, in that order; the app key from `PITWALL_API_KEY`. `--key` is refused
  with an explanation. Keys are never printed except a just-created secret, once, alone on stdout.
- **Hand-rolled parsing, no clap.** The grammar is small and the exit code for a bad command line must be 2 with a message that
  says what to fix; a parser of about 200 lines that is a pure function is easier to keep exact than configuring a framework,
  and it adds no dependency. (`keyring` is the only new dependency, already used by the app.)
- **Stable exit codes**: 0 ok, 1 error, 2 usage, 3 server not running (with the next step), 4 refused (insufficient memory,
  offline mode, busy, conflict, a limit). The mapping from `mgmt::ErrorCode` is a pure function with a test per code.
- **Pure core, thin edge.** `args`, `exit`, `format`, `sse` and the decisions in `discover` are pure and unit tested (above
  96 % of lines); `api` and `run` are the I/O edge, tested by `tests/cli.rs` which runs the built binary against a fake local
  server on an ephemeral port.

## Consequences

- The CLI and the server must be the same version; a DTO mismatch is reported as such ("update pitwall and Pitwall").
- Packaging is a follow-up: shipping `pitwall` in the bundles needs a second sidecar (`externalBin`, built by
  `scripts/sidecar.mjs`) and a `PATH` entry or symlink per OS. Until then `cargo build -p pitwall-cli --profile dist` builds it.
- Aliases from the presets work in `chat` unchanged; `load`, `unload` and `bench` take model ids because the management calls
  do.
