# Shell adoption checklist

- Status: checklist (2026-10-28). Written from the independent reviews of the Spotter move (`spotter/shell-1.0`) and the
  Pitcrew move (`pitcrew/shell-1.1`), which hit the same findings (ER-21: a recurring class gets a shared mechanism and a
  check). Dyno is next; Pitwall 1.1 and the later apps use it too.
- Use: go through it before asking for review, and paste the ticked list in the branch's last commit message or PR text.
  Items marked **(checked)** fail in CI or in the app's own spec if you get them wrong; **(manual)** ones are on you and on the
  reviewer. The helpers are heuristics, not proof: they catch the findings of the two reviews, a determined author can still
  fool them, so every image is also reviewed by someone else.
  Where it is enforced: C44 runs in every app's gates (`app-gates` action) and in Paddock's conformance job; the helpers'
  self-test runs in ci.yml (job `ui`); `captureDocScreens` goes through `captureScreen`; the other helpers only fail in a spec
  that calls them, so **(checked)** there means "once your spec calls it".

## 1. Screenshots (documentation and review images)

Findings: images taken before content loaded (skeletons, avatars not decoded), cropped mid-element, the frame scrolled
instead of the page, and one image "doctored" (a cover painted over cut-off content).

- [ ] Take every image with `captureScreen(page, path, opts)` from `@decryp7/paddock-e2e`, or with `captureDocScreens`, which
      calls it (it has no crop or clip options any more). **(checked once your spec calls it)** It waits for: no `pd-skeleton`, `.skeleton` or `[aria-busy=true]`; fonts ready; every
      `img` decoded; no running animation or transition in main; the same layout in two looks in a row.
- [ ] Only the main scroller scrolls (`scrollTop` option); the frame never does. **(checked)**
- [ ] Nothing is cut mid-element at the bottom edge; a long page is taken whole with `fullPage: 'main'`, which grows the
      window by exactly the hidden height and restores it. **(checked)**
- [ ] No cover, mask or crop to hide a flaw: fix the page. `captureScreen` refuses an opaque, empty cover (any CSS colour
      syntax, `::before`/`::after`, fixed and sticky bars); `allow` only excuses a cut-off element, never a cover.
      **(checked, heuristic)**
- [ ] A shot of "Still working" or another running state: emulate `reducedMotion: 'reduce'` (the shell honours it), or name
      the spinner in `animated`; never take it mid-animation.
- [ ] A reviewer who did not take the images looks at every one (ER-23; C24 needs the `REVIEWED.md` row). **(manual)**
- [ ] Data in images is invented, never a user's (ER-14).

## 2. Layout under the shell host

Finding: a page lost its height under `pd-page-host`: a CDK virtual viewport resolved to 0 because `:host` / `height: 100%`
had no definite parent height.

- [ ] The shell's `pd-page-host` is a flex column that fills `<main>` (`min-height: 0`), and the loaded page's box
      (`pd-part-outlet.pd-page`) is a flex item that may shrink: `height: 100%` in a page resolves. Make the page's own
      host `display: block` or `flex`, with `height: 100%` (or `flex: 1; min-height: 0` in a flex column) — not `auto`.
- [ ] Every spec of a page with a list, table or virtual viewport calls `expectPageFillsHost(page)`: it fails when the real
      page component is shorter than the host or a rendered `cdk-virtual-scroll-viewport` is 0 high (`fill: false` for a
      content-high page). **(checked once your spec calls it)**
- [ ] A page with `height: 100%` and `overflow: hidden` is clipped to the host now, not scrolled by `<main>`: use
      `overflow: auto` inside the page, or no fixed height. **(manual)**
- [ ] A route outside the shell page host (a detail or full-screen route) gives its own root a definite height. **(manual)**
- [ ] No horizontal scroll at 320 px and up (`expectNoHorizontalScroll`, `LAYOUT_WIDTHS`).

## 3. Dialogs

Finding: a dialog's primary button was clipped because the footer sat inside the scroll area.

- [ ] The dialog is a real OS window (rule on dialogs); the body scrolls, the footer is a sibling outside the scroll area
      and always visible, at the dialog's smallest size and with long content. **(manual)**
- [ ] The primary action of every dialog and wizard step is asserted with `expectFullyVisible(locator)`; it polls until the
      button holds still and fails for one that is outside the window, clipped by an ancestor's overflow, or covered at its
      centre or a corner. **(checked once your spec calls it)**
- [ ] Dialog kinds, sizes and buttons come from the shared dialog API, not hand-made.

