# Shell adoption checklist

- Status: checklist (2026-10-28). Written from the independent reviews of the Spotter move (`spotter/shell-1.0`) and the
  Pitcrew move (`pitcrew/shell-1.1`), which hit the same findings (ER-21: a recurring class gets a shared mechanism and a
  check). Dyno is next; Pitwall 1.1 and the later apps use it too.
- Use: go through it before asking for review, and paste the ticked list in the branch's last commit message or PR text.
  Items marked **(checked)** fail in CI or in the spec if you get them wrong; the others are on you and on the reviewer.

## 1. Screenshots (documentation and review images)

Findings: images taken before content loaded (skeletons, avatars not decoded), cropped mid-element, the frame scrolled
instead of the page, and one image "doctored" (a cover painted over cut-off content).

- [ ] Take every image with `captureScreen(page, path, opts)` from `@decryp7/paddock-e2e` (or `captureDocScreens`, which
      should call it). **(checked)** It waits for: no `pd-skeleton`, `.skeleton` or `[aria-busy=true]`; fonts ready; every
      `img` decoded; no running animation or transition in main; the same layout in two looks in a row.
- [ ] Only the main scroller scrolls (`scrollTop` option); the frame never does. **(checked)**
- [ ] Nothing is cut mid-element at the bottom edge; a long page is taken whole with `fullPage: 'main'`, which grows the
      window by exactly the hidden height and restores it. **(checked)**
- [ ] No injected overlay, cover, mask or crop to hide a flaw: fix the page. `captureScreen` refuses an opaque cover
      (`allow` is for a documented exception, say why next to it). **(checked)**
- [ ] A reviewer who did not take the images looks at every one (ER-23; C24 needs the `REVIEWED.md` row).
- [ ] Data in images is invented, never a user's (ER-14).

## 2. Layout under the shell host

Finding: a page lost its height under `pd-page-host`: a CDK virtual viewport resolved to 0 because `:host` / `height: 100%`
had no definite parent height.

- [ ] The shell's `pd-page-host` is a flex column that fills `<main>` (`min-height: 0`), and the loaded page's box
      (`pd-part-outlet.pd-page`) is a flex item that may shrink: `height: 100%` in a page resolves. Make the page's own
      host `display: block` or `flex`, with `height: 100%` (or `flex: 1; min-height: 0` in a flex column) — not `auto`.
- [ ] Every spec of a page with a list, table or virtual viewport calls `expectPageFillsHost(page)`. **(checked)** It fails
      when the page is shorter than the host or a `cdk-virtual-scroll-viewport` is 0 high.
- [ ] No horizontal scroll at 320 px and up (`expectNoHorizontalScroll`, `LAYOUT_WIDTHS`).

## 3. Dialogs

Finding: a dialog's primary button was clipped because the footer sat inside the scroll area.

- [ ] The dialog is a real OS window (rule on dialogs); the body scrolls, the footer is a sibling outside the scroll area
      and always visible, at the dialog's smallest size and with long content.
- [ ] The primary action of every dialog and wizard step is asserted with `expectFullyVisible(locator)`. **(checked)** It
      fails for a button that is outside the window, clipped by an ancestor's overflow, or covered.
- [ ] Dialog kinds, sizes and buttons come from the shared dialog API, not hand-made.

## 4. Jobs and notices

Findings: success toasts for trivial actions, and double announcements (the app's own job listener plus the shell's
`JobsStore`).

- [ ] No `paddock://jobs` listener in the app. **(checked: conformance C44)** Start work with `JobFollow.run(...)` or
      `JobsStore.follow(id)` from `@decryp7/paddock-ui/shell/jobs` and act on its result; the shell announces the end.
- [ ] No toast for a trivial, instant, visible action (a copy, a toggle, a rename that the UI shows). A toast is for an
      outcome the user could miss: a background job's end, an error with a next step.
- [ ] One announcement per event: if the shell already says it (job end, health change, update), the app does not.
- [ ] Nothing times out a running job; the UI shows "Still working" (rule 12, C35).

## 5. Copy and removed UI

Finding: texts that still pointed at UI that was removed in the move.

- [ ] Search the whole app (templates, `$localize` texts, help, errors, empty states, docs, dialog copy, menu items) for
      the names of every control, menu, page and setting the move removed or renamed, and fix or delete each hit.
- [ ] Error and empty states say what to do with the controls that exist now; sentence case, verb plus object on buttons.
- [ ] Rebuild the translation catalogs (`paddock-i18n build`; C32).

## 6. Before review

- [ ] Affected tests only (`node scripts/affected.mjs origin/main --explain`), plus the touched e2e files, one engine.
- [ ] `captureScreen`/`expectFullyVisible`/`expectPageFillsHost` are used in the app's e2e, not copies of them (rule 42).
- [ ] CHANGELOG entry for anything shared that changed; the app's own docs updated.

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
