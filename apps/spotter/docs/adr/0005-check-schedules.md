# 5. Check schedules: three modes, evaluated in Rust, jiff and croner

- Status: accepted (2026-10-23)

## Context
A watch stored one number, `interval_secs`. Users want "weekdays at 09:00 and 14:00" and cron expressions too, with the
right behaviour across daylight-saving changes and after the machine slept.

## Decision
- **A tagged enum in the watch spec**: `schedule = { mode = "every" | "times" | "cron", ... }` (`spotter_core::schedule::Schedule`),
  mirrored by `Schedule` in `api.types.ts`. `Every` has `secs`; `Times` has `days`, `times` (`HH:MM`) and an optional IANA
  `zone`; `Cron` has `expr` (five fields, or six with a leading seconds field) and an optional `zone`. Config `SCHEMA` 3
  migrates `interval_secs` to `{ mode = "every", secs }`.
- **All maths is pure and in Rust** (`spotter-core`: `schedule.rs`, `calendar.rs`); time and jitter are passed in. The
  scheduler (`spotter-engine`) only asks `next_check(schedule, local zone, now, timing)`. The UI never computes a next run:
  the `preview_schedule` command (a query) returns the description, the next five runs and warnings.
- **Crates**: [`croner`](https://crates.io/crates/croner) (MIT) parses and evaluates cron and writes the English
  description; [`jiff`](https://crates.io/crates/jiff) (MIT or Unlicense) supplies time zones with a bundled tz database
  (`tzdb-bundle-always`), so results do not depend on the OS having zoneinfo (Windows has none) and tests run anywhere.
  `cron` and `chrono-tz` were rejected: no description, a second date library next to `time`, and no per-expression DST rules.
  Both licences are already allowed by `deny.toml`.
- **Local zone**: the shell reports the IANA name (`iana-time-zone`) next to the UTC offset on the clock; without a name the
  fixed offset is used. A schedule can choose its own zone.
- **Minimum interval and jitter**: a source on the network is checked at most once a minute, a folder every 10 seconds
  (`SourceConfig::min_interval_secs`). "Every" is raised to that minimum; a calendar schedule never runs closer than it to the
  last check, so a too-frequent expression is clamped (extra runs are skipped, not delayed) and the preview warns. "Every"
  keeps its +-10 % jitter and exponential backoff; calendar runs get up to 30 s of positive jitter so watches on one host
  do not fire together.
- **Catch-up**: the next check is always computed after "now", so a machine that slept through several runs does one
  check on wake, not a burst.
- **Daylight saving**: a time inside the spring-forward gap runs once (at the shifted wall time for "at times", at the first
  instant after the gap for cron); a time that happens twice on fall-back runs once, the first time.
- Quiet hours and the digest apply to notifications, never to checks; "Check now" is unaffected.

## Consequences
Two small dependencies (croner and jiff, with derive_builder and the tz data). Hand-written cron semantics are avoided. The
cron description is English only. Old configs are migrated on load and rewritten as schema 3.
