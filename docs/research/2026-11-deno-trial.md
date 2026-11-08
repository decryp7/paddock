# Deno 2 for the Angular toolchain: a trial

Research note, 2026-11-08. Question (from the Deno 2.0 announcement, which claims Angular support; Angular's own request is
angular-cli#28884): does Paddock's Angular toolchain run under Deno 2, and is it faster than Node? Background decision: all
tooling moves to Rust; Node stays for Angular, Vitest and Playwright unless this shows a real gain.

## What was tried

App: the **shell-demo** in the `ui` workspace (`ng build shell-demo`), because it is the smallest Angular app that uses the
shared shell and has its own Playwright project. The shell-demo has no unit-test target, so unit tests use `workbench-demo`
(2 tests) and a 32-file, 348-test slice of `paddock-ui` (`shell/{menu,jobs,notify,settings}`). Playwright: the `shell-demo`
project, specs `shell-demo-toasts` and `shell-demo-theme` (14 tests).

Deno 2.9.8 (stable, aarch64-apple-darwin) downloaded from the denoland/deno release, sha256 checked against the published
`.sha256sum`, run from a scratch folder with `DENO_DIR` there; nothing installed system-wide. `npm ci` at the root as usual
(Node 24.22.0, npm 11.19.1). Deno runs the same `node_modules`: `deno run -A ../node_modules/@angular/cli/bin/ng.js ...` and
`deno run -A ../node_modules/@playwright/test/cli.js test ...`. For Playwright the dev server was started once under Node and
reused (`PW_REUSE_SERVER=1`) for both runners, with an untracked config that only empties `webServer`; so the A/B compares
the test runner, not the server.

## Does it work

| Step | Result under Deno |
|---|---|
| `ng build shell-demo` | Works. Output identical to Node: same 89 files, same sha256 over all file hashes (`b3190915...638747`), cold and warm. |
| `deno task shell-demo:build` | Works (package.json scripts); `npm run` and `node` inside a script still start Node, `ng` runs under Deno. |
| `deno run -A npm:@angular/cli build` | Works but resolves the registry each time (+0.9 s); `npm:@angular/cli@x` fails unless `deno install` was run. Use the local bin path. |
| workbench-demo unit tests | 2 of 2 pass, same as Node. |
| paddock-ui `shell/**` (97 files, 1208 tests) | **Fails in two ways**: (1) `shell/menu/src/overrides.spec.ts` ("ignores a prototype key"): Deno removes `Object.prototype.__proto__` by default; fixed with `--unstable-unsafe-proto`. (2) `shell/dialogs/src/retarget.spec.ts` and `ui.spec.ts` kill their worker: `Uncaught TypeError: Failed to execute 'dispatchEvent' on 'EventTarget': parameter 1 is not of type 'Event'` (the DOM environment's Event is not Deno's global Event). Not fixed; Node passes 1208 of 1208. |
| Playwright, 14 tests | 14 of 14 pass, same as Node. Browser launch and the pipe transport work. |
| `ng serve` under Deno | Not measured. |

Tracked files: none touched. No `deno.lock` was created in any of the runs, `package-lock.json` and `node_modules` stayed as
`npm ci` left them. Permission flags: `-A` was used throughout (Angular needs read, write, env, run, net, sys; narrower
flags were not tried). Install step: none beyond downloading the 40 MB binary. `node_modules` is 581 MB (`du`), as for Node.

Node itself had unrelated, load-induced flakes in this trial (a `cause-codes.spec` timeout at 5 s in 3 of 6 slice runs under
load 30+, one `shell-demo-theme` reduced-motion failure on the first run): note for rule 12, not caused by Deno.

## Numbers

See the results block in `docs/benchmarks.md` ("Deno 2 vs Node for the Angular toolchain"). Summary: user+system CPU is
lower under Deno for `ng build` (median 6.95 s vs 8.85 s, -21 %) and workbench unit tests (5.7 s vs 6.8 s), higher for the
paddock-ui slice (31.3 s vs 23.3 s, +34 %) and equal for Playwright (13.2 s vs 13.1 s). Wall time is the opposite for the
build (warm median 5.06 s vs 3.76 s: Deno has about 1 s more start-up, the Node build uses more threads) and equal for
Playwright. The wall differences are inside the noise of a loaded machine (single runs ranged 3 s to 110 s).

## Recommendation

No gain worth a migration: build and test times are equal or worse in wall time, CPU gains are small and not consistent,
and Vitest in the paddock-ui slice breaks (two worker crashes plus the `__proto__` difference). Keep Node for Angular,
Vitest and Playwright. Revisit only if Angular or Vitest declare Deno support (angular-cli#28884) and the `dispatchEvent`
crash is gone; the cost of a re-check is about an hour with this setup.
