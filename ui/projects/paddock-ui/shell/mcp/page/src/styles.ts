/** The look of the AI access page (theme tokens only; no raw colours). Layout follows the container, not the window (rule 18). */
export const MCP_STYLES = `
  :host { display: flex; flex-direction: column; gap: var(--pd-space-6); color: var(--pd-text); line-height: 1.5; min-width: 0; container-type: inline-size; }
  h2, h3 { margin: 0; font-size: var(--pd-text-subtitle); font-weight: var(--pd-weight-semibold); }
  p { margin: 0; overflow-wrap: anywhere; }
  section { display: flex; flex-direction: column; gap: var(--pd-space-2); min-width: 0; }
  .pd-sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .pd-mc__muted { color: var(--pd-text-muted); }
  .pd-mc__master { padding: var(--pd-space-3) var(--pd-space-4); border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); background: var(--pd-surface-1); }
  .pd-mc__label { min-width: 0; display: flex; flex-direction: column; }
  .pd-mc__master strong { font-weight: var(--pd-weight-semibold); }
  .pd-mc__check { min-width: 0; margin-inline-start: calc((var(--pd-control-height) - 18px) / -2); }
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
