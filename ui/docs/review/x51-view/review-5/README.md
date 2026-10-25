# Contact sheet of `<pd-view>`: review 5 (NOT self-approved)

Plan X51, ADR 0002. Made from the real components after the fixes for review 4 (`../REVIEW.md`, `../FIXES.md`). Every section (kinds, states, interaction, 320 px) was captured in one run from one commit: `1a75abca60`. Awaiting the independent reviewer (rule 22).

Open the `sheet-*.png` files: each is one section, dark on the left and light on the right, no taller than about 3,000 px (6,000 px of bitmap), readable at 100%.
The single images (`dark-*.png`, `light-*.png`) are the same shots at 2x; a subject taller than 1,800 px is cut into numbered slices (`-1`, `-2`).

- `sheet-text.png`: Text, badges and progress
- `sheet-data.png`: Tables, trees, matrices and charts
- `sheet-drawings.png`: Graphs and diagrams
- `sheet-form.png`: Form and review list
- `sheet-states.png`: Empty, needs-AI, unknown, stale, invalid, busy and loading
- `sheet-states-2.png`: Empty, needs-AI, unknown, stale, invalid, busy and loading (2)
- `sheet-states-3.png`: Empty, needs-AI, unknown, stale, invalid, busy and loading (3)
- `sheet-interaction.png`: Keyboard focus and hover
- `sheet-narrow.png`: The whole view at 320 px
- `sheet-narrow-2.png`: The whole view at 320 px (2)
- `sheet-narrow-3.png`: The whole view at 320 px (3)

Rewritten by `CONTACT_SHEET=1 npx playwright test --project=review contact-sheet-x51` (rules 22 and 18, UI patterns 2, 5, 6, 8, 9, 15).
