# 0018 Throttling, hardware life forecasts and learned baselines

Status: accepted (2026-10-23); amended by ADR 0021 (2026-10-28) where it describes writes.  Related: 0001, 0002, 0006, 0008, 0013.

## Context

Three features turn what the machine reports about itself into advice: "the CPU is slower than it should be, and here is
why" (WP-23), "this SSD has N years of writes left, replace the battery in about N months" (WP-24) and "is this value
normal for a Tuesday morning?" (WP-25). Each is easy to get wrong in the same way: a number that looks precise but rests on
a guess, an alarm without evidence, or a forecast that a replaced disk or a changed clock quietly distorts. Dyno is
observe-only (0006), its core is pure (0001) and its history is bounded (0008).

## Decision

- **Judging is pure, reading is thin.** `dyno_core::throttle`, `lifespan` and `baseline` are functions over values with time
  passed in; the OS readers sit behind three small ports (`ThrottleSignals`, `DiskWearSource`, `BatteryCapacitySource`) in
  `dyno_core::ports`, implemented in `dyno_platform::hardware`. They only read: `pmset -g therm` on macOS, the kernel's
  thermal-throttle counters and cpufreq limits on Linux, `smartctl -a -j` (smartmontools, read-only, exit status ignored) for
  SSD wear, `ioreg -rn AppleSmartBattery`, `/sys/class/power_supply` and `powercfg /batteryreport /xml` for the battery.
  Everything runs through `dyno_platform::process` with a timeout. Reading SSD wear usually needs administrator rights or an
  installed `smartctl`; until the helper (WP-01) can do it the page says exactly that instead of showing nothing or a guess.
- **Never a guess, always a reason.** An input a machine does not report is `CannotTell` / `Unavailable` with the reason; a
  cause is named only when something measured supports it (the system's own limit, a temperature near the sensor's critical
  value, battery power, a battery-saving mode), otherwise the cause is "unexplained" and says what could not be ruled out.
  Every finding carries a `Confidence` shown in words.
- **Throttling is an episode, not a sample.** A tracker needs the CPU to be busy and slow for 20 s before an episode exists and
  clear for 20 s before it ends; a gap of two minutes or time running backwards closes it. Finished episodes are stored; the
  running one lives in memory. The system's own limit is read at most every 10 s and only while the CPU is busy.
- **Forecasts are ranges on robust slopes.** Wear and capacity are projected with a Theil-Sen slope (median of pairwise
  slopes, quartiles as the range, widened when the history is short), not a least-squares line, so one wrong reading cannot
  move them. They need 14 days and 4 readings (SSD) or 30 days and 4 readings (battery); before that the outcome is
  `TooLittleHistory`. Readings that jump (a counter that goes back, a capacity that rises by 15 points, the cycle count
  dropping) cut the series so a replaced disk or battery starts a new one; readings stamped in the future are ignored.
  Readings are stored at most twice a day for up to three years in `hardware_readings`.
- **Baselines are learned on demand, not stored.** Per metric and per app the history is reduced to one mean per local hour,
  then to a median and MAD per weekday-and-hour, falling back to weekday-or-weekend and then to the hour of day when a slot has
  too few days. A value is unusual only beyond 3.5 spreads *and* beyond a per-metric absolute floor (3 % CPU is never news). A
  new install learns silently for a configurable number of days (default 7); alarms, off by default, need five consecutive
  minutes of "far above usual" at medium confidence or better.
- **Alarms only with evidence.** Throttling raises an alarm for a lasting episode at medium confidence or better; lifespan for
  a part that is past its limit or has under a year left at medium confidence or better; baselines as above. Each can be
  switched off in Settings (`hardwareAlarms`, `baselines`).

## Consequences

- On a Mac without smartmontools the SSD card explains how to get the numbers; on a desktop there is no battery card at all.
- Windows has no OS-reported throttle reason, so its throttling findings rest on the clock, the temperature and the power
  mode and are capped at medium confidence.
- Per-app baselines use the minute rows (kept 14 days by default), so they are less certain than system baselines learned
  from the quarter-hour tier (90 days); the confidence word says so.
- The SSD's rated endurance (TBW) is only used when a reader supplies it; no vendor table is bundled.
