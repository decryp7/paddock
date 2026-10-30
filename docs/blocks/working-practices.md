## Working practices

- Research official sources before choosing libraries or relying on another program's behaviour, and record what you
  found and when in the ADR or doc comment. Prefer well-maintained, permissively licensed dependencies.
- Navigate code with Serena's symbol tools (find symbol, references, overview) when they are available instead of
  reading whole files; it saves context and cost.
- Delegate mechanical or well-specified work (refactors with a precise spec, test scaffolding, docs) to cheaper models.
  Keep architecture and review on the main model.
- Share first (rule 42): check Paddock's crates and packages before writing anything an app copy would duplicate.
- Record every benchmark you quote (rule 48): first as a results block in the app's `docs/performance.md` (shared code:
  `docs/benchmarks.md`), then link it from the ADR, plan, CHANGELOG, commit or PR that quotes the number.
- Specs never wait a fixed number of event-loop turns and never leave a patch behind (rule 49): wait on the condition
  (Debrief's `openDialog()`, once `fix/debrief-import-range-flake` lands), never let an optional chain perform a test's
  action, and patch a prototype only with `vi.spyOn` (restored after each test).
- Spend CI only on what a change needs (rule 50): one run per branch head, none while review fixes are pending or for a
  branch already merged elsewhere, none for docs-only changes; branches run the affected plan, full gates once on main.
- A change that touches an app's release path builds that app's installers before it lands on main (rule 54): the batch
  dispatches `<app>-release.yml` with gates on and the bundle built; passing tests alone is not enough.
- Slow means two questions (rule 51): can more threads or workers run it (bounded by the kind of work), and can it be made
  cheaper (less work per item, no repeats, a cheaper algorithm or format, caching, batching, skipping)? Measure both;
  thread counts are Settings with an "Auto" default; a long or bulk operation's design has a "Parallelism and cost" section.
- Never act on a reusable OS name read earlier (rule 53): hold it (handle, job, listener) or verify its identity (pid plus
  start time, `paddock_proc::ProcessId`) immediately before acting; kills go through `paddock_proc` (Clippy
  `disallowed-methods` and C53 check it, ADR 0033). A discovery file that names a pid or port (Dyno's `agent.json`) is
  only a hint: act on it after a held lock or a live connection proves the process is there, and remove it only if it is
  still the same file (same pid and start time).
- Work a person waits for never runs at an OS's starvable background level (rule 52: macOS QoS `BACKGROUND`,
  `PRIO_DARWIN_BG`, `taskpolicy -b`/`-c background`/`-c maintenance`/`-d throttle` and `IOPOL_THROTTLE`; Windows
  background modes, idle priorities and EcoQoS; Linux `SCHED_IDLE` and idle I/O; for a service, launchd `ProcessType`
  `Background` (or `Adaptive` without XPC) and `LowPriorityIO`, systemd `CPUSchedulingPolicy=idle` and
  `IOSchedulingClass=idle`): bulk work (long, with progress and Cancel: walks, scans, compaction, exports) uses
  `QOS_CLASS_UTILITY`, nice 10 or `THREAD_PRIORITY_BELOW_NORMAL`; live work (a person waits on it within a deadline or
  for a screen to fill: requests, probes, a sampler, a page refresh) may use the first two but stays at the normal level
  on Windows; a thread holding a lock that live work waits on is live, or must not hold the lock across slow work; a
  starvable level only for work nobody waits on, with a `conformance: background-qos — <why>` comment (C54).