## 4. Jobs and notices

Findings: success toasts for trivial actions, and double announcements (the app's own job listener plus the shell's
`JobsStore`).

- [ ] No `paddock://jobs` listener in the app and no use of `JobsBackend`/`TauriJobsBackend`/`MemoryJobsBackend` outside the
      provider setup. **(checked: conformance C44, in the app gates)** Start work with `JobFollow.run(...)` or
      `JobsStore.follow(id)` from `@decryp7/paddock-ui/shell/jobs` and act on its result; the shell announces the end.
- [ ] The app's commands that start a job say `quiet_on_success` where the job's own end notice is enough (no second
      "done" toast from the command). **(manual)**
- [ ] No toast for a trivial, instant, visible action (a copy, a toggle, a rename that the UI shows). A toast is for an
      outcome the user could miss: a background job's end, an error with a next step.
- [ ] One announcement per event: if the shell already says it (job end, health change, update), the app does not.
- [ ] Nothing times out a running job; the UI shows "Still working" (rule 12, C35). **(manual; C35 checks the call policy only when the app's conformance run includes it)**
- [ ] A tray or toast action that opens something navigates to the page that shows it (and the page exists after the move). **(manual)**

## 5. Copy and removed UI

Finding: texts that still pointed at UI that was removed in the move.

- [ ] Search the whole app (templates, `$localize` texts, help, errors, empty states, docs, dialog copy, menu items) for
      the names of every control, menu, page and setting the move removed or renamed, and fix or delete each hit.
- [ ] Error and empty states say what to do with the controls that exist now; sentence case, verb plus object on buttons.
- [ ] Rebuild the translation catalogs (`paddock-i18n build`; C32). **(manual; C32 checks it when the app's conformance run includes it)**
- [ ] No "mock", "demo" or "placeholder" wording in user-facing text. **(manual)**

## 5b. What the move replaces

- [ ] Delete the app's own copy of every shell feature the move replaces (its notifications, jobs list, settings shell,
      about, update dialog, menu); a copy left behind drifts and shows twice. **(manual)**
- [ ] No command name of the app collides with a shell plugin command (`paddock-*`); the shell's wins at run time. **(manual)**
- [ ] The update feed and the release notes belong to the app's tag workflow (`<app>-release.yml`, `latest.json` for
      `<app>-vX.Y.Z`), and the in-app update check reads that feed. **(manual)**
- [ ] A dangerous confirmation has Cancel as the default button and exactly one primary button per state. **(manual)**

## 6. Before review

- [ ] Affected tests only (`node scripts/affected.mjs origin/main --explain`), plus the touched e2e files, one engine.
- [ ] `captureScreen`/`expectFullyVisible`/`expectPageFillsHost` are used in the app's e2e, not copies of them (rule 42). **(manual)**
- [ ] CHANGELOG entry for anything shared that changed; the app's own docs updated.

## Adoption steps per app

- Pitcrew: `screenshots.spec.ts` lines 111 and 159 take clipped shots (`clip`): replace them with `captureScreen` of the whole
  window (or `fullPage: 'main'`); a region shot hides what is around it.
- Pitwall: `capture.shot.ts` takes element shots: take the whole page with `captureScreen` instead, or `captureDocScreens`
  (which has no `element`/`clip` options any more).
- Spotter and Dyno: route their screenshot specs through `captureScreen` / `captureDocScreens`; delete their own settle code.
- Spotter and Pitcrew: wire the shell's jobs only through `provideShellJobs` options (a backend factory is fine); wait for
  jobs with `JobFollow`; drop `paddock://jobs` listeners (Pitcrew `tauri-backend.ts`).

## How an app adopts the helpers

```ts
import { captureScreen, expectFullyVisible, expectPageFillsHost, expectNoHorizontalScroll, test, expect } from '@decryp7/paddock-e2e';

test('models page', async ({ app, page }) => {
  await app.sidebar.link('Models').click();
  await expect(app.frame.heading).toHaveText('Models');
  await expectPageFillsHost(page);                       // the list did not collapse
  await expectNoHorizontalScroll(page);
  await captureScreen(page, 'docs/screenshots/dark/models.png');               // settled, not cut, not doctored
  await captureScreen(page, 'docs/screenshots/dark/models-all.png', { fullPage: 'main' });
});

test('import dialog', async ({ page }) => {
  await expectFullyVisible(page.getByRole('button', { name: 'Import files' })); // the primary action is not clipped
});
```
