# 14. The local API, the `dyno` CLI and integrations: one versioned, local-only, authenticated protocol

- Status: accepted (2026-10-22); the CLI (item 4) is built: `crates/dyno-cli`, see docs/cli.md; the HTTP API and the integrations are not
- Related: ADR 0001, 0002 (`agent.json` discovery, per-launch token), `dyno_core::protocol`, docs/plugins.md

## Context

People want `dyno status --json` in scripts, other apps of the suite want machine health, and a later optional HTTP API was
planned. Three separate interfaces would triple the attack surface and the versioning work.

## Decision

1. **One protocol.** The `dyno` CLI, the app and every local integration speak `dyno_core::protocol` over `dyno-ipc`, found
   through the same `agent.json` (endpoint, token, protocol version), authenticated with the same per-launch token and peer-pid
   check. There is no second wire format.
2. **Versioned.** `PROTOCOL_VERSION` gates every connection; a CLI of another version gets a clear message and, for the same
   major, read-only requests keep working. Additions are optional fields.
3. **Local only.** Unix socket or named pipe, owner-only discovery file. The optional HTTP API (roadmap, off by default) is a
   thin loopback adapter over the same requests with a key in the OS credential store; it adds no capability the protocol
   does not have.
4. **Read-only by default.** CLI commands in v1: `dyno status [--json]`, `dyno why-slow`, `dyno top`, `dyno alarms`, `dyno
   snapshot`. Acting commands (end, cap, undo) need an explicit flag and pass through the same validation, protected list and
   activity log as a click; there is no way to bypass them.
5. **Integrations** use the same door and only on the user's click or opt-in: Open in Debrief (a time-window export), Explain this
   via Pitwall (redacted summary), Spotter reads the installed-apps inventory to watch for updates, Pitcrew shows Dyno's machine
   health when both run. Each integration declares what it reads; none gets write access.
6. **Output is data, not markup.** JSON for `--json`; human tables for a terminal, with no file contents or command lines unless
   the user asks for them.

## Consequences

- One request needs one handler, one DTO and one test, and works for every client.
- The CLI binary is tiny and shares `dyno_ipc` and `dyno_core::protocol` only; it never links the platform adapters.
