/** The look of the Notification channels page (theme tokens only; no raw colours). Layout follows the container, not the window (rule 18). */
export const CHANNELS_STYLES = `
  :host { display: flex; flex-direction: column; gap: var(--pd-space-4); color: var(--pd-text); line-height: 1.5; min-width: 0; container-type: inline-size; }
  h2, h3 { margin: 0; font-size: var(--pd-text-subtitle); font-weight: var(--pd-weight-semibold); }
  p { margin: 0; overflow-wrap: anywhere; }
  .pd-ch__muted { color: var(--pd-text-muted); }
  .pd-ch__head { display: flex; flex-wrap: wrap; align-items: flex-start; justify-content: space-between; gap: var(--pd-space-2) var(--pd-space-4); }
  .pd-ch__head > div { min-width: 0; flex: 1 1 16rem; display: flex; flex-direction: column; gap: var(--pd-space-1); }
  .pd-ch__btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
  .pd-ch__btn--primary { background: var(--pd-accent-fill); border-color: var(--pd-accent-fill); color: var(--pd-on-accent); }
  .pd-ch__btn:disabled, .pd-ch__btn[aria-disabled='true'] { opacity: 0.6; cursor: default; }
  .pd-ch__btn:focus-visible, .pd-ch__icon:focus-visible, input:focus-visible, textarea:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
  .pd-ch__table { width: 100%; border-collapse: collapse; table-layout: fixed; }
  .pd-ch__table th { text-align: start; font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); color: var(--pd-text-muted); padding: var(--pd-space-1) var(--pd-space-2); border-bottom: 1px solid var(--pd-hairline); }
  .pd-ch__table td { padding: var(--pd-space-2); border-bottom: 1px solid var(--pd-hairline); vertical-align: top; min-width: 0; }
  .pd-ch__table tbody tr:hover, .pd-ch__table tbody tr:focus-within { background: var(--pd-hover); }
  .pd-ch__table tr.pd-ch__editing { background: var(--pd-surface-2); }
  .pd-ch__name { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .pd-ch__name strong { font-weight: var(--pd-weight-medium); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pd-ch__col-type { width: 6rem; }
  .pd-ch__col-events { width: 30%; }
  .pd-ch__col-state { width: 10rem; }
  .pd-ch__col-actions { width: 7.5rem; }
  .pd-ch__states { display: flex; flex-wrap: wrap; gap: var(--pd-space-1); }
  .pd-ch__actions { display: flex; flex-wrap: wrap; justify-content: flex-end; align-items: center; gap: 2px; opacity: 0; }
  tr:hover .pd-ch__actions, tr:focus-within .pd-ch__actions, .pd-ch__actions--open { opacity: 1; }
  @media (hover: none) { .pd-ch__actions { opacity: 1; } }
  .pd-ch__icon { display: inline-flex; align-items: center; justify-content: center; width: 28px; height: 28px; padding: 0; border: 0; border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text-muted); cursor: pointer; }
  .pd-ch__icon:hover:not(:disabled) { background: var(--pd-hover); color: var(--pd-text); }
  .pd-ch__icon:disabled { opacity: 0.5; cursor: default; }
  .pd-ch__test { display: inline-flex; align-items: center; gap: var(--pd-space-2); padding: 2px var(--pd-space-2); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); font-size: var(--pd-text-label); overflow-wrap: anywhere; min-width: 0; }
  .pd-ch__test--ok { background: var(--pd-success-tint); }
  .pd-ch__test--bad { background: var(--pd-danger-tint); }
  .pd-ch__spin { flex: none; width: 12px; height: 12px; border-radius: 50%; border: 2px solid var(--pd-border-control); border-top-color: var(--pd-accent); animation: pd-ch-spin 0.9s linear infinite; }
  @keyframes pd-ch-spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .pd-ch__spin { animation: none; } }
  .pd-ch__progress { display: block; color: var(--pd-text-muted); }
  .pd-ch__notice { display: flex; flex-direction: column; gap: var(--pd-space-1); padding: var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-danger-tint); min-width: 0; }
  .pd-ch__notice strong { font-weight: var(--pd-weight-medium); }
  .pd-ch__skeleton { display: flex; flex-direction: column; gap: var(--pd-space-2); }
  .pd-ch__editor { display: flex; flex-direction: column; gap: var(--pd-space-3); padding: var(--pd-space-4); border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); background: var(--pd-surface-1); min-width: 0; }
  .pd-ch__grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--pd-space-3); }
  .pd-ch__field { display: flex; flex-direction: column; gap: var(--pd-space-1); min-width: 0; font-weight: var(--pd-weight-medium); }
  .pd-ch__field--wide { grid-column: 1 / -1; }
  .pd-ch__field input[type='text'], .pd-ch__field input[type='number'], .pd-ch__field textarea { box-sizing: border-box; width: 100%; min-height: var(--pd-control-height); padding: 0 var(--pd-space-2); border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-weight: normal; min-width: 0; }
  .pd-ch__field textarea { padding: var(--pd-space-1) var(--pd-space-2); resize: vertical; }
  .pd-ch__field [aria-invalid='true'] { border-color: var(--pd-danger); }
  .pd-ch__hint { color: var(--pd-text-muted); font-weight: normal; font-size: var(--pd-text-caption); }
  .pd-ch__hint--bad { color: var(--pd-danger-text, var(--pd-text)); }
  .pd-ch__check { display: flex; align-items: center; gap: var(--pd-space-2); font-weight: var(--pd-weight-medium); }
  .pd-ch__secret { display: flex; flex-wrap: wrap; align-items: center; gap: var(--pd-space-2) var(--pd-space-3); padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); grid-column: 1 / -1; }
  .pd-ch__foot { display: flex; flex-wrap: wrap; align-items: center; gap: var(--pd-space-2); }
  .pd-ch__foot .pd-ch__spacer { flex: 1 1 0; }
  @container (max-width: 560px) {
    .pd-ch__grid { grid-template-columns: minmax(0, 1fr); }
    .pd-ch__col-type, .pd-ch__col-events, .pd-ch__type, .pd-ch__events { display: none; }
    .pd-ch__col-state { width: 7rem; }
    .pd-ch__col-actions { width: 5.5rem; }
  }
  @container (max-width: 420px) { .pd-ch__foot .pd-ch__btn { flex: 1 1 100%; } }
`;
