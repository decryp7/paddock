/** The look of the Background service page (theme tokens only; no raw colours). */
export const SERVICE_STYLES = `
  :host { display: flex; flex-direction: column; gap: var(--pd-space-4); color: var(--pd-text); line-height: 1.5; min-width: 0; container-type: inline-size; }
  h2 { margin: 0; font-size: var(--pd-text-subtitle); font-weight: var(--pd-weight-semibold); }
  p { margin: 0; overflow-wrap: anywhere; }
  .pd-service__muted { color: var(--pd-text-muted); }
  .pd-service__status { display: flex; flex-wrap: wrap; align-items: center; gap: var(--pd-space-2) var(--pd-space-3); min-width: 0; }
  .pd-service__actions { display: flex; flex-wrap: wrap; gap: var(--pd-space-2); }
  .pd-service__btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
  .pd-service__btn--primary { background: var(--pd-accent-fill); border-color: var(--pd-accent-fill); color: var(--pd-on-accent); }
  .pd-service__btn:disabled { opacity: 0.6; cursor: default; }
  .pd-service__btn:focus-visible { outline: 2px solid var(--pd-accent); outline-offset: 1px; }
  .pd-service__notice { display: flex; flex-direction: column; gap: var(--pd-space-1); padding: var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); min-width: 0; }
  .pd-service__notice--danger { background: var(--pd-danger-tint); }
  .pd-service__notice--success { background: var(--pd-success-tint); }
  .pd-service__title { font-weight: var(--pd-weight-medium); }
  .pd-service__row { display: flex; flex-wrap: wrap; gap: var(--pd-space-2); }
  progress { width: 100%; }
  @container (max-width: 420px) { .pd-service__actions .pd-service__btn { flex: 1 1 100%; } }
`;
