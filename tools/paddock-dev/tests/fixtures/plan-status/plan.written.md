# Fixture plan

A table with a State column (`./dev pr plan-status` tests; invented rows).

| WP | What | State |
|---|---|---|
| 1 | merged PR, row says open | done (`1234567890`), was `feat/merged` |
| 2 | merged PR, row says done | done (`feat/done-ok`) |
| 3 | landed label, row says in review | done (`feedfaceca`), was `feat/landed-label` |
| 4 | approved PR, row says done | **Done** `feat/approved` |
| 5 | closed unmerged, row says merged | merged `feat/dropped` |
| 6 | commit on main, row says done | done (`aaaaaaa`) |
| 7 | commit on main, row says open | done (`aaaaaaa`) |
| 8 | unknown branch | open `feat/ghost` |
| 9 | commit not on main, row says landed | landed `bbbbbbb` |
| 10 | no reference | open, no reference |
| 11 | a path is not a branch | open `docs/plans/x.md` |
| 12 | commit missing from the clone, row says done | done (`eeeeeee`) |
| 13 | one reference unknown, one landed | done (`fffffff`, `aaaaaaa`) |
| 14 | branch without a PR, head not on main | open `fix/not-landed` |
| 15 | the newest PR decides | open `feat/reopened` |
| 16 | "on main" counts as done | on main since `aaaaaaa` |
| 17 | open PR whose head reached main by a fast-forward | done (`c0ffee1`), was `feat/ff-landed` |

| Branch | State |
|---|---|
| `fix/no-pr` | done (`c0ffee0`) |
| `feat/approved` | open: approved |
