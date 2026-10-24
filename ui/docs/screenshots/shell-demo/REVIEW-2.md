# Shell demo "pages" frame: independent visual re-review (rule 22)

Reviewed 2026-10-24 from the 20 PNGs in this folder only (no code). Specs: `docs/design-guide.md` (shell/integrate
version, with the new title-bar, modal-drawer and selected-item rules), `briefs/ui-review-2-patterns.md`,
`briefs/design-review-drawings-overview.md`. Measurements are from the @2x images, given in CSS px.

Given context: the bell's count is danger-tinted on purpose (an unread error exists). The macOS shot is the browser
simulating the app (a 78 px inset with no real traffic lights).

Overall: the fixes landed. The frame now has realistic content, one selection style, a 40 px title bar, a modal
drawer that covers the status bar, a compact status bar at 320 px, an indeterminate job bar and a health popover.
There is no P1. Two new P2s concern a missing status item in one shot and the health wording; the rest are P3 polish.

## Previous findings

| # | Status | Evidence |
|---|---|---|
| 1 | Fixed | narrow-*: the chip shrinks to "Stopped", jobs to spinner + "1", the bell + 2 is pinned, and "… 1" carries a count. (One shot is the exception: see new #N1.) |
| 2 | Fixed | "… 1" is readable muted text, has a 24 px-high target, and focus-more shows its ring. |
| 3 | Fixed | The selection is the same in expanded, rail and overlay modes, both themes: tint, accent icon, 600 label and a 2 px accent bar. |
| 4 | Fixed | The job bar is indeterminate (accent stripes on a track) with a growing "14 s elapsed". |
| 5 | Fixed | The title is "Update runner" with the target "build-mac-2" as meta. The phase is "Waiting for the device". |
| 6 | Fixed | The phase line is muted. Warning (icon + text) appears only on "taking longer than usual". |
| 7 | Fixed | The running indicator is a neutral spinner and the nav count is a neutral pill. The bell is danger on purpose. |
| 8 | Fixed | pages-macos-*: there is no in-app menu, and the toggle and name start after the 78 px inset. |
| 9 | Mostly fixed | Runners page, runner table and "3 of 4 runners online". The nav still lists the harness pages "Dialogs" and "Keyboard" next to Runners (P3, see N9). |
| 10 | Fixed | The chip is a borderless tint pill, and pages-health shows a popover with the cause and an action (wording: see N2). |
| 11 | Fixed | The gutter is 16 px at 320. |
| 12 | Fixed at the bottom, new at the top | The shadow no longer falls on the status bar, but it now bleeds up onto the title bar (N3). |
| 13 | Fixed | The drawer and scrim cover the status bar (rail-open, narrow-open). |
| 14 | Fixed | The rail badge sits on the icon corner with a ring in the sidebar colour. |
| 15 | Fixed | The toggle uses the sidebar glyph at every width, and the app name stays at 320. |
| 16 | Fixed | The title bar is about 40 px, which is now documented in the guide. |
| 17 | Partly fixed | Focus is shown on the toggle and on "…" (2 px accent ring, clearly at least 3:1 in both themes). "focus-nav" does not focus a nav item (N6), and the ring on "…" is not contained (N7). |

## New findings

| # | Pri | Screenshot / region | Problem | Fix |
|---|---|---|---|---|
| N1 | P2 | focus-toggle-dark, status bar | The jobs item (spinner + "1") is missing, while the job card above still shows the job running. focus-toggle-light, narrow-* and focus-more-* all show it. Either the item flickers (for example while it re-measures overflow or when focus changes) or the capture is not deterministic. In both cases this is the exception rule 11/12 says must never vanish. | Find out which one it is. If the item drops out on a re-layout, fix it and add a test asserting the jobs item stays visible at 320 px with focus anywhere. If it is a capture race, wait for a stable status bar before shooting, then re-shoot. |
| N2 | P2 | pages-health-*, chip vs popover | One fact is described three ways: the chip says "stopped", the popover title says "damaged", and the button says "Try again" with "to rebuild it" in the body. The button does not name the repair (rule 19: a working Repair action). | Use one state word in the chip and the title, e.g. chip "Search index: damaged" and title "The search index is damaged". Name the action after what it does: "Rebuild index" (primary), and drop the "Choose Try again…" sentence or replace it with the consequence ("Search is unavailable until it is rebuilt."). |
| N3 | P3 | rail-open-light, narrow-open-light, title bar left | The drawer's shadow spreads upwards: there is a grey gradient behind the toggle and "Paddock demo" (about y 30–40 px). The title bar looks dirty and its bottom hairline disappears over the drawer. | Cast the shadow to the right only (e.g. `8px 0 24px -8px`), or clip it at the drawer's top edge. Keep the title-bar hairline above the drawer. |
| N4 | P3 | narrow-*, all status bars | The static spinner is a thin 270° arc in text colour, and it reads as a letter: "C 1", "C 1 running". | Draw the spinner over a faint full ring (track at about 30 % opacity), or use a distinct "activity" glyph. Freeze it at a recognisable angle for screenshots. |
| N5 | P3 | narrow-*, job card warning | When the warning wraps to two lines, the ⚠ icon is centred between them instead of on the first line. | Align the icon to the first line's text box (`align-items: start` plus a small top offset to match the cap height). |
| N6 | P3 | pages-focus-nav-* | The name says "nav", but focus is on the sidebar toggle. No shot shows a focused nav item, which is the case most likely to clip (the ring inside the 216 px sidebar, next to the selected tint). | Re-shoot with a nav item focused (and ideally one that is focused and selected), or rename the file to focus-titlebar. |
| N7 | P3 | pages-focus-more-*, "… 1" | The focus ring is taller than the 24 px status bar: it crosses the bar's top hairline and its bottom stroke sits flush on the window edge. | Inside the status bar use an inset ring (`outline-offset: -2px`) or a 20 px item height with a 2 px offset, so the ring fits inside the bar. |
| N8 | P3 | wide-*, table vs job card | The demo data contradicts itself. "Update runner build-mac-2" is waiting for the device, but build-mac-2's row says Online with its last job 18 min ago. build-mac-1 is "Running a job" but has no job. The "Runners 1" nav badge doesn't say what the 1 counts (offline? updating?). | Make build-mac-2's state "Updating" (info dot), so the job and the row agree. Give the nav badge a clear meaning (e.g. 1 offline, with that tooltip), or remove it. |
| N9 | P3 | sidebar, all shots | "Dialogs" and "Keyboard" are harness pages sitting beside a realistic "Runners" page. Since these shots are the family's frame reference, readers will copy that navigation. | Use plausible manager pages (Overview, Runners, Jobs, Settings) in the reference shots and keep the harness pages behind a dev flag. Otherwise say clearly in the folder README that this is a harness. |
| N10 | P3 | narrow-*, chip "Stopped" | At 320 the chip has lost its subject: "Stopped" alone doesn't say what stopped. | Keep a subject glyph (a search/index icon) before the word, or use a short form such as "Index stopped". Keep the full text in the tooltip and aria-label. |
| N11 | P3 | wide-*, status bar "1 running: Still working" | The status bar repeats the card's "Still working" wording rather than saying what is running. | Show "1 running: Update runner" (and the phase in the tooltip). The card owns the "still working" message. |
| N12 | P3 | wide-*/rail-*, State column | Healthy rows say "Online" in words as well as a dot. Rule 3 prefers dot-only for the normal state, with words on exceptions. | Optional: keep the words here, because State is the column's content and a dot alone in a table cell is too terse. Make sure the dot alone never carries meaning. No change needed if this is a deliberate choice. |

## Notes
- Contrast: all text passes 4.5:1 in both themes, by eye and from the earlier measurements. The striped bar, focus
  rings and selection bar are well above 3:1. Light warning text (#8F5F00-class) on white is still about 5.4:1.
- macOS (optional, not a finding): native sidebar apps (Finder, Mail) often run the sidebar material up behind the
  traffic lights. The flat full-width title bar shown here is acceptable and matches the guide as written.
