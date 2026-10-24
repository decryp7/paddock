/** The look of the Storage page (theme tokens only; no raw colours). */
export const STORAGE_STYLES = `
  :host { display: flex; flex-direction: column; gap: var(--pd-space-4); color: var(--pd-text); line-height: 1.5; min-width: 0; container-type: inline-size; }
  h2 { margin: 0; font-size: var(--pd-text-subtitle); font-weight: var(--pd-weight-semibold); }
  p { margin: 0; overflow-wrap: anywhere; }
  .pd-storage__muted { color: var(--pd-text-muted); }
  .pd-storage__sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .pd-storage__summary { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--pd-space-2) var(--pd-space-3); }
  .pd-storage__row { display: flex; flex-wrap: wrap; gap: var(--pd-space-2); }
  .pd-storage__btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
  .pd-storage__btn--primary { background: var(--pd-accent-fill); border-color: var(--pd-accent-fill); color: var(--pd-on-accent); }
  .pd-storage__btn:disabled { opacity: 0.6; cursor: default; }
  .pd-storage__btn:focus-visible { outline: 2px solid var(--pd-accent); outline-offset: 1px; }
  .pd-storage__notice { display: flex; flex-direction: column; gap: var(--pd-space-1); padding: var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); min-width: 0; }
  .pd-storage__notice--danger { background: var(--pd-danger-tint); }
  .pd-storage__notice--success { background: var(--pd-success-tint); }
  .pd-storage__title { font-weight: var(--pd-weight-medium); }
  progress { width: 100%; }
  details ul { margin: var(--pd-space-1) 0 0; padding-inline-start: var(--pd-space-4); }
  .pd-storage__table { width: 100%; border-collapse: collapse; table-layout: fixed; }
  .pd-storage__table th { text-align: start; font-weight: var(--pd-weight-semibold); color: var(--pd-text-muted); font-size: var(--pd-text-label); padding: var(--pd-space-1) var(--pd-space-2); border-bottom: 1px solid var(--pd-hairline); }
  .pd-storage__table td { padding: var(--pd-space-2); border-bottom: 1px solid var(--pd-hairline); vertical-align: top; min-width: 0; }
  .pd-storage__name { display: flex; flex-direction: column; gap: 2px; overflow-wrap: anywhere; }
  .pd-storage__kind { font-size: var(--pd-text-label); color: var(--pd-text-muted); }
  .pd-storage__note { font-size: var(--pd-text-label); }
  .pd-storage__size { text-align: end; white-space: nowrap; width: 8em; font-variant-numeric: tabular-nums; }
  .pd-storage__actions { width: 13em; text-align: end; }
  .pd-storage__actions .pd-storage__btn { margin-inline-start: var(--pd-space-1); }
  .pd-storage__tree { display: flex; flex-direction: column; gap: var(--pd-space-2); min-width: 0; }
  .pd-storage__tree h3 { margin: 0; font-size: var(--pd-text-body); font-weight: var(--pd-weight-semibold); }
  tr:hover td { background: var(--pd-hover); }
  /* A narrow page drops the folder column (the path stays in the Show button's folder and in the tooltip of the name) and stacks the buttons. */
  @container (max-width: 560px) { .pd-storage__folder { display: none; }  .pd-storage__size { width: 6em; } }
  @container (max-width: 420px) { .pd-storage__summary .pd-storage__btn { flex: 1 1 100%; } }
`;
