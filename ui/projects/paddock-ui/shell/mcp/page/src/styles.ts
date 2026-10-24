/** The look of the AI access page (theme tokens only; no raw colours). Layout follows the container, not the window (rule 18). */
export const MCP_STYLES = `
  :host { display: flex; flex-direction: column; gap: var(--pd-space-5); color: var(--pd-text); line-height: 1.5; min-width: 0; container-type: inline-size; }
  h2, h3 { margin: 0; font-size: var(--pd-text-subtitle); font-weight: var(--pd-weight-semibold); }
  p { margin: 0; overflow-wrap: anywhere; }
  section { display: flex; flex-direction: column; gap: var(--pd-space-2); min-width: 0; }
  .pd-sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .pd-mc__muted { color: var(--pd-text-muted); }
  .pd-mc__btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
  .pd-mc__btn--primary { background: var(--pd-accent-fill); border-color: var(--pd-accent-fill); color: var(--pd-on-accent); }
  .pd-mc__btn:disabled, .pd-mc__btn[aria-disabled='true'] { opacity: 0.6; cursor: default; }
  .pd-mc__btn:focus-visible, input:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
  .pd-mc__master { display: flex; align-items: flex-start; gap: var(--pd-space-3); padding: var(--pd-space-3) var(--pd-space-4); border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); background: var(--pd-surface-1); }
  .pd-mc__master > span { min-width: 0; display: flex; flex-direction: column; gap: var(--pd-space-1); }
  .pd-mc__master strong { font-weight: var(--pd-weight-semibold); }
  .pd-mc__check { display: flex; align-items: flex-start; gap: var(--pd-space-2); min-width: 0; }
  .pd-mc__check > span { min-width: 0; display: flex; flex-direction: column; }
  .pd-mc__check strong { font-weight: var(--pd-weight-medium); overflow-wrap: anywhere; }
  .pd-mc__tools { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; }
  .pd-mc__tools li { padding: var(--pd-space-2) 0; border-bottom: 1px solid var(--pd-hairline); }
  .pd-mc__warn { padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-warning-tint); }
  .pd-mc__notice { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--pd-space-2); padding: var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-danger-tint); }
  .pd-mc__head { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: var(--pd-space-2) var(--pd-space-4); }
  .pd-mc__table { width: 100%; border-collapse: collapse; table-layout: fixed; }
  .pd-mc__table th { text-align: start; font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); color: var(--pd-text-muted); padding: var(--pd-space-1) var(--pd-space-2); border-bottom: 1px solid var(--pd-hairline); }
  .pd-mc__table td { padding: var(--pd-space-2); border-bottom: 1px solid var(--pd-hairline); vertical-align: middle; min-width: 0; overflow-wrap: anywhere; }
  .pd-mc__col-state { width: 7rem; }
  .pd-mc__col-created { width: 12rem; }
  .pd-mc__col-actions { width: 7rem; }
  .pd-mc__skeleton { display: flex; flex-direction: column; gap: var(--pd-space-2); }
  @container (max-width: 520px) {
    .pd-mc__col-created, .pd-mc__created { display: none; }
    .pd-mc__col-actions { width: 5.5rem; }
  }
`;
