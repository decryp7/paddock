# Amendment to ADR 0098 (2026-11-22): member trials run on the real runners, and no longer rest on ADR 0087

- Status: Accepted (user, 2026-11-16; recorded 2026-11-22). Amends 0098's line "no per-member CI run".
- Supersedes as the basis: `0087-member-gate-other-os-2026-11-16.md`, an amendment to ADR 0087, which was withdrawn (`0087-withdrawn-2026-11-12.md`) and never landed.

A member's other-OS check is a **trial run on the real runners** of each OS (ER-9: a gate that decides on a branch runs on the same kind of
machine as CI). The member gate dispatches it itself through `./dev ci dispatch`, which waits for the train's runner reservation (ER-24:
the train always goes first; a trial that cannot get the runner in its bound reports `not measured`). A run on any other machine counts only
as `not measured`, or against the base's own result there. This is the rule; ADR 0087's VM mirror and its withdrawn text are not its source.
