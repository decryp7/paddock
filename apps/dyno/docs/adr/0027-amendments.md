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
