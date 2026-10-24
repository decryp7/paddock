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
  .pd-storage__btn:disabled { opacity: 0.6; cursor: default; }
  .pd-storage__btn:focus-visible { outline: 2px solid var(--pd-accent); outline-offset: 1px; }
  .pd-storage__notice { display: flex; flex-direction: column; gap: var(--pd-space-1); padding: var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); min-width: 0; }
  .pd-storage__notice--danger { background: var(--pd-danger-tint); }
  .pd-storage__notice--success { background: var(--pd-success-tint); }
  .pd-storage__title { font-weight: var(--pd-weight-medium); }
  progress { width: 100%; }
  details ul { margin: var(--pd-space-1) 0 0; padding-inline-start: var(--pd-space-4); }
  .pd-storage__name { display: flex; align-items: baseline; gap: var(--pd-space-2); min-width: 0; overflow: hidden; }
  .pd-storage__name .pd-storage__title { flex: none; max-width: 100%; }
  .pd-storage__note { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--pd-text-muted); font-size: var(--pd-text-label); font-weight: var(--pd-weight-regular); }
  .pd-storage__item { display: flex; align-items: center; gap: var(--pd-space-2); min-width: 0; }
  .pd-storage__item pd-glyph { color: var(--pd-text-muted); }
  .pd-storage__link { border: 0; background: transparent; color: var(--pd-text-muted); font: inherit; text-decoration: underline; cursor: pointer; padding: var(--pd-space-1) var(--pd-space-2); border-radius: var(--pd-radius-control); }
  .pd-storage__link:hover { color: var(--pd-text); }
  .pd-storage__link:focus-visible { outline: 2px solid var(--pd-focus-ring); }
  .pd-storage__tree-head { display: flex; align-items: center; justify-content: space-between; gap: var(--pd-space-2); }
  .pd-storage__tree { display: flex; flex-direction: column; gap: var(--pd-space-2); min-width: 0; }
  .pd-storage__tree h3 { margin: 0; font-size: var(--pd-text-body); font-weight: var(--pd-weight-semibold); }
`;
