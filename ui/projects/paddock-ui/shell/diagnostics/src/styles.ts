/** The look of the Report a Problem content (theme tokens only; no raw colours). */
export const DIAGNOSTICS_STYLES = `
  :host { display: flex; flex-direction: column; gap: var(--pd-space-4); color: var(--pd-text); line-height: 1.5; min-width: 0; container-type: inline-size; }
  h3 { margin: 0; font-size: var(--pd-text-subtitle); font-weight: var(--pd-weight-semibold); }
  p { margin: 0; }
  .pd-diag__muted { color: var(--pd-text-muted); }
  .pd-diag__lede { color: var(--pd-text-muted); }
  .pd-diag__field { display: flex; flex-direction: column; gap: var(--pd-space-1); }
  .pd-diag__field label { color: var(--pd-text-muted); font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); }
  textarea { box-sizing: border-box; width: 100%; min-height: 6em; resize: vertical; padding: var(--pd-space-2); border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; }
  .pd-diag__scroller { max-height: 300px; overflow: auto; border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-control); }
  table { width: 100%; border-collapse: collapse; }
  th, td { padding: var(--pd-space-1) var(--pd-space-3); text-align: left; vertical-align: top; }
  thead th { position: sticky; top: 0; background: var(--pd-surface-1); color: var(--pd-text-muted); font-weight: var(--pd-weight-medium); border-bottom: 1px solid var(--pd-border-control); }
  tbody tr + tr > * { border-top: 1px solid var(--pd-border-subtle); }
  tbody th.pd-diag__name { font-weight: var(--pd-weight-medium); font-family: var(--pd-font-mono); font-size: var(--pd-text-caption); overflow-wrap: anywhere; word-break: break-all; hyphens: none; }
  .pd-diag__what { overflow-wrap: anywhere; }
  .pd-diag__num { min-width: 6.5rem; text-align: right; font-variant-numeric: tabular-nums; white-space: nowrap; }
  @container (max-width: 520px) { .pd-diag__what { display: none; } }
  .pd-diag__row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--pd-space-2); }
  .pd-diag__btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
  .pd-diag__btn:disabled { opacity: 0.6; cursor: default; }
  .pd-diag__notice { display: flex; flex-direction: column; gap: var(--pd-space-1); padding: var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); }
  .pd-diag__notice--danger { background: var(--pd-danger-tint); }
  .pd-diag__notice .pd-diag__title { font-weight: var(--pd-weight-medium); }
  .pd-diag__notice .pd-diag__detail { color: var(--pd-text-muted); overflow-wrap: anywhere; }
  .pd-diag__path { font-family: var(--pd-font-mono); font-size: var(--pd-text-caption); white-space: nowrap; overflow: hidden; }
  ul { margin: 0; padding-left: var(--pd-space-6); }
  progress { width: 100%; }
`;
