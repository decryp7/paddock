# Shell demo "pages" frame: independent visual review (rule 22)

Reviewed 2026-10-24 from the 10 PNGs in this folder only (no code). Specs: `paddock/docs/design-guide.md`,
`briefs/ui-review-2-patterns.md` (20 UI rules), `briefs/design-review-drawings-overview.md`. Measurements are from the
@2x images and given in CSS px. Contrast values are estimates from the pixels.

Overall: the frame is clean and mostly on spec. Sidebar 216 / rail 56 / overlay 216 px, a 24 px page gutter, a 24 px
status bar, an 18/600 title with a muted subtitle, a 10 px card radius, a 32 px secondary button, and both themes tuned
separately (light is not an inverted dark). Expanded, rail and overlay behave as expected. There is no P1 in the frame.
The problems are a weak and inconsistent selection, the job card's progress bar, status-bar priority at 320 px, and
placeholder content.

## Findings

| # | Pri | Screenshot / region | What's wrong | Fix |
|---|---|---|---|---|
| 1 | P2 | narrow-{dark,light}, status bar right | At 320 px the bell and its **3 unread** count disappear behind a faint "…" with no badge. The jobs item goes too, so a running job and unread notifications are invisible at rest. The overflow order is wrong: the attention count is hidden and the long text chip is kept. | Not acceptable as is. Collapse by **form** before hiding items: (a) the health chip drops its text to icon + "Stopped" (or icon only, with the full text in the tooltip and aria-label); (b) jobs become a compact "1" pill with a spinner/dot; (c) the bell (24 px icon + count) is never hidden. Hide into "…" only what still doesn't fit. When "…" hides anything with a count or attention state, "…" carries an aggregate dot or count. |
| 2 | P2 | narrow-*, "…" button | The overflow button is very faint (about 2:1 against the status bar in both themes), small, and has no hover/focus affordance visible. | Use the text-muted token (at least 3:1 as a UI component) and a 24×24 hit target. Give it a tooltip/aria-label such as "More status (2)". |
| 3 | P2 | wide-*, rail-* sidebar, "Activity" selected | The selected item is only a low tint (dark: about 1.3:1 against the sidebar) and the icon stays grey. In the overlay (rail-open, narrow-open) the selected icon **is** accent violet. The two states are inconsistent, and the dark selection is weak. | Use one selection style everywhere (guide §4 accents and §5): 14 % accent tint + accent icon (filled variant for the active item), plus 600 label weight or a 2 px accent bar. Apply it to expanded, rail and overlay alike. |
| 4 | P2 | all, job card progress bar | The bar is drawn at 100 % width and static while the job is "Still working". That reads as "done", and rule 11 forbids a static value when the total is unknown. The fill also fails 1.4.11: light lavender on white is about 2:1, and dark muted violet on surface-1 is about 2:1. | Unknown total: use an indeterminate bar (a moving 30 % segment, static and striped under reduced motion) plus a growing count ("12 s, 3 checks"). Known total: show the real fraction. Use the accent-500 fill (light #7546E0-class, dark #A58BFA-class) on a surface-2 track so both parts reach 3:1. |
| 5 | P2 | all, job card header | "Waiting for the device" appears twice: as the card title and again as the amber phase line. One value per fact (UI rule 7). | The title is the job name (what is being done, e.g. "Update runner"). The phase line is the phase ("Waiting for the device", step 2 of 4). |
| 6 | P2 | all, job card phase line | The phase line is in warning amber (#F2B84B / #8F5F00) for a normal, expected state. A status colour used as decoration breaks "calm by default", and amber is the warning hue. | Make the phase text-muted or text. Use warning only when the job is actually stalled past its threshold, and then with icon + word ("Slower than usual"). The "Still working" line already covers rule 12 without colour. |
| 7 | P2 | all, status bar bell count and "1 running" dot | Both the unread count pill and the running dot use the accent fill. The guide reserves accent for action and selection: "not for badges meaning attention" and "nav counts neutral". | The notification count is a neutral surface-3 pill, or danger tint only when an error is unread. The running indicator is a neutral spinner/dot (or info). |
| 8 | P2 | wide-*, rail-*, narrow-* menu bar "File View Help" | On macOS an in-window menu bar duplicates the native menu bar and looks web-like (ADR 0028, native feel). | On macOS hide the in-app menu and put the same commands in the native NSMenu (Tauri menu). Keep the in-app bar on Windows/Linux. Re-shoot one macOS frame showing the traffic-light inset (about 78 px left padding) so the title bar can be judged. |
| 9 | P2 | all, content and status bar left | Placeholder copy ("This page brings its own providers…", "Visit 1 of this page", "Greetings: 2", "Shell demo") is dev text. These shots will be the family's reference for the frame (UI rule 20: realistic data). | Fill the demo with plausible manager content (e.g. Pitcrew-like "Runners" page, a real status-bar item such as "3 runners online") or label the folder clearly as a dev harness and do not use these shots in READMEs. |
| 10 | P2 | wide-*, rail-*, narrow-* health chip | It is the loudest element in the frame: a bordered pill, a tint, 600 weight and a red square, inside a 24 px status bar. The guide's badge has no border. It is also unclear whether the chip opens a Repair action (rule 19). | Use a borderless tint pill (or the dot + word of rule 3) at 12/500. Clicking it opens a popover with the cause and a **Repair** button. Show that popover in one screenshot. |
| 11 | P3 | narrow-*, content gutter | The 24 px gutter at a 320 px width leaves a 272 px card and wraps the subtitle and helper text early. | Use a 16 px gutter below about 480 px container width (container query), as many native compact layouts do. |
| 12 | P3 | rail-open-light, narrow-open-light, bottom-left | The overlay's shadow bleeds below the panel onto the status bar (grey gradient over "Greetings"). | Clip the shadow to the panel's height, or let the overlay and scrim span down over the status bar consistently. |
| 13 | P3 | rail-open-*, narrow-open-* | The scrim covers the content but not the status bar or the menu bar. The status bar stays interactive while a modal overlay is open. | Choose one: a modal overlay with the scrim over everything below the title bar (focus trapped, Esc closes), or a non-modal overlay with no scrim. The first matches most native drawers. |
| 14 | P3 | rail-*, Activity count badge | The "2" badge overlaps the activity icon's top-right stroke. | Offset the badge to the icon's corner (−4/−4 from the icon box) with a 2 px ring in the sidebar colour, or use a 6 px dot in rail mode with the count in the tooltip. |
| 15 | P3 | narrow-* vs wide-*, toggle icon | The sidebar toggle changes glyph between widths (a sidebar icon at wide/rail, a hamburger at narrow) and the app name disappears at narrow. | Acceptable if intentional (overlay vs dock). Otherwise keep the sidebar glyph for consistency. Keep the app name (truncated with a tooltip) when space allows. |
| 16 | P3 | menu bar height | The title/menu bar is about 36 px. The guide gives a 44 px toolbar (Debrief 40). | Document the title bar as its own height (36) in the guide, or align it to 40/44, so the apps don't diverge. |
| 17 | P3 | all, focus | No shot shows keyboard focus. The pressed toggle (open shots) is a subtle tint only. | Add one focused-state shot (nav item, toggle, "…") showing the 2 px accent ring at a 2 px offset. Check that it is at least 3:1 in both themes. |

## Contrast notes (estimates)
- Text: the title, body, muted subtitle and status-bar text all pass 4.5:1 in both themes. Light amber phase text
  #8F5F00 on white is about 5.4:1 (passes, but see #6). Danger text on the light chip tint is about 4.9:1.
- UI parts below 3:1: the progress fill in both themes (#4), the "…" button (#3/#2), and the dark selected-row tint
  (the tint is decoration, so it needs the accent icon/bar to carry selection, #3).

## Answer to the 320 px question
Moving jobs and the bell behind "…" is **not acceptable as shown**. A hidden unread count and a hidden running job
with no trace on "…" violate "calm by default, loud on exception" in reverse: the exception gets hidden. It becomes
acceptable when items first shrink to icon form (health chip icon-only, jobs as a count pill), the bell with its count
is pinned, and "…" carries an aggregate badge for anything it hides (findings #1 and #2).
