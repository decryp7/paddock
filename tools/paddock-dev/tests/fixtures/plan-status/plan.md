# Fixture plan

A table with a State column (`./dev pr plan-status` tests; invented rows).

| WP | What | State |
|---|---|---|
| 1 | merged PR, row says open | open (`feat/merged`) |
| 2 | merged PR, row says done | done (`feat/done-ok`) |
| 3 | landed label, row says in review | in review `feat/landed-label` |
| 4 | approved PR, row says done | **Done** `feat/approved` |
| 5 | closed unmerged, row says merged | merged `feat/dropped` |
| 6 | commit on main, row says done | done (`aaaaaaa`) |
| 7 | commit on main, row says open | open, see `aaaaaaa` |
| 8 | unknown branch | open `feat/ghost` |
| 9 | commit not on main, row says landed | landed `bbbbbbb` |
| 10 | no reference | open, no reference |
| 11 | a path is not a branch | open `docs/plans/x.md` |

| Branch | State |
|---|---|
| `fix/no-pr` | open |
| `feat/approved` | open: approved |
