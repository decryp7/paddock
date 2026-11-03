# 48. Test in the user's launch context: each app declares its launch contexts, and the VM smoke runs each on a fresh account

- Status: **Draft (WIP, paused 2026-11-03 for Dyno 1.1.0)**; to become Proposed. Approved for drafting by the user
  (2026-11-03, item 10). Proposes rule **66**.
- Related: ADR 0037 (app composition contract), ADR 0039 §5 (release-critical behaviour gets a probe), ADR 0040 (Windows
  GUI test session); this extends them and must not duplicate them. `apps/<app>/vm-smoke.json`, `scripts/vm-smoke`.

## Evidence (read 2026-11-03)

| Instance | Source |
|---|---|
| The macOS agent saw 0 of 20 sockets only as a child of the LaunchServices-started app; from a shell or launchd it saw all | Dyno ADR 0026 amendment, `6393dfaf0` and `40cab7c06` on `fix/dyno-agent-via-launchd-2` |
| The Windows console window appeared only when Explorer started the agent at logon | `98703a274`, `8c27d153d` on `fix/dyno-windows-shell` |
| `sfltool dumpbtm` worked on the dev Mac only because the session held `system.privilege.admin`; in a fresh VM it hung behind a SecurityAgent dialog | `17bae21b1` on `fix/dyno-disk-startup` |
| Windows logon started only the agent after the first app start's repair (Run value collision, ADR 0045) | `8c27d153d` |

## Decision (outline, to be written)

1. Launch contexts declared per app (in `vm-smoke.json` or the ADR 0037 contract): Finder/Dock or Start menu, login item
   / Run entry after a restart, after an update, CLI from a terminal, elevated and not, a second instance.
2. The VM smoke runs each declared context on a fresh user account, from the artifact, never on the dev machine's state
   (held authorizations, existing files, approved sessions).
3. Conformance check (provisional **C76**): an app with a release workflow that lacks a declared context for a kind it
   uses (a Run entry, a login item, a CLI) fails.

## Still to do

Work packages with agent-days, G.7 placement, decisions (manifest location; fresh account per context vs per run; which
contexts are mandatory per OS).
