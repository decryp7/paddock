/**
 * The shared look of the About, Updates and What's New contents (theme tokens only; rule: no raw colours). Class names
 * carry the `pd-` prefix, as the templates use them. Type scale (design guide): a dialog's own title 18/600
 * (`.pd-title`), a section heading 15/600 (`h3`, `.pd-heading`), a label 12/500 muted (`.pd-label`, a fact's name).
 */
export const ABOUT_STYLES = `
  :host { display: flex; flex-direction: column; gap: var(--pd-space-4); color: var(--pd-text); line-height: 1.5; min-width: 0; }
  h2 { margin: 0; font-size: var(--pd-text-title); font-weight: var(--pd-weight-semibold); }
  h3 { margin: 0 0 var(--pd-space-2); font-size: var(--pd-text-subtitle); font-weight: var(--pd-weight-semibold); }
  p { margin: 0; }
  .pd-muted { color: var(--pd-text-muted); }
  .pd-label { color: var(--pd-text-muted); font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); }
  .pd-facts { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: var(--pd-space-2) var(--pd-space-4); margin: 0; }
  .pd-facts dt { color: var(--pd-text-muted); }
  .pd-facts dd { margin: 0; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-variant-numeric: tabular-nums; }
  .pd-link { color: var(--pd-accent); text-decoration: underline; text-underline-offset: 2px; }
  .pd-error { margin: 0; padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-danger-tint); overflow-wrap: anywhere; }
  .pd-row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--pd-space-2); }
  .pd-actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: var(--pd-space-2); }
  .pd-btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
  .pd-btn.pd-primary { background: var(--pd-accent-fill); border-color: var(--pd-accent-fill); color: var(--pd-on-accent); }
  .pd-btn[aria-pressed='true'] { border-color: var(--pd-accent-fill); }
  .pd-btn:disabled { opacity: 0.6; cursor: default; }
  .pd-text { margin: 0; padding: var(--pd-space-3); max-height: 320px; overflow: auto; border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-control); background: var(--pd-surface-1); font-family: var(--pd-font-mono); font-size: var(--pd-text-caption); white-space: pre-wrap; overflow-wrap: anywhere; }
  .pd-problem { display: flex; flex-direction: column; gap: var(--pd-space-1); padding: var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-danger-tint); }
  .pd-problem .pd-message { font-weight: var(--pd-weight-medium); }
  .pd-problem .pd-detail { color: var(--pd-text-muted); overflow-wrap: anywhere; }
  .pd-prefs { display: flex; flex-direction: column; gap: var(--pd-space-3); }
  label.pd-check { display: inline-flex; align-items: center; gap: var(--pd-space-2); }
  label.pd-field { display: flex; flex-direction: column; align-items: flex-start; gap: var(--pd-space-1); }
  select { min-height: var(--pd-button-height); border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; }
  ul { margin: 0; padding-left: var(--pd-space-6); }
  li + li { margin-top: var(--pd-space-1); }
`;
