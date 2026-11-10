# 84. Accessible names need a role that allows them: a template lint, one loading-state component, a shared ARIA spec helper and axe over loading, empty and error states

- Status: Proposed (2026-11-10). The user asked for this redesign on 2026-11-10 (ER-21). Needs the decisions below and an
  independent review before any build.
- Date: 2026-11-10
- Related: ER-21 / rule 40 (recurring bugs mean a missing design rule), ER-22 / rules 42, 43 and 55 (share first; one
  implementation per mechanism), ER-33 / rule 71 (a check counts only with a positive control), rule 47 (Material and
  CDK first), C33 (every window is checked with axe), AGENTS.md "every state is designed (skeleton, empty, error with
  Retry)".
- Number: family ADRs on `origin` branches end at 0080; 0081, 0082 and 0085 are held by live agent worktrees; 0083 is
  this ADR's sibling (2026-11-10). Rule and check numbers are assigned on acceptance.
- Evidence read at `origin/main` `b51c72509` unless a row names a branch. The inventory scanner and its output are in the
  session scratch folder (`scratchpad/adr-0083-0084/scan2.py`, `inventory-main.txt`); WP-0 turns it into the lint rule's
  fixture list.

## Pattern

**An element is given an accessible name its role does not allow.** Whenever a template wants to describe a region (a
loading placeholder, a count badge, a status dot, a scrolling `<pre>`), it puts `aria-label` or `aria-labelledby` on a
`div`, `span`, `p`, `pre` or `code`. Those elements map to `generic`, `paragraph` or `code`, which WAI-ARIA 1.2 lists
under "Roles which cannot be named (Name prohibited)" ([§5.2.8.6](https://www.w3.org/TR/wai-aria-1.2/#namefromprohibited):
caption, code, deletion, emphasis, generic, insertion, paragraph, presentation, strong, subscript, superscript); authors
"MUST NOT use the aria-label or aria-labelledby attributes to name the element". Assistive technology ignores the name,
so a skeleton announces nothing and an icon-only badge has no text.

**The invariant that should hold.** Every element with `aria-label` or `aria-labelledby` has an explicit or implicit
role that allows a name, or is a custom element whose host role is declared, or is inside `aria-hidden`. Every loading,
empty and error state is scanned by axe like the loaded state.

## Context: the incidents

| # | Where | What happened |
| --- | --- | --- |
| 1 | Dyno CI run 7860 | axe failed `aria-prohibited-attr` on `.skeleton`: a labelled loading `div` with no role. |
| 2 | `origin/fix/dyno-skeleton-aria` `f86f59b80`, `eaf88500f` (not on `main`) | The fix found the same markup in five Dyno screens (counters, dashboard tile, disk map, hash and web search dialogs) and in the shared `dy-async` wrapper (48 uses in 43 files by `git grep -c "<dy-async"`), and gave each `role="status"`. |
| 3 | paddock-ui `shell/jobs/src/jobs-list.ts:27`, `shell/undo/src/activity-page.ts:65` | The shared shell has the same labelled, role-less loading `div`; its own specs assert the label on it (`jobs-components.spec.ts:68`), as does Pitcrew's (`shared/ui/ui-components.spec.ts:95`). |
| 4 | `apps/dyno/app/e2e/axe.spec.ts:13-27` | The axe page test waits until **no** `[aria-busy="true"]` is left before it scans, so it never sees a loading state; run 7860 found the skeleton only where a loading state outlived that wait. |
| 5 | `origin/fix/dyno-skeleton-aria` `apps/dyno/app/src/testing/aria-name.ts` | The new spec helper treats a missing `role` attribute as "no role" and fails it, so it would also fail a correctly labelled `<section>`, `<nav>` or `<button>` (implicit roles that allow a name); and it is a Dyno copy. |

## Why axe alone misses most of them (verified)

axe-core 4.14.0 (the version in `package-lock.json`) was run in jsdom 30.1.3 on synthetic markup with
`runOnly: ['aria-prohibited-attr']` (43 ms for the document):

| Markup | axe result |
| --- | --- |
| `<div aria-busy="true" aria-label="Loading widgets"></div>` | violation |
| `<span aria-label="Connected"></span>`, `<p aria-label="Waiting"></p>` | violation |
| `<div aria-label="Loading widgets">some text</div>` | **incomplete** (needs review) |
| `<div role="status" aria-busy="true" aria-label="...">`, `<div role="progressbar" aria-label="...">`, `<section aria-label="...">`, `<button aria-label="...">` | pass |

axe fails the rule only when the element has no text it would announce; with text it returns `incomplete`, matching its
source ([`aria-prohibited-attr-evaluate.js`](https://github.com/dequelabs/axe-core/blob/develop/lib/checks/aria/aria-prohibited-attr-evaluate.js),
read 2026-11-10). The shared helper `ui/projects/paddock-e2e/src/axe.ts` returns `results.violations` only, so every
labelled role-less element **with text** passes every axe spec in the family.

## Instances: every labelled element whose role prohibits a name (`origin/main` `b51c72509`, 2026-11-10)

**Method.** A scanner over the 2,252 tracked non-spec `*.ts` and `*.html` files (inline templates included) finds start
tags of `div span p pre code b i u s small em strong del ins sub sup q samp data bdi bdo` that carry `aria-label`,
`aria-labelledby` or their `[attr.…]` bindings, with no `role` or a name-prohibited static role. Elements carrying a CDK
or Material directive that sets a role through its host (`cdkMenu`, `cdkMenuBar`, `cdkListbox`, `cdkOption`, `matMenu…`
and similar) are left out: the first pass counted 86, of which 22 carried such a directive (19 `cdkMenu`, 2 `cdkMenuBar`, 1 `cdkListbox`). "Empty" means no text between the
tags (an axe violation); "text" means axe reports `incomplete` and the family's helper ignores it. **Positive control:**
on `origin/fix/dyno-skeleton-aria` the scanner finds exactly the six Dyno "empty" rows below gone and nothing new.

| Owner | Empty (axe violation) | With text (axe incomplete) | Total | Of them, loading placeholders (`aria-busy`) |
| --- | --- | --- | --- | --- |
| Debrief | 0 | 13 | 13 | 0 |
| Dyno | 6 | 8 | 14 | 6 |
| Pitcrew | 2 | 8 | 10 | 2 |
| Pitwall | 5 | 10 | 15 | 0 |
| Spotter | 0 | 1 | 1 | 0 |
| paddock-ui | 2 | 5 | 7 | 2 |
| shell-demo | 0 | 1 | 1 | 0 |
| **Shipped total** | **15** | **46** | **61** | **10** |

Plus 3 in the design mock `docs/design/tray-widgets/mock.html` (not shipped). Pacenotes, Sighter and workbench-demo have
none. By element: 23 `span`, 18 `div`, 10 `p`, 9 `pre`, 1 `code`. One Pitcrew row (`shared/ui/load-state.ts:36`) is
counted as empty by hand: the scanner's text heuristic was misled by a `>` inside a binding.

The rows (paths below `app/src/app/` for apps, below `ui/projects/paddock-ui/` for paddock-ui; *E* = empty):

- **Debrief:** `ai/chat/chat-panel.ts:132 p`, `ai/folder-setup/folder-setup-wizard.ts:156 pre`,
  `ai/settings/ai-settings-extras.ts:26 p`, `ai/setup/setup-wizard.ts:31 p`, `analysis/outliers-view.ts:42 span`,
  `plugins-panel/plugins-table.ts:71 span`, `problems/problems-panel.ts:142 pre`, `profile/history/profile-history.ts:56 pre`,
  `profile/pipeline-editor.ts:56 div`, `share/dashboard/dashboard-tile.ts:62 div`, `store-pane/sql-console.ts:92 pre`,
  `timeline/timeline-legend.ts:71 span`, `tools/perfmon/perfmon-view.ts:120 pre`.
- **Dyno:** `features/counters/counters-page.ts:155 div E`, `features/dashboard/tile.ts:28 div E`,
  `features/disk-map/disk-map-page.ts:220 div E`, `features/lookup/hash-search-dialog.ts:36 div E`,
  `features/lookup/web-search-dialog.ts:36 div E`, `shared/async-state.ts:30 div E` (all six fixed on the branch);
  `features/benchmarks/bench-page.ts:217 p`, `features/lookup/hash-search-dialog.ts:29 code`,
  `features/power/power-page.ts:160 div`, `:183 div`, `features/processes/processes-page.ts:106, 115, 124 span`,
  `features/settings/settings-page.ts:53 p`.
- **Pitcrew:** `features/logs/logs-page.html:57 div E`, `shared/ui/load-state.ts:36 div E`;
  `features/dashboard/runner-tile.html:17 span`, `:76 div`, `:104 span`, `features/insights/insights-page.html:100 span`,
  `features/runners/config/config-diff-dialog.ts:25 span`, `features/runners/runner-detail-page.html:36 span`,
  `layout/sidebar.ts:50 span`, `:55 span`.
- **Pitwall:** `features/apps/apps-page.ts:86 span E`, `features/dashboard/apps-card.ts:54 span E`,
  `features/dashboard/hot-models.ts:40 span E`, `features/dashboard/queue-card.ts:29 span E`,
  `features/playground/playground-page.html:59 p E`; `features/apps/apps-page.ts:52 p`, `:54 div`,
  `features/dashboard/hot-models.ts:59 span`, `features/dashboard/pairing-card.ts:33 p`, `:35 div`,
  `features/dashboard/queue-card.ts:18 p`, `features/requests/request-detail.ts:92 pre`, `:94 pre`,
  `shell/sidebar.ts:66 span`, `:69 span`.
- **Spotter:** `layout/sidebar.ts:47 span`.
- **paddock-ui:** `shell/jobs/src/jobs-list.ts:27 div E`, `shell/undo/src/activity-page.ts:65 div E`;
  `shell/about/src/about-dialog.ts:54 pre`, `shell/about/src/updates-dialog.ts:71 pre`,
  `shell/diagnostics/src/report-problem-dialog.ts:53 div`, `states/src/needs-ai-card.ts:38 p`,
  `workbench/explorer/src/explorer.ts:101 span`.
- **shell-demo:** `ui/projects/shell-demo/src/app/runners-status.ts:15 span`.

**Shapes.** (1) Loading placeholders (10): want one shared loading state (§2). (2) Status dots and count badges on a
`span` (about 20): the name belongs in visually hidden text or on the element that has the role (the link or button
around it). (3) Scrolling `pre` with `tabindex="0"` (9): a focusable scroll area wants `role="region"` (or a `section`)
with the name. (4) Labelled `p`/`div` that already show the same text (the rest): drop the redundant label.

**Coverage gaps.** Pitcrew, Pitwall and Spotter have no spec that calls the shared axe helpers (`git grep` for
`axeViolations|axeInThemes|axeTargets|axeEveryDialog|AxeBuilder` finds only Debrief, Dyno and `ui/e2e`); WP-0 confirms
whether C33 runs for them or is waived. ESLint already applies angular-eslint's `templateAccessibility` set to every
template (`eslint/src/preset.cjs:79`, inline templates through `processInlineTemplates`), and none of its rules catches
this: all 61 rows lint clean today.

## Decision

### 1. A template lint rule: `paddock/template-name-needs-role`

Added to `eslint/src/rules/template-rules.cjs` and switched on as `error` in the preset for `**/*.html` and inline
templates (all five apps' `eslint.config.js` and `ui/eslint.config.cjs` use the preset):

- On every element with a static `aria-label`/`aria-labelledby` or a bound `[attr.aria-label]`/`[attr.aria-labelledby]`
  (`[aria-label]` too), work out the role: a static `role` wins; a bound `[attr.role]` is accepted (it cannot be known);
  a directive attribute in the preset's `hostRoleDirectives` list (CDK and Material defaults; an app adds its own) is
  accepted; otherwise the implicit role from HTML-AAM (a table in the rule; `aria-query`'s `elementRoles`, Apache-2.0
  and already in the lockfile, is the cross-check in the rule's test).
- Report when that role is in ARIA 1.2's name-prohibited list (`generic` for `div`, `span`, `b`, `i`, `u`, `s`, `small`,
  `q`, `samp`, `data`, `bdi`, `bdo`, `pre`, and a `section` or `header`/`footer` only when the rule cannot tell; plus
  `paragraph`, `code`, `emphasis`, `strong`, `deletion`, `insertion`, `subscript`, `superscript`, `presentation`,
  `none`), unless the element is inside an `aria-hidden="true"` ancestor in the same template. Custom elements are out
  (axe treats valid custom elements as allowed).
- The message names the shape and the fix: "a `div` has no role that allows a name; use `<pd-loading-state>`, a role
  that fits (`region`, `status`, `img`, `group`), or visually hidden text".
- **Positive controls** in `eslint/test`: one fixture per shape (empty `div`, `span` with text, `pre` with `tabindex`,
  `p`, a `[attr.aria-label]` binding, a static prohibited `role="presentation"`) must fail; a `section`, `button`, `nav`,
  a `cdkMenu` `div` and a labelled custom element must pass; the test asserts how many templates and elements the rule
  visited and fails when none.

### 2. One loading state: `pd-loading-state` in paddock-ui

Beside `pd-empty-state` and `pd-error-state` (`src/lib/states.ts`): `<pd-loading-state [label]="..." [rows]="n">`
renders the placeholder with the role of D2, `aria-busy="true"`, the name, and `pd-skeleton` children (already
`aria-hidden`). Dyno's `dy-async`, Pitcrew's `load-state`, paddock-ui's jobs list and activity page, and the six other
loading rows use it; their copies of the markup go (rule 55: registered in `docs/known-copies.toml` until removed, if any
app cannot move in the same change). Its spec runs the helper of §3.

### 3. One spec helper that knows implicit roles: `expectAriaValid` in `@decryp7/paddock-ui/testing`

`expectAriaValid(el, { rules? })` runs axe-core inside the spec's jsdom on `el`'s subtree with the ARIA rules only
(`aria-prohibited-attr`, `aria-allowed-attr`, `aria-allowed-role`, `aria-required-attr`, `aria-valid-attr-value`), and
fails on violations **and** on `incomplete` results of `aria-prohibited-attr`, naming the selector and rule. One engine
for unit specs and Playwright, so implicit roles, custom elements and `aria-hidden` are judged the same way
(verified in jsdom above, 43 ms). `axe-core` (MPL-2.0, allowed as weak copyleft) becomes a declared dependency of the
testing entry only, never of the shipped library. Dyno's `src/testing/aria-name.ts` is replaced by it, and the specs
that assert today's wrong markup (`jobs-components.spec.ts:68`, Pitcrew `ui-components.spec.ts:95`, Dyno's
`lookup-dialogs.spec.ts` checks on the branch) assert through it instead.

### 4. axe over loading, empty and error states

- **`axeViolations` counts `incomplete` for a declared list of rules** (`AxeOptions.failIncomplete`, default
  `['aria-prohibited-attr']`), so the 46 "with text" rows fail in the browser too.
- **`AxeTarget` gains `states`:** `axeTargets` visits each target in its loaded state and in each state listed
  (`'loading' | 'empty' | 'error'`), by URL parameter `?state=<s>` that every app's **mock backend** honours: `loading`
  leaves every read pending, `empty` answers empty lists, `error` fails every read with a typed error (the mock exists
  for tests and development only, AGENTS.md). The loaded state keeps today's "no `aria-busy` left" wait; the `loading`
  state waits for at least one `[aria-busy="true"]` and reports `check broken` when none appears (rule 71), the `empty`
  and `error` states wait for `pd-empty-state` / `pd-error-state` (or the app's declared selector).
- **C33 gains a line:** an app's axe spec passes `states` for its pages (static, like today's dialog coverage), with a
  reasoned skip list in `paddock.conformance.json`.

### 5. Rule text (proposed; number on acceptance)

"An element named with `aria-label` or `aria-labelledby` has a role that allows a name; loading placeholders use the
shared loading state. Every page is checked with axe in its loaded, loading, empty and error states, and `incomplete`
results of `aria-prohibited-attr` count as failures. Checked by `paddock/template-name-needs-role`, `expectAriaValid`
and C33."

## Alternatives rejected

- **Fix each instance when axe trips** (today): axe sees only the 15 empty rows, and only in states the tests reach.
- **A conformance regex over templates** instead of ESLint: the scanner for this ADR needed hand fixes for CDK host
  roles and a `>` inside a binding; angular-eslint's template AST already gives elements, static and bound attributes.
  The scanner stays only as WP-0's inventory.
- **Dyno's role table helper, shared as is:** it does not know implicit roles; a full HTML-AAM table is what axe already
  maintains.
- **`role="status"` everywhere a label sits:** passes axe but makes badges and `pre` blocks live regions; the fix
  depends on the shape (§Shapes).
- **Scan states only in unit specs:** misses layout, theme and contrast of the loading and error states, which only the
  browser shows.

## Consequences

- New labelled role-less elements fail lint in the editor and in CI, before any axe run.
- Every axe-checked page costs up to three more scans per theme (Dyno: 20 pages, so up to 60 more per theme). WP-4
  measures the added time and records it (rule 48); D4 limits states to one theme if it is large.
- The mock backends grow one switch each; the real backend is untouched.
- Screen readers announce loading placeholders consistently across the family (one component).

## Work packages (raw agent-days, one agent)

| WP | Content | Days |
| --- | --- | --- |
| WP-0 | Confirm the 61 rows by reading each; turn them into the lint rule's fixture list; confirm C33's status for Pitcrew, Pitwall and Spotter | 0.5 |
| WP-1 | `paddock/template-name-needs-role` with fixtures and positive controls, `hostRoleDirectives` option, preset wiring | 1.5 |
| WP-2 | `pd-loading-state` (role per D2, a screen-reader check with VoiceOver on macOS and Narrator in the local Windows VM, recorded), gallery entry, CHANGELOG | 1 |
| WP-3 | `expectAriaValid` in `@decryp7/paddock-ui/testing` with its own tests; replace Dyno's `aria-name.ts` | 1 |
| WP-4 | `failIncomplete` and `states` in `axe.ts`; the `?state=` switch in the mock backends of Dyno, Debrief and the shell demo; C33 extension; measure the added time | 2 |
| WP-5 | Fix the 61 rows through the mechanism: paddock-ui and shell-demo 0.5, Dyno 0.5 (after the branch lands), Pitwall 0.5, Pitcrew 0.5, Debrief 0.5, Spotter 0.25 | 2.75 |
| WP-6 | Axe specs with states for Pitcrew, Pitwall and Spotter (none run axe today) and their mock switches | 2.5 |
| WP-7 | Rule text, lessons entry, ADR index | 0.25 |
| | **Total** | **11.5** |

First cut (stops new instances and closes the empty, axe-failing ones): WP-0, WP-1, WP-2, WP-3, and WP-5 for the 15
empty rows and the 10 loading rows = **about 5.5 days**. The states scans (WP-4, WP-6) follow per app.

## Release impact (ER-1, ER-31)

paddock-ui and the ESLint preset are shared: every app that lints with the preset or ships paddock-ui is in the affected
plan; the rule lands with its fixes per app (rule 43), so no app's lint turns red without its fixes. Dyno's branch
`fix/dyno-skeleton-aria` lands as it is (its `role="status"` passes axe) and moves to `pd-loading-state` in WP-5.
Nothing here blocks a tag in flight.

## Decisions for the user

| # | Question | Options | Recommendation |
| --- | --- | --- | --- |
| D1 | Where the static check lives | (a) An ESLint template rule in the shared preset. (b) A conformance check over template text. | (a): it has the template AST and runs in the editor. |
| D2 | The role of a loading placeholder | (a) `role="progressbar"` without a value (indeterminate) and the name, with `aria-busy` on the region being loaded (ARIA 1.2's progressbar text). (b) `role="status"` with the name (Dyno's fix). (c) `role="status"` with visually hidden "Loading ..." text and no `aria-label`. | (a), confirmed by the WP-2 screen-reader check; (c) if VoiceOver or Narrator does not speak it. Not verified with a screen reader yet. |
| D3 | The spec helper | (a) axe-core in jsdom (`expectAriaValid`). (b) A role table (Dyno's helper extended with implicit roles). | (a): one engine for unit and browser checks. |
| D4 | States in the axe scans | (a) Loading, empty and error in both themes. (b) In one theme (the loaded state stays in both). (c) Only loading. | (b), switching to (a) if WP-4 measures under 20 % added time. |
| D5 | Rollout | (a) The rule lands as `error` with every app's fixes in one change. (b) Per app with a shrink-only baseline until each app's next tag. | (a): 61 small edits, about 2.75 days; a baseline would live longer than the work. |
| D6 | Binding rule | Rule text of §5. | Yes. ER candidate: "accessibility checks scan every state, not only the loaded one" as an ER-33 example, not a new ER. |
| D7 | Scope now | (a) All WPs (11.5 days). (b) The first cut (about 5.5 days) now, the states scans per app before its next release. | (b). |
