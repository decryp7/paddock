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
- Spend CI only on what a change needs (rule 50): one run per branch head, none while review fixes are pending or for a
  branch already merged elsewhere, none for docs-only changes; branches run the affected plan, full gates once on main.
- Slow means two questions (rule 51): can more threads or workers run it (bounded by the kind of work), and can it be made
  cheaper (less work per item, no repeats, a cheaper algorithm or format, caching, batching, skipping)? Measure both;
  thread counts are Settings with an "Auto" default; a long or bulk operation's design has a "Parallelism and cost" section.
- Work a person waits for never runs at an OS's starvable background level (rule 52: macOS QoS `BACKGROUND` and
  `PRIO_DARWIN_BG`, Windows background modes and idle/lowest priorities, Linux `SCHED_IDLE` and idle I/O; for a service,
  launchd `ProcessType` `Background` (or `Adaptive` without XPC), `LowPriorityIO` and systemd `IOSchedulingClass=idle`):
  lowered work uses `QOS_CLASS_UTILITY`, nice 10 or `THREAD_PRIORITY_BELOW_NORMAL`; a starvable level only for work
  nobody waits on, with a `conformance: background-qos — <why>` comment (C54).
