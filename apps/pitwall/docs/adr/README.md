# Architecture decision records

Numbered, with status, context, decision and consequences. ADRs are not renumbered; a superseded one stays and says so.

| # | Title | Status |
|---|---|---|
| [0001](0001-local-only.md) | Local only: no prompt, response or metric ever leaves the machine | accepted |
| [0002](0002-core-crate-ports-adapters.md) | A pure core crate with thin adapters | accepted |
| [0003](0003-api-surface-and-compatibility.md) | API surface and compatibility policy | accepted |
| [0004](0004-engine-abstraction-and-process-model.md) | Engines: one trait, one process per model | accepted; llama.cpp verified on macOS only; pack and planned adapters superseded by [0020](0020-llama-cpp-is-the-only-engine.md) |
| [0005](0005-queue-and-scheduling.md) | Queue and scheduling | accepted |
| [0006](0006-memory-policy.md) | Memory policy: load on use, release when idle, never touch a busy model | accepted |
| [0007](0007-security-model.md) | Security model: keys, pairing, loopback only | accepted; LAN exposure not built |
| 0008 | not allocated | |
| [0009](0009-app-integration-contract.md) | How apps (Debrief first) find, pair with and use Pitwall | accepted; Debrief side not built |
| [0010](0010-no-plugin-system.md) | No plugin system | accepted |
| [0011](0011-declarative-packs.md) | Extend through declarative packs, not code | accepted; engine packs superseded by [0020](0020-llama-cpp-is-the-only-engine.md), model catalog packs stay |
| [0012](0012-native-feel.md) | Native feel: windows behave like a desktop application | accepted |
| [0013](0013-usage-over-time.md) | Per-app usage over time, and limits that explain themselves | accepted (number provisional) |
| [0014](0014-model-presets-and-aliases.md) | Model presets and aliases | accepted (number provisional) |
| [0015](0015-command-line-tool.md) | The `pitwall` command-line tool | accepted (number provisional) |
| [0016](0016-parallel-slots.md) | Parallel slots per model and what they cost | accepted (number provisional) |
| [0017](0017-model-update-notices.md) | Model update notices: ask, never fetch | accepted (number provisional) |
| [0018](0018-engine-pins-and-capability-gating.md) | Engine version pins, rollback and capability gating | accepted (number provisional) |
| [0019](0019-caching.md) | Caching: prompt-prefix reuse, a response cache and an embeddings cache | accepted (number provisional); not built yet |
| [0020](0020-llama-cpp-is-the-only-engine.md) | llama.cpp is the only engine, and installs itself | accepted; supersedes the engine parts of 0011 and 0004 |
| [0021](0021-what-quitting-does-about-serving.md) | What quitting Pitwall does about serving | accepted |
