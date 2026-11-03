# Dialog size: what the edges probes assume (ADR 0043)

A note for the Windows and Linux edge probes (2026-11-04):

- **Every kind has a floor of 320 x 200** (`DIALOG_MIN`), and the **viewer's kind floor is 680 x 560** (`VIEWER_MIN`). Both are the OS
  minimum (`min = max(kind floor, measured min content)`), so an inner height below 200, or a viewer below 560, is a bug.
  A probe expects `max(floor.height, min(measured, cap))` for `Height` kinds.
- A viewer therefore opens at 560 px or its content, whichever is larger, and never shrinks (`Shrink::Never`); the ADR's
  "opens at `min(content, preferred)`" is read with that floor (the table in ADR 0043 §1 now says so).
- A tool (`Manual`) opens at its contract's size, 960 x 680 by default.
