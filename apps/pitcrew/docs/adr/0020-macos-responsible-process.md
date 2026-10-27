# 20. macOS: the runner is its own responsible process

- Status: accepted (2026-10-23)
- Related: ADR 0003 (service model); code: `pitcrew_service::disclaim`, `launch::spawn_child`

## Context

On macOS the privacy system (TCC) attributes access to protected folders (Documents, Desktop, Downloads, iCloud Drive,
removable volumes) to the *responsible process* of the one that touches them, by default the app that started the chain.
Pitcrew's service is started from Pitcrew.app and starts the runner, which starts every CI job. A job that read
`~/Documents` therefore produced "Pitcrew.app would like to access files in your Documents folder", repeatedly, and an
ad-hoc signed app gets a new code identity on every reinstall, so an answer did not stick either.

## Decision

Start the runner with `posix_spawn` and call `responsibility_spawnattrs_setdisclaim(attr, 1)` on the spawn attributes
(the private but long-stable libSystem function Terminal, iTerm2 and VS Code use): the runner, and so its jobs, are
their own responsible processes. The symbol is looked up with `dlsym`; when a macOS does not have it the runner starts the
old way and the service log says so once.

`std::process::Command` has no hook for spawn attributes, so `disclaim.rs` spawns directly and keeps what the std path
gave: a pipe each for output and error (read by the same line readers), `/dev/null` as input, the runner's own process
group (graceful stop and tree kill use the group, unchanged), the working folder, the environment (the service's, then
the login-shell `PATH` and the runner's `env` table), `SIGPIPE` back to its default, an empty signal mask, no inherited
descriptors (`POSIX_SPAWN_CLOEXEC_DEFAULT`). Waiting is `waitpid(WNOHANG)`; the FFI is the one small `unsafe` module on
macOS with a comment on every block.

## Consequences

- Pitcrew.app is no longer named in privacy prompts for job file access. A prompt may still name the runner binary; the
  fix for that is jobs that avoid protected folders, or granting the runner binary access (docs/macos.md).
- Tests: a recording function stands in for the real one to prove the attribute is set with `1`; the real function is
  exercised on the macOS runner; the start without the function is covered too.
- It relies on a private symbol. If Apple removes it the fallback is the previous behaviour, not a failure.
