# 13. Alarms: an ISA-18.2 style lifecycle in a pure state machine

- Status: accepted (2026-10-22); core data model in progress (`dyno_core::alarms`)
- Related: ADR 0006 (observe-only), ADR 0008 (storage), `dyno_core::alerts` (the older alert list), custom alerts on the rules
  engine's conditions

## Context

Alerts that only append to a list become noise: the same low-disk message every minute, no way to say "I know", no way to see
which rule chatters. Industrial alarm management (ISA-18.2) solved this with a lifecycle, shelving and statistics. Dyno adopts
the parts that make sense for one PC.

## Decision

1. **States.** `Active` (condition true, not acknowledged), `Acknowledged` (condition still true, the user saw it),
   `Cleared` (condition false, acknowledged) and `ClearedUnacknowledged` (condition false before the user acknowledged).
   Transitions: condition true: new or repeat; acknowledge; condition false: clear. The ISA-18.2 *shelved* and *suppressed by
   design* states map to **shelve** (1 hour, until tomorrow, until reboot, custom) and **mute rule**; *out of service* maps to
   a disabled rule.
2. **A pure state machine in the core** (`alarms`): `step(state, condition, now, user_action) -> (state, effects)`, no clock,
   no I/O. The agent feeds it the rule evaluations each tick and carries out the effects (notify, persist).
3. **Identity and de-duplication.** One alarm per (rule, source); a repeat while active increments `repeat_count` and updates
   `last_occurrence` and `current_value` instead of adding a row. Fields: severity, source and rule, first and last occurrence,
   repeat count, current value, chart link (time window), user notes.
4. **Any metric.** Custom alerts use the same condition model as CPU taming (threshold, duration, context) but their action is
   notification only; they never change the machine.
5. **Views.** Active, History and Statistics (noisiest rules, time to acknowledge, chattering rules with a suggestion to tune or
   retire them). Single and bulk acknowledge, shelve, mute, note, Open in Debrief.
6. **Delivery.** A badge on the tray and dashboard, native notifications with Acknowledge and Snooze actions, quiet hours with a
   critical bypass.
7. **Bounded storage.** SQLite tables with a row cap and an age cap, oldest cleared alarms first; active alarms are never pruned.
   Unreadable rows are skipped with a note.

## Consequences

- The lifecycle is exhaustively unit-tested as a table of (state, event) pairs.
- `dyno_core::alerts` (the flat list) is superseded by alarms once this lands; no compatibility is kept (unreleased).
