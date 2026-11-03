# Rule 52 exceptions

Rule 52 runs bulk work gently (lowered priority that never starves). Dyno has exactly one exception, decided in ADR 0030
(section 2, D1):

| Process | Why | Where it is marked |
|---|---|---|
| `dyno-bench` (the benchmark runner; later `dyno-bench-gpu`) | A benchmark's result *is* the speed of the machine; a lowered thread measures the scheduler (on Apple silicon, likely the efficiency cores). It is work the person started and waits for. | `// rule 52 exception: ADR 0030` in `crates/dyno-agent/src/bench_child.rs`, in `ProcessChild::spawn`, the one place the child is started and deliberately not lowered |

The exception is narrow: only the runners' measuring threads, only after a click, never scheduled. The agent's orchestration thread
(`dyno-bench`, the run's thread) stays at its normal role. In exchange a run is one at a time and Cancel always works (the pre-flight
warning about other programs' CPU use is still to come with the page's work package, B7).

`bench_child::tests::the_rule_52_exception_has_one_marker_and_the_child_is_never_lowered` checks that the marker appears once and
that the file never lowers the child. A process the agent starts at normal priority for bulk work anywhere else is a violation, not
a second exception.
