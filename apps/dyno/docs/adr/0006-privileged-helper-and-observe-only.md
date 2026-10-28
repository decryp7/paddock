# 6. Observe-only by default; system changes only through a privileged helper installed on first use

- Status: accepted (2026-10-23); its write parts (points 1 to 3 and 6) are superseded by ADR 0021 (2026-10-28); the helper is read-only and is not in 1.0 (ADR 0021 point 4, ADR 0022)
- Related: ADR 0004 (CPU taming), ADR 0005 (cleanup safety), `dyno_core::helper`, `crates/dyno-helper`

## Context

The user first wanted Dyno to be "a readonly tool", then decided it must also tame and clean, as long as nothing
happens behind their back. Dyno also reads untrusted data (process names, command lines, files) and renders it, and
the app is a webview, so nothing in it may run with elevated rights.

## Decision

1. **Observe-only is the default posture.** After install Dyno measures and explains. CPU-taming rules ship disabled,
   scheduled cleanups run only categories the user enabled, diagnosis and health only *suggest*. A change to the
   machine happens only (a) from a rule the user enabled, or (b) from an explicit click. Settings states this plainly.
2. **Everything Dyno does to processes or files is logged and undoable** (`dyno_core::activity`): rule actions release
   when the rule stops holding, when the app exits and when the agent stops; deletes go to Trash/Recycle Bin or a
   7-day quarantine; every entry carries its undo record.
3. **The app and the agent never run elevated.** Changes that need administrator rights (power scheme, Defender
   exclusions, machine-wide startup entries, indexing, hibernation…) go through `dyno-helper`, a separate minimal
   executable with a **closed set of `HelperAction`s**: each has an exact inverse, is validated in the core *and again
   in the helper*, is shown as preview lines before it runs, and is recorded with its inverse.
4. **The helper is installed on first use, never at install time.** The first time the user approves an action that
   needs it, Dyno asks the OS for the rights with the OS's own prompt (UAC `runas`, macOS `SMAppService` daemon
   approval / authorisation prompt, Linux polkit `pkexec`), installs the helper, and runs the action. Declining leaves
   Dyno fully working in observe mode. An uninstall action removes it again. Nothing about Dyno needs the helper to be
   present to start.
5. **The helper trusts nobody.** It listens on a private endpoint with a per-launch token and peer-pid check (the same
   `dyno-ipc` as ADR 0002), accepts only `HelperAction` frames, canonicalises paths, refuses system roots, and exits
   when idle. It never evaluates strings as commands.
6. **The protected list is absolute** (`dyno_core::protect`): critical OS processes, antivirus, the audio engine and
   Dyno itself are never touched, by a rule or by a click; rules never touch the foreground app.

## Consequences

- Every feature that wants to change something must express it as a rule action, a cleanup item or a `HelperAction`
  with an inverse; "just run a command" is not available to features.
- The first admin action has one extra step (the install prompt); that is deliberate and explained in the dialog.
- Tests of changes use dummy processes, temp folders and fake launchers; never the developer's real settings.
