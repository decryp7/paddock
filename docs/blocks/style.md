## Style

- Match the surrounding code: naming, comment density (doc comments on public items, comments only where intent isn't
  obvious), idioms. Rust uses `rustfmt.toml` (max_width 140). Never run `prettier --write` on TypeScript.
- UI: standalone components, signals, OnPush, CSS custom properties for theme tokens (dark and light), accessible markup
  (real buttons, labels, aria-labels on icon buttons), sentence-case copy, verb plus object on buttons.
- UI building blocks: use **Angular CDK** for behaviour and accessibility (menus, overlays, dialogs, listbox, scrolling)
  instead of hand-rolling them, and **Angular Material first** (rule 47: use a Material component wherever one fits), themed through the
  `--pd-*` tokens. Icons are inline SVG registered with `MatIconRegistry`; no icon fonts and no icon URLs fetched at run time (the
  CSP forbids it).
- Status colours always come with words; validate colours for contrast and colour-vision deficiency.
