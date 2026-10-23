# Architecture decision records

Numbered, immutable once accepted (supersede with a new record rather than rewriting). Code comments cite these numbers,
so the numbering is fixed.

| # | Decision |
| --- | --- |
| [0001](0001-architecture.md) | A pure core, OS adapters behind ports, four kinds of process (app, agent, helper, plugins) |
| [0002](0002-sampling-architecture.md) | A per-user agent samples; the app subscribes to pushed updates; Dyno stays near 1 % CPU |
| [0003](0003-process-grouping.md) | Processes grouped by application |
| [0004](0004-cpu-taming-and-throttling-methods.md) | CPU taming rules with an escalation ladder; per-OS throttling methods; crash recovery |
| [0005](0005-cleanup-safety.md) | Cleanup safety: admission policy, preview, recoverable deletes |
| [0006](0006-privileged-helper-and-observe-only.md) | Observe-only by default; a privileged helper installed on first use |
| [0007](0007-extensibility-three-tiers.md) | Extensibility: built-in core, TOML packs, out-of-process plugins |
| [0008](0008-storage-sqlite-and-retention.md) | SQLite in the agent, raw samples for a day, rollups for months |
| [0009](0009-native-feel.md) | The window behaves like a desktop application (Debrief ADR 0028) |
| [0010](0010-platforms-and-services.md) | Per-user agent on every OS: launch agent, systemd user unit, Windows Run entry |
| [0011](0011-design-system.md) | Design tokens, system fonts, two themes, a colour-vision-checked palette |
| [0012](0012-widgets-and-tray-items.md) | Widgets and tray items are versioned data (editable dashboard, several menu-bar items) |
| [0013](0013-alarms-lifecycle.md) | Alarms: an ISA-18.2 style lifecycle in a pure state machine |
| [0014](0014-local-api-cli-and-integrations.md) | One versioned, local-only, authenticated protocol for the CLI, the API and integrations |
| [0015](0015-benchmarks-network-and-ip-lookup.md) | Benchmarks on click out of process; sockets through a port; IP lookup is a browser hand-off |
| [0016](0016-privileged-helper-install-and-ipc.md) | The privileged helper: a registered system daemon, installed on a click, one authorised user, closed action list |
| [0017](0017-issues-and-the-program-baseline.md) | Issues from the OS logs behind a port; a baseline of programs that explains, never accuses |
| [0018](0018-extras-backup-checks-and-taming-proof.md) | Extras and backup health behind a ToolRunner; the login-shell PATH resolver; taming rules that expire and prove their effect |

New records: copy the shape of an existing one (status, related, context, decision, consequences). ADR numbers are
allocated at merge time; renumber on a clash and fix the code comments that cite them.
