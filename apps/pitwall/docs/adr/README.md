# Architecture decision records

Numbered, with status, context, decision and consequences. ADRs are not renumbered; a superseded one stays and says so.

| # | Title | Status |
|---|---|---|
| [0001](0001-local-only.md) | Local only: no prompt, response or metric ever leaves the machine | accepted |
| [0002](0002-core-crate-ports-adapters.md) | A pure core crate with thin adapters | accepted |
| [0003](0003-api-surface-and-compatibility.md) | API surface and compatibility policy | accepted |
| [0004](0004-engine-abstraction-and-process-model.md) | Engines: one trait, one process per model | accepted; llama.cpp verified on macOS only |
| [0005](0005-queue-and-scheduling.md) | Queue and scheduling | accepted |
| [0006](0006-memory-policy.md) | Memory policy: load on use, release when idle, never touch a busy model | accepted |
| [0007](0007-security-model.md) | Security model: keys, pairing, loopback only | accepted; LAN exposure not built |
| 0008 | not allocated | |
| [0009](0009-app-integration-contract.md) | How apps (Debrief first) find, pair with and use Pitwall | accepted; Debrief side not built |
| [0010](0010-no-plugin-system.md) | No plugin system | accepted |
| [0011](0011-declarative-packs.md) | Extend through declarative packs, not code | accepted |
| 0012 | Native feel (after Debrief ADR 0028), written by another agent | in progress |
