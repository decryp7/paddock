/** The look of the Knowledge packs page (theme tokens only; no raw colours). */
export const PACKS_STYLES = `
  :host { display: flex; flex-direction: column; gap: var(--pd-space-4); color: var(--pd-text); line-height: 1.5; min-width: 0; container-type: inline-size; }
  h2 { margin: 0; font-size: var(--pd-text-subtitle); font-weight: var(--pd-weight-semibold); }
  h3 { margin: 0; font-size: var(--pd-text-body); font-weight: var(--pd-weight-semibold); }
  p { margin: 0; overflow-wrap: anywhere; }
  ul { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: var(--pd-space-1); }
  .pd-packs__muted { color: var(--pd-text-muted); }
  .pd-packs__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--pd-space-2); }
  .pd-packs__kind { display: flex; flex-direction: column; gap: var(--pd-space-2); min-width: 0; }
  .pd-packs__kind-title { display: flex; align-items: center; gap: var(--pd-space-2); min-width: 0; }
  .pd-packs__ok { color: var(--pd-success); }
  .pd-packs__warn { color: var(--pd-warning); }
  .pd-packs__bad { color: var(--pd-danger); }
  .pd-packs__row { display: flex; align-items: center; gap: var(--pd-space-2); min-width: 0; }
  .pd-packs__row pd-path-text { flex: 1 1 auto; }
  .pd-packs__origin { flex: none; color: var(--pd-text-muted); font-size: var(--pd-text-label); }
  .pd-packs__problem { display: flex; gap: var(--pd-space-2); align-items: flex-start; padding: var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-danger-tint); min-width: 0; }
  .pd-packs__problem > div { display: flex; flex-direction: column; min-width: 0; }
  .pd-packs__file { font-weight: var(--pd-weight-medium); overflow-wrap: anywhere; }
  .pd-packs__note { display: flex; gap: var(--pd-space-2); align-items: flex-start; min-width: 0; color: var(--pd-text-muted); }
  .pd-packs__notice { display: flex; flex-direction: column; gap: var(--pd-space-1); padding: var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-danger-tint); min-width: 0; }
  .pd-packs__title { font-weight: var(--pd-weight-medium); }
  .pd-packs__btn { display: inline-flex; align-items: center; gap: var(--pd-space-1); min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
  .pd-packs__btn--primary { background: var(--pd-accent-fill); border-color: var(--pd-accent-fill); color: var(--pd-on-accent); }
  .pd-packs__btn:disabled { opacity: 0.6; cursor: default; }
  .pd-packs__btn:focus-visible { outline: 2px solid var(--pd-accent); outline-offset: 1px; }
  progress { width: 100%; }
  @container (max-width: 420px) { .pd-packs__head .pd-packs__btn { flex: 1 1 100%; justify-content: center; } }
`;
