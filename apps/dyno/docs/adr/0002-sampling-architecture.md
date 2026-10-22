# 2. Sampling architecture: a per-user agent samples, the app subscribes to pushed updates, Dyno stays near 1 % CPU

- Status: accepted (2026-10-23)
- Related: ADR 0001 (ports and adapters), ADR 0008 (storage), `dyno_core::sampling`, `dyno_agent`, `dyno_ipc`

## Context

Dyno shows live numbers and keeps days of history, so something must measure while the window is closed. A monitor
that costs noticeable CPU defeats its purpose: the user explicitly wants Dyno's own overhead measured and shown, with
a budget of about 1 % of one machine's CPU. The UI is a webview; it must never poll or do the measuring.

## Decision

1. **A separate agent process (`dyno-agent`) does all measuring.** It runs per user, outside the app: started at login
   by the OS service mechanism of ADR 0010 (launch agent / Windows service / systemd user unit), or by the app as a
   detached process when no service is installed. It owns the sampling loop, the SQLite store (ADR 0008), the rules
   engine and the process-control state. Closing the window therefore never stops history.
2. **Sensors and process table sit behind the `dyno_core::ports` traits.** The loop calls `SystemSensors::sample`
   every system interval and `ProcessSource::processes` every process interval; slow facts (`FactsSource`) run every
   few minutes on a worker, never on the sampling path.
3. **Adaptive intervals (pure, `dyno_core::sampling::intervals`).** Someone watching (a live subscriber) gets the
   configured 1 s / 2 s; nobody watching gets 2 s / 10 s (3 s when rules are active); battery doubles them; and when
   the measured own CPU goes over budget the intervals stretch in proportion to the overshoot.
4. **Own overhead is measured, not assumed.** Every tick the agent reads its own CPU time and resident memory
   (`ProcessSource` for its own pid) and publishes `Overhead { agent_cpu, agent_memory, intervals }` inside every
   `LiveUpdate`. The UI shows "Dyno: 0.4 % CPU · 38 MB" permanently. The measurement on release builds is a CI
   artefact (`dyno-agent bench`), recorded in docs/roadmap.md next to the load average at the time. Dyno reports its
   CPU as a **share of one core** (as a process list does), not of the whole machine: `Sampler` multiplies the
   process's share of the machine by the core count before publishing `Overhead::agent_cpu` and before comparing it
   with `sampling::CPU_BUDGET` (1.0). 2 % of one core on a 14-core machine is about 0.14 % of the machine. The goal
   stays "at most about 1 % of the machine"; the code's budget is the stricter 1 % of one core on machines with
   several cores. (The doc comments on `Overhead::agent_cpu` and `Load::own_cpu` still say "percent of the machine"
   and should be corrected by the owner of `dyno-core`.)
5. **Pushed, not polled.** The app connects over `dyno-ipc` (private local endpoint, per-launch token), sends
   `Subscribe { detail }` and receives `Push(LiveUpdate)` frames; `detail: Processes` adds the full process list and is
   requested only while the task manager is open. Calls (history, details, actions) are request/reply with timeouts.
   Unsent pushes are dropped when a subscriber is slow (bounded queue), never buffered without limit.
6. **Discovery.** The agent writes `agent.json` (endpoint name, token, pid, start time, protocol version) with owner-only
   permissions into the user's data directory and removes it on exit. The app reads it, connects, and treats a stale
   file (dead pid, refused connection) as "agent not running". A newer app than agent (protocol or build mismatch)
   asks the agent to restart (`Shutdown`); the service manager brings it back.
7. **Failure containment.** A panic in one tick is caught (`guard::catch`-style), counted, and the loop continues; a
   failing sensor becomes an `Unavailable` reading, never an error for the whole sample.

## Consequences

- The agent is the single writer of the store and the single owner of process-control state; the app is a client.
- Two code paths exist for "no service installed" (app-started agent) and "service installed" (OS-started); both end
  in the same discovery file and the same protocol.
- Overhead numbers depend on the machine; the budget is enforced by the adaptive intervals, and reported honestly.
