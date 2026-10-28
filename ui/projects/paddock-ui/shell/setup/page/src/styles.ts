/** The look of the Set-up checklist (theme tokens only; no raw colours). */
export const SETUP_STYLES = `
  :host { display: flex; flex-direction: column; gap: var(--pd-space-4); color: var(--pd-text); line-height: 1.5; min-width: 0; container-type: inline-size; }
  h2 { margin: 0; font-size: var(--pd-text-subtitle); font-weight: var(--pd-weight-semibold); }
  h3 { margin: 0; font-size: var(--pd-text-body); font-weight: var(--pd-weight-semibold); min-width: 0; overflow-wrap: anywhere; }
  p { margin: 0; overflow-wrap: anywhere; }
  .pd-setup__muted { color: var(--pd-text-muted); }
  .pd-setup__list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; }
  .pd-setup__item { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: var(--pd-space-2) var(--pd-space-4); padding: var(--pd-space-3) 0; border-top: 1px solid var(--pd-border-subtle); min-width: 0; }
  .pd-setup__item:first-child { border-top: 0; }
  .pd-setup__body { display: flex; flex-direction: column; gap: var(--pd-space-1); min-width: 0; }
  .pd-setup__head { display: flex; flex-wrap: wrap; align-items: center; gap: var(--pd-space-1) var(--pd-space-2); min-width: 0; }
  .pd-setup__actions { display: flex; flex-wrap: wrap; align-items: flex-start; justify-content: flex-end; gap: var(--pd-space-2); }
  .pd-setup__notice { display: flex; flex-direction: column; gap: var(--pd-space-1); padding: var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); min-width: 0; }
  .pd-setup__notice--danger { background: var(--pd-danger-tint); }
  .pd-setup__title { font-weight: var(--pd-weight-medium); }
  .pd-setup__row { display: flex; flex-wrap: wrap; gap: var(--pd-space-2); }
  .pd-setup__footer { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--pd-space-2); }
  mat-progress-bar { width: 100%; }
  @container (max-width: 420px) {
    .pd-setup__item { grid-template-columns: minmax(0, 1fr); }
    .pd-setup__actions { justify-content: flex-start; }
    .pd-setup__actions button { flex: 1 1 100%; }
  }
`;
