# 24. Local days, hours and minutes come from a time-zone port

- Status: accepted (2026-10-30)
- Related: ADR 0013 (quiet hours, "until tomorrow"), 0017 (the daily stability score, crash days), 0018 (learned baselines),
  0020 (maintenance windows, backup checks)

## Context

The engine kept the user's offset from UTC as a setting, `AlarmConfig::utc_offset_minutes`, defaulting to 0. Nothing in the
product ever filled it in: the UI wrote it only when the settings were missing, and the agent never asked the OS. So the real
agent computed every local day in UTC, while the UI grouped the Issues table by the browser's local day. In UTC+8, "Today, out
of 100" covered 08:00 to 08:00, quiet hours and maintenance windows were eight hours off, "until tomorrow" ended at 08:00, a
baseline phrase named the wrong part of the day, and a Time Machine folder name (a local wall time) was read eight hours late.
A stored offset is also wrong for half the year wherever daylight saving applies. The CI failure of the Issues e2e test on
2026-10-30 (run 7154, a test bug fixed in d2410901b) led to the audit.

## Decision

1. **A port, not a setting.** `dyno_core::time::LocalZone` answers the offset in force at an instant
   (`offset_minutes(at)`); the core never reads the machine's zone. `dyno_platform::zone::SystemZone` reads the OS zone through
   `jiff` (the IANA database on macOS and Linux, the Windows zone mapped to IANA with jiff's bundled database), at every
   lookup, so daylight saving and a zone changed while the agent runs are both followed. `FixedZone` and `TzZone` (POSIX rules,
   no database) serve tests. The platform carries the zone (`Platform::zone`, UTC in the fake); the agent keeps it in `Core`.
2. **One set of day functions.** `time::{to_local, from_local, day_start, next_day_start, previous_day_start, local_minute,
   local_epoch_day}` are the only way to a local day boundary, minute or weekday. Days are walked midnight to midnight, so a day
   is 23 or 25 hours long when the clocks change; offsets are clamped to UTC±14.
3. **Every boundary goes through them:** the daily scores, window and trend of the Issues page and the history's crash days
   (`issues::stats`); quiet hours and "until tomorrow" (`AlarmEngine` holds the zone); maintenance windows; the weekday and hour
   slots a baseline learns and the words it judges with; Time Machine folder names and Borg's zoneless times
   (`backup::parse_rfc3339`). Times that carry their own offset (restic, the Windows event log, `wbadmin`) stay as they are, and
   the inventory report keeps labelling its dates UTC.
4. **The setting is gone.** `utcOffsetMinutes` is removed from the settings and the UI (no compatibility before v1; an old
   settings file's key is ignored).

## Consequences

- Each of those features has an agent test in UTC+8 (`crates/dyno-agent/tests/local_day.rs`) that fails when the agent reads UTC.
- A wall time that occurs twice when the clocks go back maps to one of the two instants; one that does not occur maps to the jump.
- The UI still formats times with the browser's zone, which is the same OS zone on the same machine.
