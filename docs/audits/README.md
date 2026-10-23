# Audits

- [2026-10-23-long-operations.md](2026-10-23-long-operations.md): progress, cancel and parallelism, all apps.
- Per-app gap lists against the [problem catalogue](../problem-catalogue.md), 2026-10-24, read-only, ordered by severity:
  [debrief](debrief.md), [pitcrew](pitcrew.md), [pitwall](pitwall.md), [spotter](spotter.md), [dyno](dyno.md),
  [sighter](sighter.md) (design stage), [pacenotes](pacenotes.md) (design stage).

| App | Critical | High | Medium | Low | Released with a known critical bug |
|---|---|---|---|---|---|
| Debrief | 2 | 4 | 7 | 2 | v1.0.1 (macOS store worker aborts) |
| Pitcrew | 1 | 6 | 8 | 1 | v1.0.1 (service not repaired after an update; fix on main, untagged) |
| Pitwall | 1 | 4 | 7 | 2 | v1.0.3 (same; fix on main, untagged) |
| Spotter | 0 | 2 | 8 | 2 | none (not released) |
| Dyno | 0 | 5 | 10 | 1 | none (not released) |
| Sighter | 0 | 5 | 4 | 1 | none (design) |
| Pacenotes | 0 | 5 | 4 | 1 | none (design) |

Severity: Critical = a released or about-to-be-released build is broken for users; High = a rule is unmet and a known incident
class can recur; Medium = rule unmet, low immediate risk; Low = polish. For the design-stage apps "High" means must be in the
first commit of that area.
