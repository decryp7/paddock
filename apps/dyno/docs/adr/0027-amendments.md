# ADR 0027 amendments

The ADR file `0027-helper-for-1-1-0.md` is not on this branch; these amendments apply to it and are merged into it when it lands.

## 2026-11-03: the interactive users' rights on the helper's pipe (security review of WP-1a/1b)

Section 1's endpoint row listed `GRGW` for interactive users (`IU`). `GRGW` is `GENERIC_READ | GENERIC_WRITE`, and `GENERIC_WRITE`
on a pipe includes `FILE_APPEND_DATA`, which is `FILE_CREATE_PIPE_INSTANCE`. Any interactive user could therefore create another
instance of the helper's pipe. The client's owner check does not see that (a pipe's owner is its creator, and the instance a
client reaches is chosen by the OS), so a user could serve forged answers to another user.

The row now reads: SYSTEM and Administrators `GA`; interactive users `0x12019b`
(`FILE_GENERIC_READ | FILE_WRITE_DATA | FILE_WRITE_ATTRIBUTES | FILE_WRITE_EA`, no append, so no create-instance right); network logons
denied. `paddock-ipc` applies the same change to `Access::service_for_interactive_users` (Pitcrew's service pipe). Tests:
`interactive_users_are_never_granted_the_right_to_create_pipe_instances` (any OS) and, on Windows, a second `CreateNamedPipeW`
on a running pipe with only those rights is denied.

## 2026-11-04: SMART is read through the helper first, with an unelevated fallback (amends O2; user)

Decision O2 (section 7) said the Windows SMART path always goes through the helper. Dyno 1.1.0 testing found that nothing ever read
SMART through the helper (`HelperLink::smart` had no caller), the helper's `Info` listed no tools (so Settings showed "Not
installed" after a successful install) and the agent looked for `smartctl` only in places the helper never installs to. The user then
decided, amending O2 (**amended 2026-11-04 (user): helper first, unelevated fallback**):

- With the helper on, every disk is read through it (`HelperFirstWear`, `dyno-agent/src/helper_wear.rs`).
- With the helper off, or when it reads nothing, the agent reads on this computer with any smartmontools it finds: Dyno's own copy,
  then `%ProgramFiles%` (and the x86 folder), the helper's folder, the `InstallLocation` of the installer's registry key (both
  registry views, because the upstream installer writes its uninstall key without choosing one), and the `PATH`
  (`dyno_platform::smartctl_search`). It reads the disks Windows allows an ordinary account (NVMe, per WP-0) and names the others
  as needing Administrator readings.
- The seam: `READ_LOCALLY_WHEN_HELPER_OFF` (`helper_wear.rs`). `false` restores "only through the helper".
- The helper's `Info` lists the tools found in `tools\smartmontools-<pin>\bin\smartctl.exe`; the Settings card says which copy
  Dyno uses (the helper's, Dyno's own, or the person's own install). The install folder is defined once
  (`dyno_platform::tool_install::install_folder`/`smartctl_path`) and used by the installer, the tools list and the helper's reader.
- The installer empties its destination folder first, so a file left by an earlier install cannot pass the post-install check.
- The helper's `Smart` reply carries what the agent's own reader makes, so turning the helper on loses nothing and the history
  continues: `SmartDisk` gained `wear_id` (`DiskWear::id`, model and serial; the one place a serial travels, still no WWN),
  `bytes_written` and `rated_bytes`, filled by the same parser (`parse_smartctl_ssd`, rule 55); `SmartDisk::to_wear()` makes the
  `DiskWear`. `HELPER_API` is 4. Test: `the_helper_path_and_the_agents_own_path_make_the_same_wear_reading_with_the_same_id`.

## 2026-11-04 (helper wiring, WP-0): the fallback policy is one seam for every reading

The user's decision D1 (helper first, then this computer, for every reading) is implemented once, in `dyno-agent/src/helper_first.rs`
(rule 55), and Smart is its first instance:

- `HelperFirst<R: HelperReading>`: helper ready, then the helper's answer; else (or when the helper errors, or its answer holds
  nothing useful) the agent's own reader; else an honest needs-admin reason with the next step. It returns `Sourced { value, via }`.
- `ReadyGate`: `HelperCentre::status()` costs two IPC calls (`Info`, `Audit`), so readiness is kept for 5 s and dropped at once when
  a call finds the helper unreachable or a job changes the helper (`HelperCentre::ready`, `forget_ready`).
- `via: ReadVia` (`dyno-core/src/read_via.rs`) on the reply types of the readings; the UI shows it with `dy-reading-source`.
  App-agent protocol 4 is unreleased, so the fields were appended without a bump.
- `Shared.helper` is over `Box<dyn HelperTransport>`, so tests serve a real helper `Dispatcher` (`tests/common/helper_rig.rs`).
- `SmartLink`, `SharedSmart` and `HelperFirstWear::new` are gone: `helper_first_wear(port, local)` builds the Smart instance.
  `READ_LOCALLY_WHEN_HELPER_OFF` is unchanged. The memory modules (Linux) are the second instance.
- D6 (helper `Traffic` scoped to the caller, administrators see all) is WP-TRF's: the seam only carries the helper's answer.
