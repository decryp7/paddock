# Release checklist

Releases are cut from a tag in CI, never from a workstation (rule 9). Before pushing a tag:

- [ ] The affected gates are green on the combined result (rule 3) and CI on `main` is green.
- [ ] **Run the local VM update smoke** for each app that has a background service (today the shell demo is its reference):
      `bash tools/vm-smoke.sh macos` and `bash tools/vm-smoke.sh linux` on the Mac mini. It builds N and N+1 and checks that
      the agent runs N+1 after the update (C21, R03). This is the only end-to-end update test: CI has no VMs (user decision,
      2026-10-28), so a release is not tagged without it.
- [ ] Icons, signing keys, the changelog and the version are checked by the release preflight (C03, C06, C07, C08).
- [ ] After the tag's run: inspect the published artifact (installer, `latest.json`, notes), not only the green mark.

## The local VM update smoke

`tools/vm-smoke.sh macos|linux` needs an Apple-silicon Mac, [Tart](https://tart.run) (`brew install cirruslabs/cli/tart`),
the external disk mounted (VM images go to `TART_HOME`, default `/Volumes/d7sst7/ci-tart`, overridable) with 60 GB free,
and for `linux` also Docker or OrbStack (the linux-arm64 builds are made in one throw-away container). It fails at once with
a message naming the missing tool or disk. The VM is cloned from a pinned image per run and always deleted again; the smoke
never touches your session (rule 7). See [tools/README.md](../tools/README.md).

## Tests that need a throwaway OS

Local VMs are for tests that would otherwise change the machine they run on (rule 7): login tests (Login Items, background
services, launch at login) and installs, updates and system settings. `tools/vm-smoke.sh` is the one entry point for them;
the update smoke is its first case. Candidates noted, not moved yet (they are faked or covered by fakes on the host today):

- `apps/dyno`: `app/src-tauri/src/login.rs`, `service_repair.rs`, `crates/dyno-agent/src/service/tests.rs`,
  `crates/dyno-platform/src/linux/startup.rs`, `crates/dyno-helper/src/install.rs` (launch agent, helper install, autostart entries).
- `apps/pitwall`: `app/src-tauri/src/service_install.rs`, `service_repair.rs`, `crates/pitwall-server/src/service/mod.rs`.
- `apps/debrief`: `crates/debrief-agent/src/service/launchd.rs`, `cli.rs` (agent install and start at login).
- `crates/paddock-service`: scenarios over fakes (`scenarios.rs`); the real launchd and systemd behaviour is only the update smoke.
- `apps/shell-demo`: the agent's update smoke (C21) is the first case already running there.
