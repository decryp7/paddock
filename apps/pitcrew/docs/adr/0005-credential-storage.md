# 5. Secrets: the OS credential store, owned by the app

- Status: accepted (2026-10-22); amended by [ADR 0021](0021-auto-scale.md) (proposed, decided by the owner
  2026-10-28): the service keeps its own read-only `read:user` token for the Actions queue.
- Related: ADR 0001; code: `app/src-tauri/src/credentials.rs`, `pitcrew_core::protocol::Secret`

## Context

Two secrets appear: the **registration token** the user copies from the server's runner settings, and an optional
**read token** for the Gitea API (job details, outcomes, queue times). AGENTS.md forbids storing tokens in Pitcrew's
files or logs, and the user asked for a kept registration token to live in the OS credential store.

## Decision

- Secrets are stored only in the **OS credential store** through `keyring` 4 (MIT/Apache-2.0): Windows Credential
  Manager, macOS Keychain (Secret Service on Linux). Service names `Pitcrew Gitea API` and `Pitcrew registration`; the
  account is the normalised server URL.
- The **app** stores and reads them, in the logged-on user's context. The Windows service runs as LocalSystem and
  could not read the user's vault anyway; it never needs the read token (the app calls the Gitea API itself).
- The registration token travels once: UI → app command → `Request::Register { token: Secret }` → the runner's
  `register --token …` command line. `Secret` has a redacting `Debug`; the service's process helper never prints
  arguments; `register` output is scrubbed of the token and redacted before it is shown. It is kept in the credential
  store only when the user ticks "remember".
- The UI never gets a token back: it can ask whether one is set, set one, or clear it.

## Consequences

- No secret in `pitcrew.toml`, `state.json`, logs or diagnostics bundles (which are redacted again on export).
- The token is briefly visible on the `register` process's command line to other processes of the machine; the
  runner offers no other non-interactive way.
- Linux needs a Secret Service (GNOME Keyring, KWallet) for these features; without one the UI says the credential
  store is not available.
