/** The look of the Report a Problem content (theme tokens only; no raw colours). */
export const DIAGNOSTICS_STYLES = `
  :host { display: flex; flex-direction: column; gap: var(--pd-space-4); color: var(--pd-text); line-height: 1.5; min-width: 0; container-type: inline-size; }
  h3 { margin: 0; font-size: var(--pd-text-subtitle); font-weight: var(--pd-weight-semibold); }
  p { margin: 0; }
  .muted { color: var(--pd-text-muted); }
  .lede { font-weight: var(--pd-weight-medium); }
  .field { display: flex; flex-direction: column; gap: var(--pd-space-1); }
  .field label { font-weight: var(--pd-weight-medium); }
  textarea { box-sizing: border-box; width: 100%; min-height: 6em; resize: vertical; padding: var(--pd-space-2); border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; }
  .scroller { max-height: 300px; overflow: auto; border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-control); }
  table { width: 100%; border-collapse: collapse; }
  th, td { padding: var(--pd-space-1) var(--pd-space-3); text-align: left; vertical-align: top; }
  thead th { position: sticky; top: 0; background: var(--pd-surface-1); color: var(--pd-text-muted); font-weight: var(--pd-weight-medium); border-bottom: 1px solid var(--pd-border-control); }
  tbody tr + tr > * { border-top: 1px solid var(--pd-border-subtle); }
  tbody th { font-weight: var(--pd-weight-medium); font-family: var(--pd-font-mono); font-size: var(--pd-text-caption); white-space: nowrap; }
  td.num, th.num { text-align: right; font-variant-numeric: tabular-nums; white-space: nowrap; }
  @container (max-width: 520px) { td.what, th.what { display: none; } }
  .row { display: flex; flex-wrap: wrap; align-items: center; gap: var(--pd-space-2); }
  .actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: var(--pd-space-2); }
  .btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
  .btn.primary { background: var(--pd-accent-fill); border-color: var(--pd-accent-fill); color: var(--pd-on-accent); }
  .btn:disabled { opacity: 0.6; cursor: default; }
  .notice { display: flex; flex-direction: column; gap: var(--pd-space-1); padding: var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); }
  .notice.danger { background: var(--pd-danger-tint); }
  .notice .title { font-weight: var(--pd-weight-medium); }
  .notice .detail { color: var(--pd-text-muted); overflow-wrap: anywhere; }
  .path { font-family: var(--pd-font-mono); font-size: var(--pd-text-caption); white-space: nowrap; overflow: hidden; }
  ul { margin: 0; padding-left: var(--pd-space-6); }
  progress { width: 100%; }
`;
