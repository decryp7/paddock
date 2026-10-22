# Pitcrew roadmap

Pitcrew aims to keep CI runners healthy without anyone watching: **run** them, **watch** them, **update** them safely and
**show** what they do, for Gitea's runner first, then Forgejo's and GitHub's. Items are marked *(done)* or *(planned)*;
nothing planned is in the code yet. The remaining engineering follow-ups are at the end. No release is published yet
(v0.1.0); the queue below is finished before one is cut.

## Phase 1: supervise and update *(done)*

- **Service model:** Windows service (LocalSystem, runners in the user's session or session 0), macOS launch agent,
  Linux systemd user service. *(Done; [ADR 0003](adr/0003-service-model.md), [0009](adr/0009-linux-service-and-runner-environment.md).)*
- **Supervisor and watchdog:** restart with back-off, crash-loop protection, the stall rule, stop, restart and drain
  between jobs, adopting a registered runner folder. *(Done; [ADR 0006](adr/0006-supervision-and-housekeeping.md).)*
- **Safe updates:** discovery, verification against two hosts, drain, swap, confirm, automatic roll-back, policies, windows,
  pin and skip. *(Done; [ADR 0002](adr/0002-runner-auto-update.md).)*
- **Housekeeping:** scheduled and pressure-driven cleanup with a dry run. *(Done.)*
- **Local IPC with push events** and a versioned protocol. *(Done; [ADR 0004](adr/0004-ipc-protocol-and-acl.md).)*
- **Credentials in the OS store.** *(Done; [ADR 0005](adr/0005-credential-storage.md).)*

## Phase 2: see everything *(done)*

- Dashboard with runner-scoped CPU and memory, history, jobs per day (per hour for 24 h), uptime, queue wait and an events
  feed; runner pages; Activity; Logs; Updates; Settings; the register wizard; tray and notifications. *(Done;
  [ADR 0007](adr/0007-runner-scoped-metrics.md).)*
- A desktop feel and a design system. *(Done; [ADR 0008](adr/0008-native-feel.md), [0010](adr/0010-design-system.md).)*
- Pitcrew's own signed, opt-in self-update (check and install on the Updates page). *(Done; [releasing.md](releasing.md).)*
- The login shell's `PATH` for runners and `[runners.env]` on macOS and Linux; the service restarts itself when its
  program changes. *(Done.)*

## Phase 3: more runners and platforms *(planned)*

- **Forgejo's runner:** on Windows and macOS a user-provided binary (upstream publishes none); OpenPGP-verified updates on
  Linux, where upstream publishes signed binaries. The core already describes the kind (`RunnerKind::ForgejoRunner`).
- **GitHub's self-hosted runner.**
- **Per-runner `env` on Windows** (the table is validated everywhere but applied on macOS and Linux only).
- **LaunchDaemon mode** for headless Macs and a **systemd system unit** for headless Linux hosts whose jobs need no GUI.
- **One SCM service per runner** under a virtual account on Windows, as an option next to session mode.

## Phase 4: distribution *(planned)*

- Developer ID signing and notarisation of the macOS build; publishing installers and `latest.json` for the first release.
- A Linux bundle check on a real desktop (the `systemctl` calls, the tray and the bundles are verified in a container and
  on CI only).

## Engineering follow-ups

- **Automatic roll-back on a first-job crash:** if the updated runner crashes during its first job within 24 hours, offer
  or perform a roll-back ([ADR 0002](adr/0002-runner-auto-update.md)). *(Planned.)*
- **Compressed downloads:** fetch the `.xz` variant to save bandwidth. *(Planned.)*
- **A Windows job object** so orphaned job processes are counted in a runner's usage
  ([ADR 0007](adr/0007-runner-scoped-metrics.md)). *(Planned.)*
- **Stricter authorisation** on shared build machines (the ACL admits every interactive user; `Request::mutates` marks what
  a stricter rule would cover). *(Planned.)*
- **Verification on a dedicated Windows build machine** end to end. *(Open.)*
- **Property-based and fuzz tests** for the parsers (log lines, checksum files, release bodies, configuration), and
  mutation testing to measure test strength. *(Planned.)*
